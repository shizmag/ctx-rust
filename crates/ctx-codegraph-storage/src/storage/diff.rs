use crate::backend::{BackendRegistry, global_registry};
use crate::error::CodeGraphError;
use crate::index::BuildIndexOptions;
use crate::model::{
    FileChangeDetection, FileParseStatus, FileSnapshot, IndexDiff, IndexState, Language,
    RebuildReason,
};
use std::path::{Path, PathBuf};

use super::compat::check_db_compatibility_with_registry;
use super::workspace::find_workspace_root;

pub fn compute_index_diff(
    conn: &rusqlite::Connection,
    workspace_root: &Path,
    options: &BuildIndexOptions,
) -> Result<IndexDiff, CodeGraphError> {
    compute_index_diff_with_registry(conn, workspace_root, options, global_registry())
}

pub fn compute_index_diff_with_registry(
    conn: &rusqlite::Connection,
    workspace_root: &Path,
    options: &BuildIndexOptions,
    registry: &BackendRegistry,
) -> Result<IndexDiff, CodeGraphError> {
    let mut disk_files = std::collections::HashSet::new();
    let walker = walkdir::WalkDir::new(workspace_root)
        .into_iter()
        .filter_entry(|e| {
            let path = e.path();
            if path.is_dir()
                && let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && crate::discovery::should_skip_dir(name) {
                        return false;
                    }
            true
        });
    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file()
            && crate::index::should_index_path_with_registry(path, registry) {
                disk_files.insert(path.to_path_buf());
            }
    }

    let mut db_files = std::collections::HashMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT path, rel_path, language, backend_id, mtime_ms, size_bytes, content_hash, parser_id, parser_version, parser_config_hash, parse_status FROM files")?;
        let db_files_rows = stmt.query_map([], |row| {
            let path_str: String = row.get(0)?;
            let rel_path_str: String = row.get(1)?;
            let language: String = row.get(2)?;
            let backend_id: String = row.get(3)?;
            let mtime_ms: i64 = row.get(4)?;
            let size_bytes: u64 = row.get(5)?;
            let content_hash: Option<String> = row.get(6)?;
            let parser_id: String = row.get(7)?;
            let parser_version: String = row.get(8)?;
            let parser_config_hash: String = row.get(9)?;
            let parse_status_str: String = row.get(10)?;
            let parse_status =
                FileParseStatus::from_str(&parse_status_str).unwrap_or(FileParseStatus::Success);
            Ok((
                PathBuf::from(path_str),
                (
                    PathBuf::from(rel_path_str),
                    language,
                    backend_id,
                    mtime_ms,
                    size_bytes,
                    content_hash,
                    parser_id,
                    parser_version,
                    parser_config_hash,
                    parse_status,
                ),
            ))
        })?;

        for row in db_files_rows {
            let (path, val) = row?;
            db_files.insert(path, val);
        }
    }

    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut deleted = Vec::new();
    let mut unchanged = Vec::new();

    for path in &disk_files {
        let disk_mtime = crate::index::get_mtime_ms(path).unwrap_or(0);
        let disk_size = crate::index::get_size_bytes(path).unwrap_or(0) as u64;

        if let Some((
            rel_path,
            db_lang,
            db_backend_id,
            db_mtime,
            db_size,
            db_hash,
            db_parser_id,
            db_parser_version,
            db_parser_config_hash,
            db_parse_status,
        )) = db_files.get(path)
        {
            let mut disk_hash = None;
            let is_modified = match options.change_detection {
                FileChangeDetection::MtimeAndSize => {
                    disk_mtime != *db_mtime
                        || disk_size != *db_size
                        || *db_parse_status == FileParseStatus::Failed
                }
                FileChangeDetection::ContentHash => {
                    let computed = crate::index::compute_file_hash(path);
                    disk_hash = computed.clone();
                    computed != *db_hash || *db_parse_status == FileParseStatus::Failed
                }
            };

            let snapshot = FileSnapshot {
                file_id: None,
                rel_path: rel_path.clone(),
                abs_path: path.clone(),
                language: Language(db_lang.clone()),
                backend_id: db_backend_id.clone(),
                size_bytes: disk_size,
                mtime_ms: disk_mtime,
                mtime_ns: None,
                content_hash: disk_hash.or_else(|| db_hash.clone()),
                parser_id: db_parser_id.clone(),
                parser_version: db_parser_version.clone(),
                parser_config_hash: db_parser_config_hash.clone(),
                indexed_at_ms: None,
                parse_status: *db_parse_status,
            };

            if is_modified {
                modified.push(snapshot);
            } else {
                unchanged.push(snapshot);
            }
        } else {
            let snapshot = crate::index::create_file_snapshot_with_registry(
                workspace_root,
                path,
                options.change_detection,
                options.include_tests,
                registry,
            );
            added.push(snapshot);
        }
    }

    for path in db_files.keys() {
        if !disk_files.contains(path) {
            deleted.push(path.clone());
        }
    }

    Ok(IndexDiff {
        added,
        modified,
        deleted,
        unchanged,
    })
}

pub fn get_index_state(
    root: &Path,
    options: &BuildIndexOptions,
) -> Result<IndexState, CodeGraphError> {
    get_index_state_with_registry(root, options, global_registry())
}

pub fn get_index_state_with_registry(
    root: &Path,
    options: &BuildIndexOptions,
    registry: &BackendRegistry,
) -> Result<IndexState, CodeGraphError> {
    let workspace_root = find_workspace_root(root);
    let db_path = workspace_root.join(".ctx-codegraph/codegraph.sqlite");
    if !db_path.exists() {
        return Ok(IndexState::NeedsFullRebuild(RebuildReason::MissingDatabase));
    }

    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(_) => {
            return Ok(IndexState::NeedsFullRebuild(RebuildReason::CorruptDatabase));
        }
    };

    if conn.execute("PRAGMA foreign_keys = ON;", []).is_err() {
        return Ok(IndexState::NeedsFullRebuild(RebuildReason::CorruptDatabase));
    }

    if let Some(reason) = check_db_compatibility_with_registry(&conn, options, registry)? {
        return Ok(IndexState::NeedsFullRebuild(reason));
    }

    let diff = compute_index_diff_with_registry(&conn, &workspace_root, options, registry)?;
    if diff.added.is_empty() && diff.modified.is_empty() && diff.deleted.is_empty() {
        if options.use_lsp {
            let lsp_status = conn
                .query_row(
                    "SELECT value FROM metadata WHERE key = 'lsp_enrichment'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap_or_else(|_| "none".to_string());
            if lsp_status == "none" {
                return Ok(IndexState::NeedsIncrementalUpdate(diff));
            }
        }
        Ok(IndexState::Ready)
    } else {
        Ok(IndexState::NeedsIncrementalUpdate(diff))
    }
}

pub fn validate_index_db(root: &Path, options: &BuildIndexOptions) -> Result<bool, CodeGraphError> {
    validate_index_db_with_registry(root, options, global_registry())
}

pub fn validate_index_db_with_registry(
    root: &Path,
    options: &BuildIndexOptions,
    registry: &BackendRegistry,
) -> Result<bool, CodeGraphError> {
    match get_index_state_with_registry(root, options, registry)? {
        IndexState::Ready => Ok(true),
        _ => Ok(false),
    }
}
