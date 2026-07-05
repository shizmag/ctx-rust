use crate::backend::{BackendRegistry, ParseInput, ParsedFile, ResolveInput, global_registry};
use crate::error::CodeGraphError;
use crate::index::BuildIndexOptions;
use crate::model::{
    AffectedSet, CallEdge, CallId, CallSite, CodeIndex, EdgeKind, FileChangeDetection, FileId,
    FileParseStatus, FileSnapshot, IndexDiff, IndexState, Language, LanguageObject,
    LanguageObjectKind, RebuildReason, ResolutionConfidence, SourceRange, Symbol, SymbolId,
    SymbolKind, SymbolResolution, TextRange, Occurrence, OccurrenceId, OccurrenceKind, EdgeId, GraphEdge, LanguageId,
};
use crate::resolver::noop::resolve_name_only;
use std::path::{Path, PathBuf};

pub fn find_workspace_root(start_dir: &Path) -> PathBuf {
    let mut current = match start_dir.canonicalize() {
        Ok(path) => path,
        Err(_) => start_dir.to_path_buf(),
    };
    let registry = crate::backend::global_registry();
    loop {
        let mut matches = current.join(".git").exists() || current.join(".ctxconfig").exists();
        if !matches {
            for backend in registry.all() {
                for marker in backend.workspace_markers() {
                    match marker {
                        crate::backend::WorkspaceMarker::File(name) => {
                            if current.join(name).exists() {
                                matches = true;
                                break;
                            }
                        }
                        crate::backend::WorkspaceMarker::Directory(name) => {
                            if current.join(name).exists() {
                                matches = true;
                                break;
                            }
                        }
                    }
                }
                if matches {
                    break;
                }
            }
        }
        if matches {
            return current;
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }
    start_dir.to_path_buf()
}

pub fn check_db_compatibility(
    conn: &rusqlite::Connection,
    options: &BuildIndexOptions,
) -> Result<Option<RebuildReason>, CodeGraphError> {
    check_db_compatibility_with_registry(conn, options, global_registry())
}

pub fn check_db_compatibility_with_registry(
    conn: &rusqlite::Connection,
    options: &BuildIndexOptions,
    registry: &BackendRegistry,
) -> Result<Option<RebuildReason>, CodeGraphError> {
    // Check if metadata table exists
    let table_exists: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='metadata'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .unwrap_or(false);

    if !table_exists {
        return Ok(Some(RebuildReason::MissingDatabase));
    }

    let get_meta = |key: &str| -> Option<String> {
        conn.query_row("SELECT value FROM metadata WHERE key = ?", [key], |row| {
            row.get::<_, String>(0)
        })
        .ok()
    };

    // 1. Schema version
    let schema_version = get_meta("schema_version");
    if schema_version.as_deref() != Some("4") {
        return Ok(Some(RebuildReason::SchemaVersionChanged));
    }

    // 2. Indexer version
    let indexer_version = get_meta("indexer_version");
    if indexer_version.as_deref() != Some("0.1.0") {
        return Ok(Some(RebuildReason::IndexerVersionChanged));
    }

    // 3. Parser version & config
    let expected_parser_config_hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(format!("include_tests:{}", options.include_tests).as_bytes());
        format!("{:x}", hasher.finalize())
    };
    let parser_config_hash = get_meta("parser_config_hash");
    if parser_config_hash.as_deref() != Some(&expected_parser_config_hash) {
        return Ok(Some(RebuildReason::ParserConfigChanged));
    }

    // 4. Resolver id, version & config
    let expected_resolver_id = if options.use_lsp { "lsp" } else { "noop" };
    let resolver_id = get_meta("resolver_id");
    if resolver_id.as_deref() != Some(expected_resolver_id) {
        return Ok(Some(RebuildReason::ResolverConfigChanged));
    }

    let resolver_version = get_meta("resolver_version");
    if resolver_version.as_deref() != Some("0.1.0") {
        return Ok(Some(RebuildReason::ResolverVersionChanged));
    }

    let expected_resolver_config_hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(
            format!(
                "use_lsp:{:?},max_depth:{:?}",
                options.use_lsp, options.max_depth
            )
            .as_bytes(),
        );
        format!("{:x}", hasher.finalize())
    };
    let resolver_config_hash = get_meta("resolver_config_hash");
    if resolver_config_hash.as_deref() != Some(&expected_resolver_config_hash) {
        return Ok(Some(RebuildReason::ResolverConfigChanged));
    }

    // 5. Change detection strategy
    let expected_change_detection = match options.change_detection {
        FileChangeDetection::MtimeAndSize => "MtimeAndSize",
        FileChangeDetection::ContentHash => "ContentHash",
    };
    let change_detection = get_meta("change_detection_strategy");
    if change_detection.as_deref() != Some(expected_change_detection) {
        return Ok(Some(RebuildReason::ChangeDetectionStrategyChanged));
    }

    // 6. Base index status
    let base_index_ready = get_meta("base_index_ready");
    if base_index_ready.as_deref() != Some("true") {
        return Ok(Some(RebuildReason::PreviousRunIncomplete));
    }

    // Check backends metadata
    let backends_metadata_str: Option<String> = conn
        .query_row(
            "SELECT value FROM metadata WHERE key = 'backends_metadata'",
            [],
            |row| row.get(0),
        )
        .ok();

    if let Some(meta_str) = backends_metadata_str {
        if let Ok(stored_metas) =
            serde_json::from_str::<Vec<crate::backend::BackendMetadata>>(&meta_str)
        {
            for stored in stored_metas {
                if let Some(backend) = registry
                    .all()
                    .iter()
                    .find(|b| b.id().0 == stored.backend_id)
                {
                    let current = backend.metadata(options);
                    if current.parser_version != stored.parser_version {
                        return Ok(Some(RebuildReason::ParserVersionChanged));
                    }
                    if current.resolver_id != stored.resolver_id
                        || current.config_hash != stored.config_hash
                    {
                        return Ok(Some(RebuildReason::ResolverConfigChanged));
                    }
                    if current.resolver_version != stored.resolver_version {
                        return Ok(Some(RebuildReason::ResolverVersionChanged));
                    }
                } else {
                    return Ok(Some(RebuildReason::BackendSetChanged));
                }
            }
        } else {
            return Ok(Some(RebuildReason::CorruptDatabase));
        }
    } else {
        return Ok(Some(RebuildReason::BackendSetChanged));
    }

    Ok(None)
}

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
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name == "target"
                        || name == ".git"
                        || name == ".codegraph"
                        || name == ".ctx-codegraph"
                    {
                        return false;
                    }
                }
            }
            true
        });
    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            if crate::index::should_index_path_with_registry(path, registry) {
                disk_files.insert(path.to_path_buf());
            }
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

    for (path, _) in &db_files {
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

    if let Err(_) = conn.execute("PRAGMA foreign_keys = ON;", []) {
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

pub fn open_codegraph_db(root: &Path) -> Result<rusqlite::Connection, CodeGraphError> {
    let workspace_root = find_workspace_root(root);
    let db_dir = workspace_root.join(".ctx-codegraph");
    std::fs::create_dir_all(&db_dir)?;
    let db_path = db_dir.join("codegraph.sqlite");
    let conn = rusqlite::Connection::open(db_path)?;
    conn.execute("PRAGMA foreign_keys = ON;", [])?;
    Ok(conn)
}

pub fn open_db(root: &Path) -> Result<rusqlite::Connection, CodeGraphError> {
    open_codegraph_db(root)
}

pub fn init_schema(conn: &rusqlite::Connection) -> Result<(), CodeGraphError> {
    let meta_exists: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='metadata'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .unwrap_or(false);

    let mut needs_drop = false;
    if meta_exists {
        let schema_version: Option<String> = conn
            .query_row("SELECT value FROM metadata WHERE key = 'schema_version'", [], |row| {
                row.get::<_, String>(0)
            })
            .ok();
        if schema_version.as_deref() != Some("4") {
            needs_drop = true;
        }
    }

    if needs_drop {
        let tables = vec!["metadata", "files", "symbols", "call_sites", "call_edges", "occurrences", "edges"];
        for table in tables {
            let _ = conn.execute(&format!("DROP TABLE IF EXISTS {}", table), []);
        }
    }

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS files (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            rel_path TEXT NOT NULL,
            language TEXT NOT NULL,
            backend_id TEXT NOT NULL,
            mtime_ms INTEGER NOT NULL,
            size_bytes INTEGER NOT NULL,
            content_hash TEXT,
            parser_id TEXT NOT NULL,
            parser_version TEXT NOT NULL,
            parser_config_hash TEXT NOT NULL,
            indexed_at_ms INTEGER,
            parse_status TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS symbols (
            id INTEGER PRIMARY KEY,
            file_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            qualified_name TEXT NOT NULL,
            kind TEXT NOT NULL,
            language TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            start_col INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            end_col INTEGER NOT NULL,
            body_start_line INTEGER,
            body_start_col INTEGER,
            body_end_line INTEGER,
            body_end_col INTEGER,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS occurrences (
            id INTEGER PRIMARY KEY,
            file_id INTEGER NOT NULL,
            enclosing_symbol_id INTEGER,
            kind TEXT NOT NULL,
            raw_text TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            start_col INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            end_col INTEGER NOT NULL,
            language TEXT NOT NULL,
            backend_id TEXT NOT NULL,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
            FOREIGN KEY(enclosing_symbol_id) REFERENCES symbols(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS edges (
            id INTEGER PRIMARY KEY,
            kind TEXT NOT NULL,
            from_file_id INTEGER NOT NULL,
            from_symbol_id INTEGER,
            to_symbol_id INTEGER,
            to_external TEXT,
            occurrence_id INTEGER,
            raw_text TEXT,
            start_line INTEGER,
            start_col INTEGER,
            end_line INTEGER,
            end_col INTEGER,
            confidence TEXT NOT NULL,
            produced_by TEXT,
            FOREIGN KEY(from_file_id) REFERENCES files(id) ON DELETE CASCADE,
            FOREIGN KEY(from_symbol_id) REFERENCES symbols(id) ON DELETE CASCADE,
            FOREIGN KEY(to_symbol_id) REFERENCES symbols(id) ON DELETE SET NULL,
            FOREIGN KEY(occurrence_id) REFERENCES occurrences(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
        CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
        CREATE INDEX IF NOT EXISTS idx_symbols_qualified_name ON symbols(qualified_name);
        CREATE INDEX IF NOT EXISTS idx_symbols_file_id ON symbols(file_id);
        CREATE INDEX IF NOT EXISTS idx_occurrences_enclosing ON occurrences(enclosing_symbol_id);
        CREATE INDEX IF NOT EXISTS idx_occurrences_raw_text ON occurrences(raw_text);
        CREATE INDEX IF NOT EXISTS idx_edges_from_symbol ON edges(from_symbol_id);
        CREATE INDEX IF NOT EXISTS idx_edges_to_symbol ON edges(to_symbol_id);
        CREATE INDEX IF NOT EXISTS idx_edges_confidence ON edges(confidence);
    ",
    )?;

    conn.execute(
        "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', '4')",
        [],
    )?;
    // Store default backends metadata initially
    let registry = crate::backend::global_registry();
    let metas: Vec<_> = registry
        .all()
        .iter()
        .map(|b| b.metadata(&crate::index::BuildIndexOptions::default()))
        .collect();
    let metas_str = serde_json::to_string(&metas).unwrap_or_default();
    conn.execute(
        "INSERT OR REPLACE INTO metadata (key, value) VALUES ('backends_metadata', ?1)",
        [metas_str],
    )?;

    Ok(())
}

pub fn clear_index(conn: &mut rusqlite::Connection) -> Result<(), CodeGraphError> {
    clear_index_with_registry(conn, global_registry())
}

pub fn clear_index_with_registry(
    conn: &mut rusqlite::Connection,
    registry: &BackendRegistry,
) -> Result<(), CodeGraphError> {
    let tx = conn.transaction()?;
    for backend in registry.all() {
        let lang = backend.language().0.clone();
        let backend_id = backend.id();
        tx.execute(
            "DELETE FROM files WHERE language = ?1 AND backend_id = ?2",
            rusqlite::params![lang, backend_id.0],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn save_index(
    conn: &mut rusqlite::Connection,
    index: &mut CodeIndex,
) -> Result<(), CodeGraphError> {
    let tx = conn.transaction()?;

    let mut path_to_file_id = std::collections::HashMap::new();
    {
        let mut stmt = tx.prepare(
            "
            INSERT INTO files (
                path, rel_path, language, backend_id, mtime_ms, size_bytes,
                content_hash, parser_id, parser_version, parser_config_hash,
                indexed_at_ms, parse_status
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ",
        )?;
        for file in &mut index.files {
            let abs_path_str = file.abs_path.to_string_lossy().to_string();
            let rel_path_str = file.rel_path.to_string_lossy().to_string();
            let mtime_ms = file.mtime_ms;
            let size_bytes = file.size_bytes;
            let content_hash = file.content_hash.clone();
            let parse_status_str = file.parse_status.as_str();

            let row_id = stmt.insert(rusqlite::params![
                abs_path_str,
                rel_path_str,
                file.language,
                file.backend_id,
                mtime_ms,
                size_bytes,
                content_hash,
                file.parser_id,
                file.parser_version,
                file.parser_config_hash,
                file.indexed_at_ms.or_else(|| {
                    Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as i64,
                    )
                }),
                parse_status_str,
            ])?;
            let file_id = FileId(row_id);
            file.file_id = Some(file_id);
            path_to_file_id.insert(file.abs_path.clone(), file_id);
        }
    }

    let mut temp_sym_to_db_id = std::collections::HashMap::new();
    {
        let mut stmt = tx.prepare(
            "
            INSERT INTO symbols (
                file_id, name, qualified_name, kind, language,
                start_line, start_col, end_line, end_col,
                body_start_line, body_start_col, body_end_line, body_end_col
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        ",
        )?;
        for (i, sym) in index.symbols.iter_mut().enumerate() {
            let file_id = path_to_file_id
                .get(&sym.file)
                .copied()
                .or_else(|| {
                    index
                        .files
                        .iter()
                        .find(|f| f.rel_path == sym.file || f.abs_path == sym.file)
                        .and_then(|f| f.file_id)
                })
                .ok_or_else(|| {
                    CodeGraphError::Parse(format!(
                        "File not found for symbol: {}",
                        sym.file.display()
                    ))
                })?;
            sym.file_id = Some(file_id);

            let body_start_line = sym.body_range.as_ref().map(|r| r.start_line);
            let body_start_col = sym.body_range.as_ref().map(|r| r.start_col);
            let body_end_line = sym.body_range.as_ref().map(|r| r.end_line);
            let body_end_col = sym.body_range.as_ref().map(|r| r.end_col);

            let row_id = stmt.insert(rusqlite::params![
                file_id.0,
                sym.name,
                sym.qualified_name,
                sym.kind.as_str(),
                sym.language.0.as_str(),
                sym.range.start_line,
                sym.range.start_col,
                sym.range.end_line,
                sym.range.end_col,
                body_start_line,
                body_start_col,
                body_end_line,
                body_end_col,
            ])?;

            let db_id = SymbolId(row_id);
            let temp_id = SymbolId(i as i64);
            sym.id = Some(db_id);
            temp_sym_to_db_id.insert(temp_id, db_id);
        }
    }

    let mut temp_call_to_db_id = std::collections::HashMap::new();
    {
        let mut stmt = tx.prepare(
            "
            INSERT INTO occurrences (
                file_id, enclosing_symbol_id, kind, raw_text,
                start_line, start_col, end_line, end_col, language, backend_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ",
        )?;
        for (i, cs) in index.occurrences.iter_mut().enumerate() {
            let file_id = path_to_file_id
                .get(&cs.file)
                .copied()
                .or_else(|| {
                    index
                        .files
                        .iter()
                        .find(|f| f.rel_path == cs.file || f.abs_path == cs.file)
                        .and_then(|f| f.file_id)
                })
                .ok_or_else(|| {
                    CodeGraphError::Parse(format!(
                        "File not found for occurrence: {}",
                        cs.file.display()
                    ))
                })?;
            cs.file_id = Some(file_id);

            let from_db_id = match cs.enclosing_symbol {
                Some(temp_id) => {
                    let db_id = temp_sym_to_db_id.get(&temp_id).copied().ok_or_else(|| {
                        CodeGraphError::Parse("Enclosing symbol not saved to DB".to_string())
                    })?;
                    Some(db_id)
                }
                None => None,
            };

            let row_id = stmt.insert(rusqlite::params![
                file_id.0,
                from_db_id.map(|id| id.0),
                cs.kind.as_str(),
                cs.raw_text,
                cs.range.start_line,
                cs.range.start_col,
                cs.range.end_line,
                cs.range.end_col,
                cs.language.as_str(),
                cs.backend_id,
            ])?;

            let db_call_id = crate::model::OccurrenceId(row_id);
            let temp_call_id = crate::model::OccurrenceId(i as i64);
            cs.id = Some(db_call_id);
            cs.enclosing_symbol = from_db_id;
            temp_call_to_db_id.insert(temp_call_id, db_call_id);
        }
    }

    {
        let mut stmt = tx.prepare(
            "
            INSERT INTO edges (
                kind, from_file_id, from_symbol_id, to_symbol_id, to_external,
                occurrence_id, raw_text, start_line, start_col, end_line, end_col,
                confidence, produced_by
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        ",
        )?;
        for edge in &mut index.edges {
            let from_symbol_db_id = match edge.from_symbol_id {
                Some(temp_id) => {
                    let db_id = temp_sym_to_db_id.get(&temp_id).copied().ok_or_else(|| {
                        CodeGraphError::Parse("Edge source symbol not saved to DB".to_string())
                    })?;
                    Some(db_id)
                }
                None => None,
            };
            let to_symbol_db_id = match edge.to_symbol_id {
                Some(temp_to) => {
                    let db_id = temp_sym_to_db_id.get(&temp_to).copied().ok_or_else(|| {
                        CodeGraphError::Parse("Edge target symbol not saved to DB".to_string())
                    })?;
                    Some(db_id)
                }
                None => None,
            };
            let db_occurrence_id = match edge.occurrence_id {
                Some(temp_call_id) => {
                    let db_id = temp_call_to_db_id.get(&temp_call_id).copied().ok_or_else(|| {
                        CodeGraphError::Parse("Edge occurrence not saved to DB".to_string())
                    })?;
                    Some(db_id)
                }
                None => None,
            };

            let (from_file_db_id, raw_text, range) = match edge.occurrence_id {
                Some(temp_id) => {
                    let cs = &index.occurrences[temp_id.0 as usize];
                    (cs.file_id, Some(cs.raw_text.clone()), Some(cs.range.clone()))
                }
                None => {
                    let file_id = edge.from_file_id.or_else(|| {
                        None
                    });
                    (file_id, edge.raw_text.clone(), edge.range.clone())
                }
            };

            let from_file_db_id = from_file_db_id.ok_or_else(|| {
                CodeGraphError::Parse("Edge without valid file ID".to_string())
            })?;

            stmt.execute(rusqlite::params![
                edge.kind.as_str(),
                from_file_db_id.0,
                from_symbol_db_id.map(|id| id.0),
                to_symbol_db_id.map(|id| id.0),
                edge.to_external,
                db_occurrence_id.map(|id| id.0),
                raw_text,
                range.as_ref().map(|r| r.start_line),
                range.as_ref().map(|r| r.start_col),
                range.as_ref().map(|r| r.end_line),
                range.as_ref().map(|r| r.end_col),
                edge.confidence.as_str(),
                edge.produced_by,
            ])?;

            edge.from_file_id = Some(from_file_db_id);
            edge.from_symbol_id = from_symbol_db_id;
            edge.to_symbol_id = to_symbol_db_id;
            edge.occurrence_id = db_occurrence_id;
        }
    }

    tx.commit()?;
    Ok(())
}

pub fn load_index(conn: &rusqlite::Connection, root: &Path) -> Result<CodeIndex, CodeGraphError> {
    let mut files = Vec::new();
    let mut stmt = conn.prepare("SELECT id, path, rel_path, language, backend_id, mtime_ms, size_bytes, content_hash, parser_id, parser_version, parser_config_hash, indexed_at_ms, parse_status FROM files")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let path_str: String = row.get(1)?;
        let rel_path_str: String = row.get(2)?;
        let language: Language = row.get(3)?;
        let backend_id: String = row.get(4)?;
        let mtime_ms: i64 = row.get(5)?;
        let size_bytes: u64 = row.get(6)?;
        let content_hash: Option<String> = row.get(7)?;
        let parser_id: String = row.get(8)?;
        let parser_version: String = row.get(9)?;
        let parser_config_hash: String = row.get(10)?;
        let indexed_at_ms: Option<i64> = row.get(11)?;
        let parse_status_str: String = row.get(12)?;

        files.push(FileSnapshot {
            file_id: Some(FileId(id)),
            abs_path: PathBuf::from(path_str),
            rel_path: PathBuf::from(rel_path_str),
            language,
            backend_id,
            size_bytes,
            mtime_ms,
            mtime_ns: None,
            content_hash,
            parser_id,
            parser_version,
            parser_config_hash,
            indexed_at_ms,
            parse_status: FileParseStatus::from_str(&parse_status_str)
                .unwrap_or(FileParseStatus::Success),
        });
    }

    let file_map: std::collections::HashMap<FileId, PathBuf> = files
        .iter()
        .filter_map(|f| f.file_id.map(|id| (id, f.abs_path.clone())))
        .collect();

    let mut symbols = Vec::new();
    let mut stmt = conn.prepare(
        "
        SELECT id, file_id, name, qualified_name, kind, language,
               start_line, start_col, end_line, end_col,
               body_start_line, body_start_col, body_end_line, body_end_col
        FROM symbols
    ",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let file_id: i64 = row.get(1)?;
        let name: String = row.get(2)?;
        let qualified_name: String = row.get(3)?;
        let kind_str: String = row.get(4)?;
        let lang_str: String = row.get(5)?;

        let start_line: usize = row.get(6)?;
        let start_col: usize = row.get(7)?;
        let end_line: usize = row.get(8)?;
        let end_col: usize = row.get(9)?;

        let body_start_line: Option<usize> = row.get(10)?;
        let body_start_col: Option<usize> = row.get(11)?;
        let body_end_line: Option<usize> = row.get(12)?;
        let body_end_col: Option<usize> = row.get(13)?;

        let body_range = if let (Some(sl), Some(sc), Some(el), Some(ec)) =
            (body_start_line, body_start_col, body_end_line, body_end_col)
        {
            Some(TextRange {
                start_line: sl,
                start_col: sc,
                end_line: el,
                end_col: ec,
            })
        } else {
            None
        };

        let file_path = file_map.get(&FileId(file_id)).cloned().unwrap_or_default();

        symbols.push(Symbol {
            id: Some(SymbolId(id)),
            file_id: Some(FileId(file_id)),
            name,
            qualified_name,
            kind: SymbolKind::from_str(&kind_str).unwrap_or(SymbolKind::Function),
            language: Language(lang_str),
            file: file_path,
            range: TextRange {
                start_line,
                start_col,
                end_line,
                end_col,
            },
            body_range,
        });
    }

    let mut occurrences = Vec::new();
    let mut stmt = conn.prepare(
        "
        SELECT id, file_id, enclosing_symbol_id, kind, raw_text,
               start_line, start_col, end_line, end_col, language, backend_id
        FROM occurrences
    ",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let file_id: i64 = row.get(1)?;
        let enclosing_symbol_id: Option<i64> = row.get(2)?;
        let kind_str: String = row.get(3)?;
        let raw_text: String = row.get(4)?;
        let start_line: usize = row.get(5)?;
        let start_col: usize = row.get(6)?;
        let end_line: usize = row.get(7)?;
        let end_col: usize = row.get(8)?;
        let language_str: String = row.get(9)?;
        let backend_id: String = row.get(10)?;

        let file_path = file_map.get(&FileId(file_id)).cloned().unwrap_or_default();

        occurrences.push(Occurrence {
            id: Some(OccurrenceId(id)),
            file_id: Some(FileId(file_id)),
            enclosing_symbol: enclosing_symbol_id.map(SymbolId),
            enclosing_temp_index: None,
            kind: OccurrenceKind::from_str(&kind_str).unwrap_or(OccurrenceKind::Unknown),
            raw_text,
            file: file_path,
            range: TextRange {
                start_line,
                start_col,
                end_line,
                end_col,
            },
            language: LanguageId(language_str),
            backend_id,
        });
    }

    let mut edges = Vec::new();
    let mut stmt = conn.prepare(
        "
        SELECT id, kind, from_file_id, from_symbol_id, to_symbol_id, to_external,
               occurrence_id, raw_text, start_line, start_col, end_line, end_col,
               confidence, produced_by
        FROM edges
    ",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let kind_str: String = row.get(1)?;
        let from_file_id: i64 = row.get(2)?;
        let from_symbol_id: Option<i64> = row.get(3)?;
        let to_symbol_id: Option<i64> = row.get(4)?;
        let to_external: Option<String> = row.get(5)?;
        let occurrence_id: Option<i64> = row.get(6)?;
        let raw_text: Option<String> = row.get(7)?;
        let start_line: Option<usize> = row.get(8)?;
        let start_col: Option<usize> = row.get(9)?;
        let end_line: Option<usize> = row.get(10)?;
        let end_col: Option<usize> = row.get(11)?;
        let confidence_str: String = row.get(12)?;
        let produced_by: Option<String> = row.get(13)?;

        let range = if let (Some(sl), Some(sc), Some(el), Some(ec)) =
            (start_line, start_col, end_line, end_col)
        {
            Some(TextRange {
                start_line: sl,
                start_col: sc,
                end_line: el,
                end_col: ec,
            })
        } else {
            None
        };

        edges.push(GraphEdge {
            id: Some(EdgeId(id)),
            kind: EdgeKind::from_str(&kind_str).unwrap_or(EdgeKind::Unknown),
            from_file_id: Some(FileId(from_file_id)),
            from_symbol_id: from_symbol_id.map(SymbolId),
            to_symbol_id: to_symbol_id.map(SymbolId),
            to_external,
            occurrence_id: occurrence_id.map(OccurrenceId),
            raw_text,
            range,
            confidence: ResolutionConfidence::from_str(&confidence_str)
                .unwrap_or(ResolutionConfidence::Unresolved),
            produced_by,
        });
    }

    let call_sites_compat = occurrences
        .iter()
        .filter(|o| o.kind == OccurrenceKind::Call)
        .cloned()
        .collect();

    Ok(CodeIndex {
        root: root.to_path_buf(),
        files,
        symbols,
        occurrences,
        edges,
        call_sites: call_sites_compat,
    })
}

pub fn rebuild_index_db(
    root: &Path,
    options: BuildIndexOptions,
) -> Result<(CodeIndex, crate::model::BuildReport), CodeGraphError> {
    rebuild_index_db_with_registry(root, options, crate::backend::global_registry())
}

pub fn rebuild_index_db_with_registry(
    root: &Path,
    options: BuildIndexOptions,
    registry: &BackendRegistry,
) -> Result<(CodeIndex, crate::model::BuildReport), CodeGraphError> {
    let workspace_root = find_workspace_root(root);
    let state = get_index_state_with_registry(&workspace_root, &options, registry)?;

    let mut conn = open_db(&workspace_root)?;
    init_schema(&conn)?;

    match state {
        IndexState::NeedsFullRebuild(reason) => {
            let (index, report) = run_full_rebuild_with_registry(
                &mut conn,
                &workspace_root,
                options,
                Some(reason),
                registry,
            )?;
            Ok((index, report))
        }
        IndexState::Missing => {
            let (index, report) = run_full_rebuild_with_registry(
                &mut conn,
                &workspace_root,
                options,
                Some(RebuildReason::MissingDatabase),
                registry,
            )?;
            Ok((index, report))
        }
        IndexState::Ready => {
            let index = load_index(&conn, &workspace_root)?;
            let report = crate::model::BuildReport {
                full_rebuild: false,
                full_rebuild_reason: None,
                added_files: 0,
                modified_files: 0,
                deleted_files: 0,
                unchanged_files: index.files.len(),
                parsed_files: 0,
                reused_files: index.files.len(),
                symbols_written: 0,
                call_sites_written: 0,
                edges_written: index.edges.len(),
                lsp_edges_exact: index
                    .edges
                    .iter()
                    .filter(|e| e.confidence == ResolutionConfidence::LspExact)
                    .count(),
                syntax_edges: index
                    .edges
                    .iter()
                    .filter(|e| e.confidence == ResolutionConfidence::Syntax)
                    .count(),
                heuristic_edges: index
                    .edges
                    .iter()
                    .filter(|e| e.confidence == ResolutionConfidence::Heuristic)
                    .count(),
                unresolved_edges: index
                    .edges
                    .iter()
                    .filter(|e| e.confidence == ResolutionConfidence::Unresolved)
                    .count(),
            };
            Ok((index, report))
        }
        IndexState::NeedsIncrementalUpdate(diff) => {
            let (index, report) = run_incremental_update_with_registry(
                &mut conn,
                &workspace_root,
                options,
                diff,
                registry,
            )?;
            Ok((index, report))
        }
    }
}

#[derive(Debug, Clone)]
pub struct StagedFileUpdate {
    pub snapshot: FileSnapshot,
    pub parse_result: Result<ParsedFile, String>,
    pub previous_file_id: Option<FileId>,
}

pub fn compute_affected_set(
    conn: &rusqlite::Connection,
    diff: &IndexDiff,
    staged: &[StagedFileUpdate],
) -> Result<AffectedSet, CodeGraphError> {
    compute_affected_set_with_registry(conn, diff, staged, global_registry())
}

pub fn compute_affected_set_with_registry(
    conn: &rusqlite::Connection,
    diff: &IndexDiff,
    staged: &[StagedFileUpdate],
    registry: &BackendRegistry,
) -> Result<AffectedSet, CodeGraphError> {
    let mut files = std::collections::HashSet::new();
    let mut symbols = std::collections::HashSet::new();
    let mut occurrences = std::collections::HashSet::new();
    let mut edge_kinds = std::collections::HashSet::new();
    let mut resolvers = std::collections::HashSet::new();

    // Default edge kind and resolver
    edge_kinds.insert(EdgeKind::Call);
    resolvers.insert("noop".to_string());
    for backend in registry.all() {
        if let Some(res) = backend.resolver() {
            resolvers.insert(res.resolver_id().0);
        }
    }

    let mut get_file_id_stmt = conn.prepare("SELECT id FROM files WHERE path = ?1")?;
    for path in &diff.deleted {
        if let Ok(id) = get_file_id_stmt.query_row([path.to_string_lossy().to_string()], |row| {
            row.get::<_, i64>(0)
        }) {
            files.insert(FileId(id));
        }
    }
    for update in staged {
        if let Some(prev_id) = update.previous_file_id {
            files.insert(prev_id);
        }
    }
    drop(get_file_id_stmt);

    for &file_id in &files {
        let mut stmt = conn.prepare("SELECT id FROM symbols WHERE file_id = ?1")?;
        let rows = stmt.query_map([file_id.0], |row| row.get::<_, i64>(0))?;
        for row in rows {
            if let Ok(id) = row {
                symbols.insert(SymbolId(id));
            }
        }
    }

    for &sym_id in &symbols {
        let mut stmt =
            conn.prepare("SELECT occurrence_id FROM edges WHERE to_symbol_id = ?1")?;
        let rows = stmt.query_map([sym_id.0], |row| row.get::<_, i64>(0))?;
        for row in rows {
            if let Ok(cs_id) = row {
                occurrences.insert(OccurrenceId(cs_id));
            }
        }
    }

    Ok(AffectedSet {
        files,
        symbols,
        occurrences,
        edge_kinds,
        resolvers,
    })
}

fn load_all_symbols(conn: &rusqlite::Connection) -> Result<Vec<Symbol>, CodeGraphError> {
    let mut stmt = conn.prepare(
        "
        SELECT s.id, s.file_id, s.name, s.qualified_name, s.kind, s.language,
               s.start_line, s.start_col, s.end_line, s.end_col, f.path
        FROM symbols s
        JOIN files f ON s.file_id = f.id
    ",
    )?;
    let rows = stmt.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let file_id: i64 = row.get(1)?;
        let name: String = row.get(2)?;
        let qualified_name: String = row.get(3)?;
        let kind_str: String = row.get(4)?;
        let lang_str: String = row.get(5)?;
        let start_line: usize = row.get(6)?;
        let start_col: usize = row.get(7)?;
        let end_line: usize = row.get(8)?;
        let end_col: usize = row.get(9)?;
        let file_path: String = row.get(10)?;

        Ok(Symbol {
            id: Some(SymbolId(id)),
            file_id: Some(FileId(file_id)),
            name,
            qualified_name,
            kind: SymbolKind::from_str(&kind_str).unwrap_or(SymbolKind::Function),
            language: Language(lang_str),
            file: PathBuf::from(file_path),
            range: TextRange {
                start_line,
                start_col,
                end_line,
                end_col,
            },
            body_range: None,
        })
    })?;
    let mut symbols = Vec::new();
    for r in rows {
        symbols.push(r?);
    }
    Ok(symbols)
}

fn load_occurrences_to_resolve(
    conn: &rusqlite::Connection,
    new_file_ids: &[i64],
    affected_occurrences: &std::collections::HashSet<OccurrenceId>,
) -> Result<Vec<Occurrence>, CodeGraphError> {
    let mut occurrences = Vec::new();
    if new_file_ids.is_empty() && affected_occurrences.is_empty() {
        return Ok(occurrences);
    }

    let mut sql = "
        SELECT cs.id, cs.file_id, cs.enclosing_symbol_id, cs.kind, cs.raw_text,
               cs.start_line, cs.start_col, cs.end_line, cs.end_col, f.path, cs.language, cs.backend_id
        FROM occurrences cs
        JOIN files f ON cs.file_id = f.id
        WHERE 1=0
    "
    .to_string();

    if !new_file_ids.is_empty() {
        let placeholders: Vec<String> = new_file_ids.iter().map(|id| id.to_string()).collect();
        sql.push_str(&format!(" OR cs.file_id IN ({})", placeholders.join(",")));
    }

    if !affected_occurrences.is_empty() {
        let placeholders: Vec<String> = affected_occurrences
            .iter()
            .map(|id| id.0.to_string())
            .collect();
        sql.push_str(&format!(" OR cs.id IN ({})", placeholders.join(",")));
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let file_id: i64 = row.get(1)?;
        let enclosing_symbol_id: Option<i64> = row.get(2)?;
        let kind_str: String = row.get(3)?;
        let raw_text: String = row.get(4)?;
        let start_line: usize = row.get(5)?;
        let start_col: usize = row.get(6)?;
        let end_line: usize = row.get(7)?;
        let end_col: usize = row.get(8)?;
        let file_path: String = row.get(9)?;
        let language_str: String = row.get(10)?;
        let backend_id: String = row.get(11)?;

        Ok(Occurrence {
            id: Some(OccurrenceId(id)),
            file_id: Some(FileId(file_id)),
            enclosing_symbol: enclosing_symbol_id.map(SymbolId),
            enclosing_temp_index: None,
            kind: OccurrenceKind::from_str(&kind_str).unwrap_or(OccurrenceKind::Unknown),
            raw_text,
            file: PathBuf::from(file_path),
            range: TextRange {
                start_line,
                start_col,
                end_line,
                end_col,
            },
            language: LanguageId(language_str),
            backend_id,
        })
    })?;

    for r in rows {
        occurrences.push(r?);
    }
    Ok(occurrences)
}

fn rebuild_affected_edges_in_tx(
    tx: &rusqlite::Transaction,
    workspace_root: &Path,
    options: &BuildIndexOptions,
    affected_files: &[FileId],
    affected_occurrences: &std::collections::HashSet<OccurrenceId>,
) -> Result<(), CodeGraphError> {
    rebuild_affected_edges_in_tx_with_registry(
        tx,
        workspace_root,
        options,
        affected_files,
        affected_occurrences,
        crate::backend::global_registry(),
    )
}

fn rebuild_affected_edges_in_tx_with_registry(
    tx: &rusqlite::Transaction,
    workspace_root: &Path,
    options: &BuildIndexOptions,
    affected_files: &[FileId],
    affected_occurrences: &std::collections::HashSet<OccurrenceId>,
    registry: &BackendRegistry,
) -> Result<(), CodeGraphError> {
    let mut file_ids = Vec::new();
    for fid in affected_files {
        file_ids.push(fid.0);
    }

    let occurrences = load_occurrences_to_resolve(tx, &file_ids, affected_occurrences)?;
    if occurrences.is_empty() {
        return Ok(());
    }

    let cs_ids: Vec<String> = occurrences
        .iter()
        .map(|cs| cs.id.unwrap().0.to_string())
        .collect();
    let sql = format!(
        "DELETE FROM edges WHERE occurrence_id IN ({})",
        cs_ids.join(",")
    );
    tx.execute(&sql, [])?;

    let all_symbols = load_all_symbols(tx)?;

    let mut edge_stmt = tx.prepare(
        "INSERT INTO edges (kind, from_file_id, from_symbol_id, to_symbol_id, occurrence_id, raw_text, start_line, start_col, end_line, end_col, confidence, produced_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?;

    for cs in &occurrences {
        if cs.kind != OccurrenceKind::Call {
            continue;
        }

        let from_id = match cs.enclosing_symbol {
            Some(id) => id,
            None => {
                continue;
            }
        };

        let mut resolved_idx = None;
        let mut confidence = ResolutionConfidence::Unresolved;

        let backend = registry.find_by_path(&cs.file);
        let resolver = backend.and_then(|b| b.resolver());

        if options.use_lsp {
            if let Some(res) = resolver {
                let resolve_input = crate::backend::ResolveInput {
                    workspace_root,
                    occurrence: cs,
                    symbols: &all_symbols,
                };
                match res.resolve(resolve_input) {
                    Ok(out) => {
                        resolved_idx = out.resolved_symbol_index;
                        confidence = out.confidence;
                    }
                    Err(err) => {
                        eprintln!(
                            "LSP resolution warning for call to {}: {}",
                            cs.raw_text, err
                        );
                    }
                }
            }
        }

        if resolved_idx.is_none() {
            let (fallback_idx, fallback_conf) =
                crate::resolver::noop::resolve_name_only_occurrence(cs, &all_symbols);
            resolved_idx = fallback_idx;
            confidence = fallback_conf;
        }

        let to_db_id = resolved_idx.and_then(|idx| all_symbols[idx].id);
        let cs_id = cs.id.unwrap();
        let file_id = cs.file_id.unwrap();

        edge_stmt.execute(rusqlite::params![
            EdgeKind::Call.as_str(),
            file_id.0,
            from_id.0,
            to_db_id.map(|id| id.0),
            cs_id.0,
            cs.raw_text,
            cs.range.start_line,
            cs.range.start_col,
            cs.range.end_line,
            cs.range.end_col,
            confidence.as_str(),
            resolver.map(|r| r.resolver_id().0.clone()).unwrap_or_else(|| "noop".to_string()),
        ])?;
    }

    Ok(())
}

pub fn validate_index_invariants(conn: &rusqlite::Connection) -> Result<(), CodeGraphError> {
    let invalid_symbols: i64 = conn.query_row(
        "SELECT count(*) FROM symbols WHERE file_id NOT IN (SELECT id FROM files)",
        [],
        |row| row.get(0),
    )?;
    if invalid_symbols > 0 {
        return Err(CodeGraphError::Parse(format!(
            "Invariant violation: {} symbols with invalid file_id",
            invalid_symbols
        )));
    }

    let invalid_occurrences: i64 = conn.query_row(
        "SELECT count(*) FROM occurrences WHERE file_id NOT IN (SELECT id FROM files)",
        [],
        |row| row.get(0),
    )?;
    if invalid_occurrences > 0 {
        return Err(CodeGraphError::Parse(format!(
            "Invariant violation: {} occurrences with invalid file_id",
            invalid_occurrences
        )));
    }

    let invalid_edges_source: i64 = conn.query_row(
        "SELECT count(*) FROM edges WHERE from_symbol_id IS NOT NULL AND from_symbol_id NOT IN (SELECT id FROM symbols)",
        [],
        |row| row.get(0),
    )?;
    if invalid_edges_source > 0 {
        return Err(CodeGraphError::Parse(format!(
            "Invariant violation: {} edges with invalid source symbol id",
            invalid_edges_source
        )));
    }

    let invalid_edges_target: i64 = conn.query_row(
        "SELECT count(*) FROM edges WHERE to_symbol_id IS NOT NULL AND to_symbol_id NOT IN (SELECT id FROM symbols)",
        [],
        |row| row.get(0),
    )?;
    if invalid_edges_target > 0 {
        return Err(CodeGraphError::Parse(format!(
            "Invariant violation: {} edges pointing to non-existent symbol",
            invalid_edges_target
        )));
    }

    Ok(())
}

pub fn run_full_rebuild(
    conn: &mut rusqlite::Connection,
    workspace_root: &Path,
    options: BuildIndexOptions,
    reason: Option<RebuildReason>,
) -> Result<(CodeIndex, crate::model::BuildReport), CodeGraphError> {
    run_full_rebuild_with_registry(
        conn,
        workspace_root,
        options,
        reason,
        crate::backend::global_registry(),
    )
}

fn run_full_rebuild_with_registry(
    conn: &mut rusqlite::Connection,
    workspace_root: &Path,
    options: BuildIndexOptions,
    reason: Option<RebuildReason>,
    registry: &BackendRegistry,
) -> Result<(CodeIndex, crate::model::BuildReport), CodeGraphError> {
    clear_index_with_registry(conn, registry)?;

    let mut base_options = options.clone();
    base_options.use_lsp = false;
    let mut index =
        crate::index::build_index_with_registry(workspace_root, base_options, registry)?;

    save_index(conn, &mut index)?;

    let tx = conn.transaction()?;

    let write_meta =
        |tx: &rusqlite::Transaction, key: &str, value: &str| -> Result<(), CodeGraphError> {
            tx.execute(
                "INSERT OR REPLACE INTO metadata (key, value) VALUES (?, ?)",
                [key, value],
            )?;
            Ok(())
        };
    write_meta(&tx, "schema_version", "4")?;
    write_meta(&tx, "indexer_version", "0.1.0")?;

    let metas: Vec<_> = registry
        .all()
        .iter()
        .map(|b| b.metadata(&options))
        .collect();
    let metas_str = serde_json::to_string(&metas).unwrap_or_default();
    tx.execute(
        "INSERT OR REPLACE INTO metadata (key, value) VALUES ('backends_metadata', ?1)",
        [metas_str],
    )?;

    let parser_config_hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(format!("include_tests:{}", options.include_tests).as_bytes());
        format!("{:x}", hasher.finalize())
    };
    write_meta(&tx, "parser_config_hash", &parser_config_hash)?;

    let resolver_id = if options.use_lsp { "lsp" } else { "noop" };
    write_meta(&tx, "resolver_id", resolver_id)?;
    write_meta(&tx, "resolver_version", "0.1.0")?;
    let resolver_config_hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(
            format!(
                "use_lsp:{:?},max_depth:{:?}",
                options.use_lsp, options.max_depth
            )
            .as_bytes(),
        );
        format!("{:x}", hasher.finalize())
    };
    write_meta(&tx, "resolver_config_hash", &resolver_config_hash)?;

    let change_detection = match options.change_detection {
        FileChangeDetection::MtimeAndSize => "MtimeAndSize",
        FileChangeDetection::ContentHash => "ContentHash",
    };
    write_meta(&tx, "change_detection_strategy", change_detection)?;
    write_meta(&tx, "base_index_ready", "true")?;

    let mut affected_files = std::collections::HashSet::new();
    {
        let mut stmt = tx.prepare("SELECT id FROM files")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            affected_files.insert(FileId(row.get(0)?));
        }
    }

    let affected = AffectedSet {
        files: affected_files,
        symbols: std::collections::HashSet::new(),
        occurrences: std::collections::HashSet::new(),
        edge_kinds: {
            let mut s = std::collections::HashSet::new();
            s.insert(EdgeKind::Call);
            s
        },
        resolvers: {
            let mut s = std::collections::HashSet::new();
            s.insert(resolver_id.to_string());
            s
        },
    };

    let affected_files_vec: Vec<FileId> = affected.files.iter().copied().collect();
    rebuild_affected_edges_in_tx_with_registry(
        &tx,
        workspace_root,
        &options,
        &affected_files_vec,
        &affected.occurrences,
        registry,
    )?;

    if options.use_lsp {
        write_meta(&tx, "lsp_enrichment", "complete")?;
    } else {
        write_meta(&tx, "lsp_enrichment", "none")?;
    }

    tx.commit()?;

    validate_index_invariants(conn)?;

    let loaded = load_index(conn, workspace_root)?;

    let lsp_count = loaded
        .edges
        .iter()
        .filter(|e| e.confidence == ResolutionConfidence::LspExact)
        .count();
    let syntax_count = loaded
        .edges
        .iter()
        .filter(|e| e.confidence == ResolutionConfidence::Syntax)
        .count();
    let heuristic_count = loaded
        .edges
        .iter()
        .filter(|e| e.confidence == ResolutionConfidence::Heuristic)
        .count();
    let unresolved_count = loaded
        .edges
        .iter()
        .filter(|e| e.confidence == ResolutionConfidence::Unresolved)
        .count();

    let report = crate::model::BuildReport {
        full_rebuild: true,
        full_rebuild_reason: reason,
        added_files: loaded.files.len(),
        modified_files: 0,
        deleted_files: 0,
        unchanged_files: 0,
        parsed_files: loaded.files.len(),
        reused_files: 0,
        symbols_written: loaded.symbols.len(),
        call_sites_written: loaded.call_sites.len(),
        edges_written: loaded.edges.len(),
        lsp_edges_exact: lsp_count,
        syntax_edges: syntax_count,
        heuristic_edges: heuristic_count,
        unresolved_edges: unresolved_count,
    };

    Ok((loaded, report))
}

pub fn run_incremental_update(
    conn: &mut rusqlite::Connection,
    workspace_root: &Path,
    options: BuildIndexOptions,
    diff: IndexDiff,
) -> Result<(CodeIndex, crate::model::BuildReport), CodeGraphError> {
    run_incremental_update_with_registry(
        conn,
        workspace_root,
        options,
        diff,
        crate::backend::global_registry(),
    )
}

pub fn run_incremental_update_with_registry(
    conn: &mut rusqlite::Connection,
    workspace_root: &Path,
    options: BuildIndexOptions,
    diff: IndexDiff,
    registry: &BackendRegistry,
) -> Result<(CodeIndex, crate::model::BuildReport), CodeGraphError> {
    let mut staged_updates = Vec::new();
    let mut get_file_id_stmt = conn.prepare("SELECT id FROM files WHERE path = ?1")?;

    for snapshot in &diff.added {
        let path = &snapshot.abs_path;
        let backend = registry.find_by_path(path).ok_or_else(|| {
            CodeGraphError::Parse(format!("No backend found for path: {}", path.display()))
        })?;
        let parse_res = backend
            .parser()
            .parse_file(crate::backend::ParseInput { path })
            .map(|parsed| ParsedFile {
                symbols: parsed.symbols,
                occurrences: parsed.occurrences,
            })
            .map_err(|e| e.to_string());

        staged_updates.push(StagedFileUpdate {
            snapshot: snapshot.clone(),
            parse_result: parse_res,
            previous_file_id: None,
        });
    }

    for snapshot in &diff.modified {
        let path = &snapshot.abs_path;
        let prev_id: Option<i64> = get_file_id_stmt
            .query_row([path.to_string_lossy().to_string()], |row| row.get(0))
            .ok();

        let backend = registry.find_by_path(path).ok_or_else(|| {
            CodeGraphError::Parse(format!("No backend found for path: {}", path.display()))
        })?;
        let parse_res = backend
            .parser()
            .parse_file(crate::backend::ParseInput { path })
            .map(|parsed| ParsedFile {
                symbols: parsed.symbols,
                occurrences: parsed.occurrences,
            })
            .map_err(|e| e.to_string());

        staged_updates.push(StagedFileUpdate {
            snapshot: snapshot.clone(),
            parse_result: parse_res,
            previous_file_id: prev_id.map(FileId),
        });
    }
    drop(get_file_id_stmt);

    let affected = compute_affected_set_with_registry(conn, &diff, &staged_updates, registry)?;

    let tx = conn.transaction()?;

    let mut symbols_written = 0;
    let mut call_sites_written = 0;
    let mut parsed_files_count = 0;

    let mut delete_file_stmt = tx.prepare("DELETE FROM files WHERE path = ?1")?;
    for path in &diff.deleted {
        delete_file_stmt.execute(rusqlite::params![path.to_string_lossy().to_string()])?;
    }
    drop(delete_file_stmt);

    let mut file_insert_stmt = tx.prepare(
        "INSERT INTO files (
            path, rel_path, language, backend_id, mtime_ms, size_bytes,
            content_hash, parser_id, parser_version, parser_config_hash,
            indexed_at_ms, parse_status
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?;

    let mut file_update_meta_stmt = tx.prepare(
        "UPDATE files SET 
            mtime_ms = ?1, size_bytes = ?2, content_hash = ?3,
            indexed_at_ms = ?4, parse_status = ?5
         WHERE id = ?6",
    )?;

    let mut sym_stmt = tx.prepare(
        "INSERT INTO symbols (
            file_id, name, qualified_name, kind, language,
            start_line, start_col, end_line, end_col,
            body_start_line, body_start_col, body_end_line, body_end_col
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?;

    let mut cs_stmt = tx.prepare(
        "INSERT INTO occurrences (
            file_id, enclosing_symbol_id, kind, raw_text,
            start_line, start_col, end_line, end_col, language, backend_id
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;

    let mut delete_file_contents_stmt = tx.prepare("DELETE FROM symbols WHERE file_id = ?1")?;

    let mut successfully_parsed_file_ids = Vec::new();

    for update in &staged_updates {
        let path_str = update.snapshot.abs_path.to_string_lossy().to_string();
        let rel_path_str = update.snapshot.rel_path.to_string_lossy().to_string();
        let current_time = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
        );

        match &update.parse_result {
            Ok(parsed) => {
                if let Some(prev_id) = update.previous_file_id {
                    file_update_meta_stmt.execute(rusqlite::params![
                        update.snapshot.mtime_ms,
                        update.snapshot.size_bytes,
                        update.snapshot.content_hash,
                        current_time,
                        FileParseStatus::Success.as_str(),
                        prev_id.0,
                    ])?;
                    delete_file_contents_stmt.execute(rusqlite::params![prev_id.0])?;
                }

                let file_id = if let Some(prev_id) = update.previous_file_id {
                    prev_id.0
                } else {
                    file_insert_stmt.execute(rusqlite::params![
                        path_str,
                        rel_path_str,
                        update.snapshot.language,
                        update.snapshot.backend_id,
                        update.snapshot.mtime_ms,
                        update.snapshot.size_bytes,
                        update.snapshot.content_hash,
                        update.snapshot.parser_id,
                        update.snapshot.parser_version,
                        update.snapshot.parser_config_hash,
                        current_time,
                        FileParseStatus::Success.as_str(),
                    ])?;
                    tx.last_insert_rowid()
                };
                successfully_parsed_file_ids.push(FileId(file_id));
                parsed_files_count += 1;

                let mut file_symbols = parsed.symbols.clone();
                let mut file_occurrences = parsed.occurrences.clone();

                if !options.include_tests {
                    let mut new_symbols = Vec::new();
                    let mut index_map = std::collections::HashMap::new();
                    for (i, sym) in file_symbols.into_iter().enumerate() {
                        if sym.kind != SymbolKind::Test {
                            index_map.insert(i, new_symbols.len());
                            new_symbols.push(sym);
                        }
                    }
                    file_symbols = new_symbols;

                    file_occurrences.retain(|cs| {
                        if let Some(old_idx) = cs.enclosing_temp_index {
                            index_map.contains_key(&old_idx)
                        } else {
                            true
                        }
                    });

                    for cs in &mut file_occurrences {
                        if let Some(ref mut idx) = cs.enclosing_temp_index {
                            if let Some(&new_idx) = index_map.get(idx) {
                                *idx = new_idx;
                            }
                        }
                    }
                }

                let mut sym_ids = Vec::new();
                for sym in &file_symbols {
                    let body_start_line = sym.body_range.as_ref().map(|r| r.start_line);
                    let body_start_col = sym.body_range.as_ref().map(|r| r.start_col);
                    let body_end_line = sym.body_range.as_ref().map(|r| r.end_line);
                    let body_end_col = sym.body_range.as_ref().map(|r| r.end_col);

                    sym_stmt.execute(rusqlite::params![
                        file_id,
                        sym.name,
                        sym.qualified_name,
                        sym.kind.as_str(),
                        sym.language.0.clone(),
                        sym.range.start_line,
                        sym.range.start_col,
                        sym.range.end_line,
                        sym.range.end_col,
                        body_start_line,
                        body_start_col,
                        body_end_line,
                        body_end_col,
                    ])?;
                    let sym_db_id = tx.last_insert_rowid();
                    sym_ids.push(sym_db_id);
                    symbols_written += 1;
                }

                for cs in &file_occurrences {
                    let from_db_id = match cs.enclosing_temp_index {
                        Some(idx) => Some(sym_ids[idx]),
                        None => None,
                    };
                    cs_stmt.execute(rusqlite::params![
                        file_id,
                        from_db_id,
                        cs.kind.as_str(),
                        cs.raw_text,
                        cs.range.start_line,
                        cs.range.start_col,
                        cs.range.end_line,
                        cs.range.end_col,
                        cs.language.as_str(),
                        cs.backend_id,
                    ])?;
                    call_sites_written += 1;
                }
            }
            Err(_) => {
                if let Some(prev_id) = update.previous_file_id {
                    file_update_meta_stmt.execute(rusqlite::params![
                        update.snapshot.mtime_ms,
                        update.snapshot.size_bytes,
                        update.snapshot.content_hash,
                        current_time,
                        FileParseStatus::Failed.as_str(),
                        prev_id.0,
                    ])?;
                } else {
                    file_insert_stmt.execute(rusqlite::params![
                        path_str,
                        rel_path_str,
                        update.snapshot.language,
                        update.snapshot.backend_id,
                        update.snapshot.mtime_ms,
                        update.snapshot.size_bytes,
                        update.snapshot.content_hash,
                        update.snapshot.parser_id,
                        update.snapshot.parser_version,
                        update.snapshot.parser_config_hash,
                        current_time,
                        FileParseStatus::Failed.as_str(),
                    ])?;
                }
            }
        }
    }

    drop(file_insert_stmt);
    drop(file_update_meta_stmt);
    drop(sym_stmt);
    drop(cs_stmt);
    drop(delete_file_contents_stmt);

    rebuild_affected_edges_in_tx_with_registry(
        &tx,
        workspace_root,
        &options,
        &successfully_parsed_file_ids,
        &affected.occurrences,
        registry,
    )?;

    let write_meta =
        |tx: &rusqlite::Transaction, key: &str, value: &str| -> Result<(), CodeGraphError> {
            tx.execute(
                "INSERT OR REPLACE INTO metadata (key, value) VALUES (?, ?)",
                [key, value],
            )?;
            Ok(())
        };
    write_meta(&tx, "schema_version", "4")?;
    write_meta(&tx, "indexer_version", "0.1.0")?;

    let metas: Vec<_> = registry
        .all()
        .iter()
        .map(|b| b.metadata(&options))
        .collect();
    let metas_str = serde_json::to_string(&metas).unwrap_or_default();
    tx.execute(
        "INSERT OR REPLACE INTO metadata (key, value) VALUES ('backends_metadata', ?1)",
        [metas_str],
    )?;

    let parser_config_hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(format!("include_tests:{}", options.include_tests).as_bytes());
        format!("{:x}", hasher.finalize())
    };
    write_meta(&tx, "parser_config_hash", &parser_config_hash)?;

    let resolver_id = if options.use_lsp { "lsp" } else { "noop" };
    write_meta(&tx, "resolver_id", resolver_id)?;
    write_meta(&tx, "resolver_version", "0.1.0")?;
    let resolver_config_hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(
            format!(
                "use_lsp:{:?},max_depth:{:?}",
                options.use_lsp, options.max_depth
            )
            .as_bytes(),
        );
        format!("{:x}", hasher.finalize())
    };
    write_meta(&tx, "resolver_config_hash", &resolver_config_hash)?;

    let change_detection = match options.change_detection {
        FileChangeDetection::MtimeAndSize => "MtimeAndSize",
        FileChangeDetection::ContentHash => "ContentHash",
    };
    write_meta(&tx, "change_detection_strategy", change_detection)?;
    write_meta(&tx, "base_index_ready", "true")?;

    if options.use_lsp {
        write_meta(&tx, "lsp_enrichment", "complete")?;
    } else {
        write_meta(&tx, "lsp_enrichment", "none")?;
    }

    tx.commit()?;

    validate_index_invariants(conn)?;

    let final_index = load_index(conn, workspace_root)?;

    let lsp_count = final_index
        .edges
        .iter()
        .filter(|e| e.confidence == ResolutionConfidence::LspExact)
        .count();
    let syntax_count = final_index
        .edges
        .iter()
        .filter(|e| e.confidence == ResolutionConfidence::Syntax)
        .count();
    let heuristic_count = final_index
        .edges
        .iter()
        .filter(|e| e.confidence == ResolutionConfidence::Heuristic)
        .count();
    let unresolved_count = final_index
        .edges
        .iter()
        .filter(|e| e.confidence == ResolutionConfidence::Unresolved)
        .count();

    let report = crate::model::BuildReport {
        full_rebuild: false,
        full_rebuild_reason: None,
        added_files: diff.added.len(),
        modified_files: diff.modified.len(),
        deleted_files: diff.deleted.len(),
        unchanged_files: diff.unchanged.len(),
        parsed_files: parsed_files_count,
        reused_files: diff.unchanged.len(),
        symbols_written,
        call_sites_written,
        edges_written: final_index.edges.len(),
        lsp_edges_exact: lsp_count,
        syntax_edges: syntax_count,
        heuristic_edges: heuristic_count,
        unresolved_edges: unresolved_count,
    };

    Ok((final_index, report))
}

pub fn find_symbols(
    conn: &rusqlite::Connection,
    query: &str,
) -> Result<Vec<Symbol>, CodeGraphError> {
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let mut add_candidates = |sql: &str, param: &str| -> Result<(), CodeGraphError> {
        let mut stmt = conn.prepare(sql)?;
        let mut rows = stmt.query(rusqlite::params![param])?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            if seen.contains(&id) {
                continue;
            }
            seen.insert(id);

            let file_id: i64 = row.get(1)?;
            let name: String = row.get(2)?;
            let qualified_name: String = row.get(3)?;
            let kind_str: String = row.get(4)?;
            let lang_str: String = row.get(5)?;

            let start_line: usize = row.get(6)?;
            let start_col: usize = row.get(7)?;
            let end_line: usize = row.get(8)?;
            let end_col: usize = row.get(9)?;

            let body_start_line: Option<usize> = row.get(10)?;
            let body_start_col: Option<usize> = row.get(11)?;
            let body_end_line: Option<usize> = row.get(12)?;
            let body_end_col: Option<usize> = row.get(13)?;
            let file_path: String = row.get(14)?;

            let body_range = if let (Some(sl), Some(sc), Some(el), Some(ec)) =
                (body_start_line, body_start_col, body_end_line, body_end_col)
            {
                Some(TextRange {
                    start_line: sl,
                    start_col: sc,
                    end_line: el,
                    end_col: ec,
                })
            } else {
                None
            };

            results.push(Symbol {
                id: Some(SymbolId(id)),
                file_id: Some(FileId(file_id)),
                name,
                qualified_name,
                kind: SymbolKind::from_str(&kind_str).unwrap_or(SymbolKind::Function),
                language: Language(lang_str),
                file: PathBuf::from(file_path),
                range: TextRange {
                    start_line,
                    start_col,
                    end_line,
                    end_col,
                },
                body_range,
            });
        }
        Ok(())
    };

    add_candidates(
        "SELECT s.id, s.file_id, s.name, s.qualified_name, s.kind, s.language, s.start_line, s.start_col, s.end_line, s.end_col, s.body_start_line, s.body_start_col, s.body_end_line, s.body_end_col, f.path FROM symbols s LEFT JOIN files f ON s.file_id = f.id WHERE s.qualified_name = ?1",
        query,
    )?;

    add_candidates(
        "SELECT s.id, s.file_id, s.name, s.qualified_name, s.kind, s.language, s.start_line, s.start_col, s.end_line, s.end_col, s.body_start_line, s.body_start_col, s.body_end_line, s.body_end_col, f.path FROM symbols s LEFT JOIN files f ON s.file_id = f.id WHERE s.name = ?1",
        query,
    )?;

    add_candidates(
        "SELECT s.id, s.file_id, s.name, s.qualified_name, s.kind, s.language, s.start_line, s.start_col, s.end_line, s.end_col, s.body_start_line, s.body_start_col, s.body_end_line, s.body_end_col, f.path FROM symbols s LEFT JOIN files f ON s.file_id = f.id WHERE s.qualified_name LIKE ?1",
        &format!("%{}%", query),
    )?;

    add_candidates(
        "SELECT s.id, s.file_id, s.name, s.qualified_name, s.kind, s.language, s.start_line, s.start_col, s.end_line, s.end_col, s.body_start_line, s.body_start_col, s.body_end_line, s.body_end_col, f.path FROM symbols s LEFT JOIN files f ON s.file_id = f.id WHERE s.name LIKE ?1",
        &format!("%{}%", query),
    )?;

    Ok(results)
}

pub fn resolve_symbol(
    conn: &rusqlite::Connection,
    query: &str,
) -> Result<SymbolResolution, CodeGraphError> {
    let candidates = find_symbols(conn, query)?;
    if candidates.is_empty() {
        return Ok(SymbolResolution::NotFound);
    }

    let mut exact_qualified = Vec::new();
    let mut exact_name = Vec::new();
    let mut partial = Vec::new();

    for sym in candidates {
        let id = sym.id.unwrap_or(SymbolId(0));
        let name = sym.name.clone();
        let qualified_name = sym.qualified_name.clone();
        let kind = LanguageObjectKind::from(sym.kind);
        let file_path = sym.file.clone();
        let range = SourceRange::from(sym.range.clone());
        let language = Some(sym.language.0.clone());

        let obj = LanguageObject {
            id,
            name,
            qualified_name,
            kind,
            file_path,
            range,
            signature: None,
            language,
        };

        if obj.qualified_name == query {
            exact_qualified.push(obj);
        } else if obj.name == query {
            exact_name.push(obj);
        } else {
            partial.push(obj);
        }
    }

    if !exact_qualified.is_empty() {
        if exact_qualified.len() == 1 {
            let mut exact_qualified = exact_qualified;
            Ok(SymbolResolution::Unique(exact_qualified.remove(0)))
        } else {
            Ok(SymbolResolution::Ambiguous(exact_qualified))
        }
    } else if !exact_name.is_empty() {
        if exact_name.len() == 1 {
            let mut exact_name = exact_name;
            Ok(SymbolResolution::Unique(exact_name.remove(0)))
        } else {
            Ok(SymbolResolution::Ambiguous(exact_name))
        }
    } else {
        if partial.len() == 1 {
            let mut partial = partial;
            Ok(SymbolResolution::Unique(partial.remove(0)))
        } else {
            Ok(SymbolResolution::Ambiguous(partial))
        }
    }
}

pub fn load_callees(
    conn: &rusqlite::Connection,
    symbol_id: SymbolId,
) -> Result<Vec<(CallEdge, Option<Symbol>)>, CodeGraphError> {
    let mut results = Vec::new();
    let mut stmt = conn.prepare(
        "
        SELECT 
            e.to_symbol_id,
            e.occurrence_id,
            e.raw_text,
            e.confidence,
            c.start_line,
            c.start_col,
            c.end_line,
            c.end_col,
            s.file_id,
            s.name,
            s.qualified_name,
            s.kind,
            s.language,
            s.start_line,
            s.start_col,
            s.end_line,
            s.end_col,
            s.body_start_line,
            s.body_start_col,
            s.body_end_line,
            s.body_end_col,
            f.path
        FROM edges e
        LEFT JOIN occurrences c ON e.occurrence_id = c.id
        LEFT JOIN symbols s ON e.to_symbol_id = s.id
        LEFT JOIN files f ON s.file_id = f.id
        WHERE e.from_symbol_id = ?1 AND e.kind = 'Call'
    ",
    )?;
    let mut rows = stmt.query(rusqlite::params![symbol_id.0])?;
    while let Some(row) = rows.next()? {
        let to_symbol_id: Option<i64> = row.get(0)?;
        let occurrence_id: Option<i64> = row.get(1)?;
        let raw_name: String = row.get(2).unwrap_or_default();
        let confidence_str: String = row.get(3)?;

        let cs_start_line: usize = row.get(4).unwrap_or(0);
        let cs_start_col: usize = row.get(5).unwrap_or(0);
        let cs_end_line: usize = row.get(6).unwrap_or(0);
        let cs_end_col: usize = row.get(7).unwrap_or(0);

        let call_range = TextRange {
            start_line: cs_start_line,
            start_col: cs_start_col,
            end_line: cs_end_line,
            end_col: cs_end_col,
        };

        let edge = CallEdge {
            id: None,
            kind: EdgeKind::Call,
            from_file_id: None,
            from_symbol_id: Some(symbol_id),
            to_symbol_id: to_symbol_id.map(SymbolId),
            to_external: None,
            occurrence_id: occurrence_id.map(OccurrenceId),
            raw_text: Some(raw_name),
            range: Some(call_range),
            confidence: ResolutionConfidence::from_str(&confidence_str)
                .unwrap_or(ResolutionConfidence::Unresolved),
            produced_by: None,
        };

        let target_symbol = if let Some(to_id) = to_symbol_id {
            let s_file_id: i64 = row.get(8)?;
            let s_name: String = row.get(9)?;
            let s_qualified_name: String = row.get(10)?;
            let s_kind_str: String = row.get(11)?;
            let s_lang_str: String = row.get(12)?;
            let s_start_line: usize = row.get(13)?;
            let s_start_col: usize = row.get(14)?;
            let s_end_line: usize = row.get(15)?;
            let s_end_col: usize = row.get(16)?;
            let s_body_start_line: Option<usize> = row.get(17)?;
            let s_body_start_col: Option<usize> = row.get(18)?;
            let s_body_end_line: Option<usize> = row.get(19)?;
            let s_body_end_col: Option<usize> = row.get(20)?;
            let s_file_path: String = row.get(21)?;

            let body_range = if let (Some(sl), Some(sc), Some(el), Some(ec)) = (
                s_body_start_line,
                s_body_start_col,
                s_body_end_line,
                s_body_end_col,
            ) {
                Some(TextRange {
                    start_line: sl,
                    start_col: sc,
                    end_line: el,
                    end_col: ec,
                })
            } else {
                None
            };

            Some(Symbol {
                id: Some(SymbolId(to_id)),
                file_id: Some(FileId(s_file_id)),
                name: s_name,
                qualified_name: s_qualified_name,
                kind: SymbolKind::from_str(&s_kind_str).unwrap_or(SymbolKind::Function),
                language: Language(s_lang_str),
                file: PathBuf::from(s_file_path),
                range: TextRange {
                    start_line: s_start_line,
                    start_col: s_start_col,
                    end_line: s_end_line,
                    end_col: s_end_col,
                },
                body_range,
            })
        } else {
            None
        };

        results.push((edge, target_symbol));
    }
    Ok(results)
}

pub fn load_callers(
    conn: &rusqlite::Connection,
    symbol_id: SymbolId,
) -> Result<Vec<(CallEdge, Symbol)>, CodeGraphError> {
    let mut results = Vec::new();
    let mut stmt = conn.prepare(
        "
        SELECT 
            e.from_symbol_id,
            e.occurrence_id,
            e.raw_text,
            e.confidence,
            c.start_line,
            c.start_col,
            c.end_line,
            c.end_col,
            s.file_id,
            s.name,
            s.qualified_name,
            s.kind,
            s.language,
            s.start_line,
            s.start_col,
            s.end_line,
            s.end_col,
            s.body_start_line,
            s.body_start_col,
            s.body_end_line,
            s.body_end_col,
            f.path
        FROM edges e
        LEFT JOIN occurrences c ON e.occurrence_id = c.id
        LEFT JOIN symbols s ON e.from_symbol_id = s.id
        LEFT JOIN files f ON s.file_id = f.id
        WHERE e.to_symbol_id = ?1 AND e.kind = 'Call'
    ",
    )?;
    let mut rows = stmt.query(rusqlite::params![symbol_id.0])?;
    while let Some(row) = rows.next()? {
        let from_symbol_id: Option<i64> = row.get(0)?;
        let occurrence_id: Option<i64> = row.get(1)?;
        let raw_name: String = row.get(2).unwrap_or_default();
        let confidence_str: String = row.get(3)?;

        let cs_start_line: usize = row.get(4).unwrap_or(0);
        let cs_start_col: usize = row.get(5).unwrap_or(0);
        let cs_end_line: usize = row.get(6).unwrap_or(0);
        let cs_end_col: usize = row.get(7).unwrap_or(0);

        let call_range = TextRange {
            start_line: cs_start_line,
            start_col: cs_start_col,
            end_line: cs_end_line,
            end_col: cs_end_col,
        };

        let from_symbol_id = from_symbol_id.ok_or_else(|| {
            CodeGraphError::Parse("Caller edge without from_symbol_id".to_string())
        })?;

        let edge = CallEdge {
            id: None,
            kind: EdgeKind::Call,
            from_file_id: None,
            from_symbol_id: Some(SymbolId(from_symbol_id)),
            to_symbol_id: Some(symbol_id),
            to_external: None,
            occurrence_id: occurrence_id.map(OccurrenceId),
            raw_text: Some(raw_name),
            range: Some(call_range),
            confidence: ResolutionConfidence::from_str(&confidence_str)
                .unwrap_or(ResolutionConfidence::Unresolved),
            produced_by: None,
        };

        let s_file_id: i64 = row.get(8)?;
        let s_name: String = row.get(9)?;
        let s_qualified_name: String = row.get(10)?;
        let s_kind_str: String = row.get(11)?;
        let s_lang_str: String = row.get(12)?;
        let s_start_line: usize = row.get(13)?;
        let s_start_col: usize = row.get(14)?;
        let s_end_line: usize = row.get(15)?;
        let s_end_col: usize = row.get(16)?;
        let s_body_start_line: Option<usize> = row.get(17)?;
        let s_body_start_col: Option<usize> = row.get(18)?;
        let s_body_end_line: Option<usize> = row.get(19)?;
        let s_body_end_col: Option<usize> = row.get(20)?;
        let s_file_path: String = row.get(21)?;

        let body_range = if let (Some(sl), Some(sc), Some(el), Some(ec)) = (
            s_body_start_line,
            s_body_start_col,
            s_body_end_line,
            s_body_end_col,
        ) {
            Some(TextRange {
                start_line: sl,
                start_col: sc,
                end_line: el,
                end_col: ec,
            })
        } else {
            None
        };

        let caller_symbol = Symbol {
            id: Some(SymbolId(from_symbol_id)),
            file_id: Some(FileId(s_file_id)),
            name: s_name,
            qualified_name: s_qualified_name,
            kind: SymbolKind::from_str(&s_kind_str).unwrap_or(SymbolKind::Function),
            language: Language(s_lang_str),
            file: PathBuf::from(s_file_path),
            range: TextRange {
                start_line: s_start_line,
                start_col: s_start_col,
                end_line: s_end_line,
                end_col: s_end_col,
            },
            body_range,
        };

        results.push((edge, caller_symbol));
    }
    Ok(results)
}

pub fn load_symbols_for_file(
    conn: &rusqlite::Connection,
    path: &Path,
) -> Result<Vec<Symbol>, CodeGraphError> {
    let path_str = path.to_string_lossy().to_string();

    let file_id_res: Result<i64, rusqlite::Error> = conn.query_row(
        "SELECT id FROM files WHERE path = ?1",
        rusqlite::params![path_str],
        |r| r.get(0),
    );

    let file_id = match file_id_res {
        Ok(id) => id,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let mut results = Vec::new();
    let mut stmt = conn.prepare(
        "
        SELECT id, name, qualified_name, kind, language,
               start_line, start_col, end_line, end_col,
               body_start_line, body_start_col, body_end_line, body_end_col
        FROM symbols WHERE file_id = ?1
    ",
    )?;
    let mut rows = stmt.query(rusqlite::params![file_id])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let qualified_name: String = row.get(2)?;
        let kind_str: String = row.get(3)?;
        let lang_str: String = row.get(4)?;

        let start_line: usize = row.get(5)?;
        let start_col: usize = row.get(6)?;
        let end_line: usize = row.get(7)?;
        let end_col: usize = row.get(8)?;

        let body_start_line: Option<usize> = row.get(9)?;
        let body_start_col: Option<usize> = row.get(10)?;
        let body_end_line: Option<usize> = row.get(11)?;
        let body_end_col: Option<usize> = row.get(12)?;

        let body_range = if let (Some(sl), Some(sc), Some(el), Some(ec)) =
            (body_start_line, body_start_col, body_end_line, body_end_col)
        {
            Some(TextRange {
                start_line: sl,
                start_col: sc,
                end_line: el,
                end_col: ec,
            })
        } else {
            None
        };

        results.push(Symbol {
            id: Some(SymbolId(id)),
            file_id: Some(FileId(file_id)),
            name,
            qualified_name,
            kind: SymbolKind::from_str(&kind_str).unwrap_or(SymbolKind::Function),
            language: Language(lang_str),
            file: path.to_path_buf(),
            range: TextRange {
                start_line,
                start_col,
                end_line,
                end_col,
            },
            body_range,
        });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CallEdge, CallId, CallSite, Language, SymbolKind, TextRange};
    use std::path::PathBuf;

    #[test]
    fn test_initializes_schema() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db(dir.path()).unwrap();
        init_schema(&conn).unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            )
            .unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
        let mut tables = vec![];
        for row in rows {
            tables.push(row.unwrap());
        }

        assert!(tables.contains(&"metadata".to_string()));
        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"symbols".to_string()));
        assert!(tables.contains(&"occurrences".to_string()));
        assert!(tables.contains(&"edges".to_string()));
    }

    #[test]
    fn test_save_load_and_clear_index() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open_db(dir.path()).unwrap();
        init_schema(&conn).unwrap();

        let mut index = CodeIndex {
            root: dir.path().to_path_buf(),
            files: vec![FileSnapshot {
                file_id: None,
                rel_path: PathBuf::from("src/lib.rs"),
                abs_path: dir.path().join("src/lib.rs"),
                language: Language::rust(),
                backend_id: "rust-backend".to_string(),
                size_bytes: 200,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: Some("hash1".to_string()),
                parser_id: "tree-sitter-rust".to_string(),
                parser_version: "0.20.0".to_string(),
                parser_config_hash: "".to_string(),
                indexed_at_ms: None,
                parse_status: FileParseStatus::Success,
            }],
            symbols: vec![
                Symbol {
                    id: None,
                    file_id: None,
                    name: "run_pipeline".to_string(),
                    qualified_name: "mod::run_pipeline".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::rust(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 1,
                        start_col: 1,
                        end_line: 5,
                        end_col: 1,
                    },
                    body_range: None,
                },
                Symbol {
                    id: None,
                    file_id: None,
                    name: "load".to_string(),
                    qualified_name: "mod::load".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::rust(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 6,
                        start_col: 1,
                        end_line: 10,
                        end_col: 1,
                    },
                    body_range: None,
                },
            ],
            occurrences: vec![Occurrence {
                id: None,
                file_id: None,
                enclosing_symbol: Some(SymbolId(0)),
                enclosing_temp_index: Some(0),
                kind: OccurrenceKind::Call,
                raw_text: "load".to_string(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 3,
                    start_col: 5,
                    end_line: 3,
                    end_col: 10,
                },
                language: LanguageId::rust(),
                backend_id: "rust-backend".to_string(),
            }],
            call_sites: vec![Occurrence {
                id: None,
                file_id: None,
                enclosing_symbol: Some(SymbolId(0)),
                enclosing_temp_index: Some(0),
                kind: OccurrenceKind::Call,
                raw_text: "load".to_string(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 3,
                    start_col: 5,
                    end_line: 3,
                    end_col: 10,
                },
                language: LanguageId::rust(),
                backend_id: "rust-backend".to_string(),
            }],
            edges: vec![CallEdge {
                id: None,
                kind: EdgeKind::Call,
                from_file_id: None,
                from_symbol_id: Some(SymbolId(0)),
                to_symbol_id: Some(SymbolId(1)),
                to_external: None,
                occurrence_id: Some(OccurrenceId(0)),
                raw_text: Some("load".to_string()),
                range: Some(TextRange {
                    start_line: 3,
                    start_col: 5,
                    end_line: 3,
                    end_col: 10,
                }),
                confidence: ResolutionConfidence::Heuristic,
                produced_by: None,
            }],
        };

        // 5.2 Saves and loads index
        save_index(&mut conn, &mut index).unwrap();

        let loaded = load_index(&conn, dir.path()).unwrap();
        assert_eq!(loaded.files.len(), 1);
        assert_eq!(loaded.symbols.len(), 2);
        assert_eq!(loaded.call_sites.len(), 1);
        assert_eq!(loaded.edges.len(), 1);

        assert_eq!(loaded.symbols[0].name, "run_pipeline");
        assert_eq!(loaded.symbols[1].name, "load");

        let edge = &loaded.edges[0];
        assert_eq!(edge.from_symbol_id, loaded.symbols[0].id);
        assert_eq!(edge.to_symbol_id, loaded.symbols[1].id);

        // 5.3 Clear index removes old data
        clear_index(&mut conn).unwrap();
        let cleared = load_index(&conn, dir.path()).unwrap();
        assert!(cleared.files.is_empty());
        assert!(cleared.symbols.is_empty());
        assert!(cleared.call_sites.is_empty());
        assert!(cleared.edges.is_empty());
    }

    #[test]
    fn test_find_symbols_exact_and_partial() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open_db(dir.path()).unwrap();
        init_schema(&conn).unwrap();

        let mut index = CodeIndex {
            root: dir.path().to_path_buf(),
            files: vec![FileSnapshot {
                file_id: None,
                rel_path: PathBuf::from("src/lib.rs"),
                abs_path: dir.path().join("src/lib.rs"),
                language: Language::rust(),
                backend_id: "rust-backend".to_string(),
                size_bytes: 200,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: Some("hash1".to_string()),
                parser_id: "tree-sitter-rust".to_string(),
                parser_version: "0.20.0".to_string(),
                parser_config_hash: "".to_string(),
                indexed_at_ms: None,
                parse_status: FileParseStatus::Success,
            }],
            symbols: vec![Symbol {
                id: None,
                file_id: None,
                name: "run_pipeline".to_string(),
                qualified_name: "mod::run_pipeline".to_string(),
                kind: SymbolKind::Function,
                language: Language::rust(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 5,
                    end_col: 1,
                },
                body_range: None,
            }],
            occurrences: vec![],
            call_sites: vec![],
            edges: vec![],
        };
        save_index(&mut conn, &mut index).unwrap();

        // Exact match
        let exact = find_symbols(&conn, "run_pipeline").unwrap();
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].qualified_name, "mod::run_pipeline");

        // Partial match
        let partial = find_symbols(&conn, "pipeline").unwrap();
        assert_eq!(partial.len(), 1);
        assert_eq!(partial[0].name, "run_pipeline");

        // Missing match
        let missing = find_symbols(&conn, "missing").unwrap();
        assert!(missing.is_empty());
    }

    #[test]
    fn test_symbol_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open_db(dir.path()).unwrap();
        init_schema(&conn).unwrap();

        let mut index = CodeIndex {
            root: dir.path().to_path_buf(),
            files: vec![FileSnapshot {
                file_id: None,
                rel_path: PathBuf::from("src/lib.rs"),
                abs_path: dir.path().join("src/lib.rs"),
                language: Language::rust(),
                backend_id: "rust-backend".to_string(),
                size_bytes: 200,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: Some("hash1".to_string()),
                parser_id: "tree-sitter-rust".to_string(),
                parser_version: "0.20.0".to_string(),
                parser_config_hash: "".to_string(),
                indexed_at_ms: None,
                parse_status: FileParseStatus::Success,
            }],
            symbols: vec![
                Symbol {
                    id: None,
                    file_id: None,
                    name: "run_pipeline".to_string(),
                    qualified_name: "mod::run_pipeline".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::rust(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 1,
                        start_col: 1,
                        end_line: 5,
                        end_col: 1,
                    },
                    body_range: None,
                },
                Symbol {
                    id: None,
                    file_id: None,
                    name: "Pipeline".to_string(),
                    qualified_name: "mod::Pipeline".to_string(),
                    kind: SymbolKind::Struct,
                    language: Language::rust(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 6,
                        start_col: 1,
                        end_line: 10,
                        end_col: 1,
                    },
                    body_range: None,
                },
                Symbol {
                    id: None,
                    file_id: None,
                    name: "duplicate_name".to_string(),
                    qualified_name: "modA::duplicate_name".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::rust(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 11,
                        start_col: 1,
                        end_line: 15,
                        end_col: 1,
                    },
                    body_range: None,
                },
                Symbol {
                    id: None,
                    file_id: None,
                    name: "duplicate_name".to_string(),
                    qualified_name: "modB::duplicate_name".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::rust(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 16,
                        start_col: 1,
                        end_line: 20,
                        end_col: 1,
                    },
                    body_range: None,
                },
            ],
            occurrences: vec![],
            call_sites: vec![],
            edges: vec![],
        };
        save_index(&mut conn, &mut index).unwrap();

        // 1. query с одним exact match возвращает Unique
        let res1 = resolve_symbol(&conn, "run_pipeline").unwrap();
        if let SymbolResolution::Unique(ref obj) = res1 {
            assert_eq!(obj.name, "run_pipeline");
            assert_eq!(obj.qualified_name, "mod::run_pipeline");
            // 4. LanguageObjectKind корректно мапится хотя бы для функций и структур на fixture-коде
            assert_eq!(obj.kind, LanguageObjectKind::Function);
        } else {
            panic!("Expected Unique, got {:?}", res1);
        }

        let res2 = resolve_symbol(&conn, "mod::Pipeline").unwrap();
        if let SymbolResolution::Unique(ref obj) = res2 {
            assert_eq!(obj.name, "Pipeline");
            assert_eq!(obj.qualified_name, "mod::Pipeline");
            assert_eq!(obj.kind, LanguageObjectKind::Struct);
        } else {
            panic!("Expected Unique, got {:?}", res2);
        }

        // 2. query с несколькими match возвращает Ambiguous
        let res3 = resolve_symbol(&conn, "duplicate_name").unwrap();
        if let SymbolResolution::Ambiguous(ref objs) = res3 {
            assert_eq!(objs.len(), 2);
            assert!(
                objs.iter()
                    .any(|o| o.qualified_name == "modA::duplicate_name")
            );
            assert!(
                objs.iter()
                    .any(|o| o.qualified_name == "modB::duplicate_name")
            );
        } else {
            panic!("Expected Ambiguous, got {:?}", res3);
        }

        // 3. отсутствующий query возвращает NotFound
        let res4 = resolve_symbol(&conn, "non_existent").unwrap();
        assert_eq!(res4, SymbolResolution::NotFound);
    }

    #[test]
    fn test_load_callees_and_callers() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open_db(dir.path()).unwrap();
        init_schema(&conn).unwrap();

        let mut index = CodeIndex {
            root: dir.path().to_path_buf(),
            files: vec![FileSnapshot {
                file_id: None,
                rel_path: PathBuf::from("src/lib.rs"),
                abs_path: dir.path().join("src/lib.rs"),
                language: Language::rust(),
                backend_id: "rust-backend".to_string(),
                size_bytes: 200,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: Some("hash1".to_string()),
                parser_id: "tree-sitter-rust".to_string(),
                parser_version: "0.20.0".to_string(),
                parser_config_hash: "".to_string(),
                indexed_at_ms: None,
                parse_status: FileParseStatus::Success,
            }],
            symbols: vec![
                Symbol {
                    id: None,
                    file_id: None,
                    name: "run_pipeline".to_string(),
                    qualified_name: "mod::run_pipeline".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::rust(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 1,
                        start_col: 1,
                        end_line: 5,
                        end_col: 1,
                    },
                    body_range: None,
                },
                Symbol {
                    id: None,
                    file_id: None,
                    name: "load".to_string(),
                    qualified_name: "mod::load".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::rust(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 6,
                        start_col: 1,
                        end_line: 10,
                        end_col: 1,
                    },
                    body_range: None,
                },
                Symbol {
                    id: None,
                    file_id: None,
                    name: "process".to_string(),
                    qualified_name: "mod::process".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::rust(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 11,
                        start_col: 1,
                        end_line: 15,
                        end_col: 1,
                    },
                    body_range: None,
                },
                Symbol {
                    id: None,
                    file_id: None,
                    name: "test_run_pipeline".to_string(),
                    qualified_name: "mod::test_run_pipeline".to_string(),
                    kind: SymbolKind::Test,
                    language: Language::rust(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 16,
                        start_col: 1,
                        end_line: 20,
                        end_col: 1,
                    },
                    body_range: None,
                },
            ],
            occurrences: vec![
                Occurrence {
                    id: None,
                    file_id: None,
                    enclosing_symbol: Some(SymbolId(0)),
                    enclosing_temp_index: Some(0),
                    kind: OccurrenceKind::Call,
                    raw_text: "load".to_string(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 3,
                        start_col: 5,
                        end_line: 3,
                        end_col: 10,
                    },
                    language: LanguageId::rust(),
                    backend_id: "rust-backend".to_string(),
                },
                Occurrence {
                    id: None,
                    file_id: None,
                    enclosing_symbol: Some(SymbolId(0)),
                    enclosing_temp_index: Some(0),
                    kind: OccurrenceKind::Call,
                    raw_text: "process".to_string(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 4,
                        start_col: 5,
                        end_line: 4,
                        end_col: 15,
                    },
                    language: LanguageId::rust(),
                    backend_id: "rust-backend".to_string(),
                },
                Occurrence {
                    id: None,
                    file_id: None,
                    enclosing_symbol: Some(SymbolId(3)),
                    enclosing_temp_index: Some(3),
                    kind: OccurrenceKind::Call,
                    raw_text: "load".to_string(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 18,
                        start_col: 5,
                        end_line: 18,
                        end_col: 10,
                    },
                    language: LanguageId::rust(),
                    backend_id: "rust-backend".to_string(),
                },
            ],
            call_sites: vec![
                Occurrence {
                    id: None,
                    file_id: None,
                    enclosing_symbol: Some(SymbolId(0)),
                    enclosing_temp_index: Some(0),
                    kind: OccurrenceKind::Call,
                    raw_text: "load".to_string(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 3,
                        start_col: 5,
                        end_line: 3,
                        end_col: 10,
                },
                    language: LanguageId::rust(),
                    backend_id: "rust-backend".to_string(),
                },
                Occurrence {
                    id: None,
                    file_id: None,
                    enclosing_symbol: Some(SymbolId(0)),
                    enclosing_temp_index: Some(0),
                    kind: OccurrenceKind::Call,
                    raw_text: "process".to_string(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 4,
                        start_col: 5,
                        end_line: 4,
                        end_col: 15,
                    },
                    language: LanguageId::rust(),
                    backend_id: "rust-backend".to_string(),
                },
                Occurrence {
                    id: None,
                    file_id: None,
                    enclosing_symbol: Some(SymbolId(3)),
                    enclosing_temp_index: Some(3),
                    kind: OccurrenceKind::Call,
                    raw_text: "load".to_string(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 18,
                        start_col: 5,
                        end_line: 18,
                        end_col: 10,
                    },
                    language: LanguageId::rust(),
                    backend_id: "rust-backend".to_string(),
                },
            ],
            edges: vec![
                CallEdge {
                    id: None,
                    kind: EdgeKind::Call,
                    from_file_id: None,
                    from_symbol_id: Some(SymbolId(0)),
                    to_symbol_id: Some(SymbolId(1)),
                    to_external: None,
                    occurrence_id: Some(OccurrenceId(0)),
                    raw_text: Some("load".to_string()),
                    range: Some(TextRange {
                        start_line: 3,
                        start_col: 5,
                        end_line: 3,
                        end_col: 10,
                    }),
                    confidence: ResolutionConfidence::Heuristic,
                    produced_by: None,
                },
                CallEdge {
                    id: None,
                    kind: EdgeKind::Call,
                    from_file_id: None,
                    from_symbol_id: Some(SymbolId(0)),
                    to_symbol_id: Some(SymbolId(2)),
                    to_external: None,
                    occurrence_id: Some(OccurrenceId(1)),
                    raw_text: Some("process".to_string()),
                    range: Some(TextRange {
                        start_line: 4,
                        start_col: 5,
                        end_line: 4,
                        end_col: 15,
                    }),
                    confidence: ResolutionConfidence::Heuristic,
                    produced_by: None,
                },
                CallEdge {
                    id: None,
                    kind: EdgeKind::Call,
                    from_file_id: None,
                    from_symbol_id: Some(SymbolId(3)),
                    to_symbol_id: Some(SymbolId(1)),
                    to_external: None,
                    occurrence_id: Some(OccurrenceId(2)),
                    raw_text: Some("load".to_string()),
                    range: Some(TextRange {
                        start_line: 18,
                        start_col: 5,
                        end_line: 18,
                        end_col: 10,
                    }),
                    confidence: ResolutionConfidence::Heuristic,
                    produced_by: None,
                },
            ],
        };
        save_index(&mut conn, &mut index).unwrap();

        let loaded = load_index(&conn, dir.path()).unwrap();
        let run_pipeline_id = loaded
            .symbols
            .iter()
            .find(|s| s.name == "run_pipeline")
            .unwrap()
            .id
            .unwrap();
        let load_id = loaded
            .symbols
            .iter()
            .find(|s| s.name == "load")
            .unwrap()
            .id
            .unwrap();

        // 5.5 Load callees: run_pipeline -> load and process
        let callees = load_callees(&conn, run_pipeline_id).unwrap();
        assert_eq!(callees.len(), 2);
        let callee_names: Vec<String> = callees
            .into_iter()
            .map(|(_, opt_s)| opt_s.unwrap().name)
            .collect();
        assert!(callee_names.contains(&"load".to_string()));
        assert!(callee_names.contains(&"process".to_string()));

        // 5.6 Load callers: load <- run_pipeline and test_run_pipeline
        let callers = load_callers(&conn, load_id).unwrap();
        assert_eq!(callers.len(), 2);
        let caller_names: Vec<String> = callers.into_iter().map(|(_, s)| s.name).collect();
        assert!(caller_names.contains(&"run_pipeline".to_string()));
        assert!(caller_names.contains(&"test_run_pipeline".to_string()));
    }
}
