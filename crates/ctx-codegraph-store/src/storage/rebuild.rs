use ctx_codegraph_lang::backend::{
    BackendId, BackendRegistry, ParseInput, ParsedFile, ResolveInput, ResolverId,
};
use ctx_codegraph_lang::CodeGraphError;
use ctx_codegraph_lang::index::BuildIndexOptions;
use ctx_codegraph_lang::model::{
    AffectedSet, CodeIndex, EdgeKind, FileChangeDetection, FileId, FileParseStatus, FileSnapshot,
    IndexDiff, IndexState, Language, LanguageId, Occurrence, OccurrenceId, OccurrenceKind,
    RebuildReason, ResolutionConfidence, Symbol, SymbolId, SymbolKind, TextRange,
};
use std::path::{Path, PathBuf};

use super::diff::get_index_state_with_registry;
use super::persist::{clear_index_with_registry, load_index, save_index};
use super::schema::{init_schema, validate_index_invariants};
use super::workspace::{find_workspace_root, open_db};

pub fn rebuild_index_db_with_registry(
    root: &Path,
    options: BuildIndexOptions,
    registry: &BackendRegistry,
) -> Result<(CodeIndex, ctx_codegraph_lang::model::BuildReport), CodeGraphError> {
    let workspace_root = find_workspace_root(root, registry);
    let state = get_index_state_with_registry(&workspace_root, &options, registry)?;

    let mut conn = open_db(&workspace_root, registry)?;
    init_schema(&conn, registry)?;

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
            let report = ctx_codegraph_lang::model::BuildReport {
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
                chunks_written: 0,
                embeddings_written: 0,
                lexical_docs_written: 0,
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

/// Ensures the index is fresh (present and up-to-date w.r.t. options).
///
/// Short-circuits without invoking rebuild when `get_index_state` reports `Ready`.
/// This provides a fast path for query operations when nothing needs doing.
/// Only calls rebuild when Missing/Needs* (letting rebuild decide full vs incremental).
/// The conn-based equivalent to service load logic for callers needing direct DB access.
pub fn ensure_index_with_registry(
    root: &Path,
    options: BuildIndexOptions,
    registry: &BackendRegistry,
) -> Result<rusqlite::Connection, CodeGraphError> {
    let workspace_root = find_workspace_root(root, registry);
    let db_path = workspace_root.join(".ctx-codegraph/codegraph.sqlite");
    if !db_path.exists() {
        let _ = rebuild_index_db_with_registry(&workspace_root, options.clone(), registry)?;
    } else if let Ok(state) =
        get_index_state_with_registry(&workspace_root, &options, registry)
    {
        if !matches!(state, IndexState::Ready) {
            let _ = rebuild_index_db_with_registry(&workspace_root, options.clone(), registry)?;
        }
    } else {
        let _ = rebuild_index_db_with_registry(&workspace_root, options.clone(), registry)?;
    }
    open_db(&workspace_root, registry)
}

#[derive(Debug, Clone)]
pub struct StagedFileUpdate {
    pub snapshot: FileSnapshot,
    pub parse_result: Result<ParsedFile, String>,
    pub previous_file_id: Option<FileId>,
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
    resolvers.insert(ResolverId::new("noop"));
    for backend in registry.all() {
        if let Some(res) = backend.resolver() {
            resolvers.insert(res.resolver_id().clone());
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
        for id in rows.flatten() {
            symbols.insert(SymbolId(id));
        }
    }

    for &sym_id in &symbols {
        let mut stmt = conn.prepare("SELECT occurrence_id FROM edges WHERE to_symbol_id = ?1")?;
        let rows = stmt.query_map([sym_id.0], |row| row.get::<_, i64>(0))?;
        for cs_id in rows.flatten() {
            occurrences.insert(OccurrenceId(cs_id));
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
            backend_id: BackendId::new(backend_id),
        })
    })?;

    for r in rows {
        occurrences.push(r?);
    }
    Ok(occurrences)
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

        if options.use_lsp
            && let Some(res) = resolver {
                let resolve_input = ResolveInput {
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

        if resolved_idx.is_none() {
            let (fallback_idx, fallback_conf) =
                ctx_codegraph_lang::noop::resolve_name_only_occurrence(cs, &all_symbols);
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
            resolver
                .map(|r| r.resolver_id().0.clone())
                .unwrap_or_else(|| "noop".to_string()),
        ])?;
    }

    Ok(())
}
pub fn run_full_rebuild_with_registry(
    conn: &mut rusqlite::Connection,
    workspace_root: &Path,
    options: BuildIndexOptions,
    reason: Option<RebuildReason>,
    registry: &BackendRegistry,
) -> Result<(CodeIndex, ctx_codegraph_lang::model::BuildReport), CodeGraphError> {
    clear_index_with_registry(conn, registry)?;

    let mut base_options = options.clone();
    base_options.use_lsp = false;
    let mut index =
        ctx_codegraph_lang::index::build_index_with_registry(workspace_root, base_options, registry)?;

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
    write_meta(&tx, "schema_version", "5")?;
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
            s.insert(ResolverId::new(resolver_id));
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
    rebuild_contains_edges_in_tx(&tx)?;

    if options.use_lsp {
        write_meta(&tx, "lsp_enrichment", "complete")?;
    } else {
        write_meta(&tx, "lsp_enrichment", "none")?;
    }

    tx.commit()?;

    validate_index_invariants(conn)?;

    let config = ctx_config::find_and_load_config(workspace_root).unwrap_or_default();
    let search_report = match super::search_build::build_search_indexes(
        conn,
        workspace_root,
        &options,
        &config,
    ) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("Warning: search index build failed: {}", err);
            super::search_build::SearchBuildReport::default()
        }
    };

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

    let report = ctx_codegraph_lang::model::BuildReport {
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
        chunks_written: search_report.chunks_written,
        embeddings_written: search_report.embeddings_written,
        lexical_docs_written: search_report.lexical_docs_written,
    };

    Ok((loaded, report))
}

pub fn run_incremental_update_with_registry(
    conn: &mut rusqlite::Connection,
    workspace_root: &Path,
    options: BuildIndexOptions,
    diff: IndexDiff,
    registry: &BackendRegistry,
) -> Result<(CodeIndex, ctx_codegraph_lang::model::BuildReport), CodeGraphError> {
    let mut staged_updates = Vec::new();
    let mut get_file_id_stmt = conn.prepare("SELECT id FROM files WHERE path = ?1")?;

    for snapshot in &diff.added {
        let path = &snapshot.abs_path;
        let backend = registry.find_by_path(path).ok_or_else(|| {
            CodeGraphError::Parse(format!("No backend found for path: {}", path.display()))
        })?;
        let parse_res = backend
            .parser()
            .parse_file(ParseInput { path })
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
            .parse_file(ParseInput { path })
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
                        if let Some(ref mut idx) = cs.enclosing_temp_index
                            && let Some(&new_idx) = index_map.get(idx) {
                                *idx = new_idx;
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
                    let from_db_id = cs.enclosing_temp_index.map(|idx| sym_ids[idx]);
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
    write_meta(&tx, "schema_version", "5")?;
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

    rebuild_contains_edges_in_tx(&tx)?;

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

    let report = ctx_codegraph_lang::model::BuildReport {
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
        chunks_written: 0,
        embeddings_written: 0,
        lexical_docs_written: 0,
    };

    Ok((final_index, report))
}

fn rebuild_contains_edges_in_tx(tx: &rusqlite::Transaction<'_>) -> Result<(), CodeGraphError> {
    use ctx_codegraph_lang::model::{EdgeKind, ResolutionConfidence, SymbolKind};

    tx.execute("DELETE FROM edges WHERE kind = ?1", [EdgeKind::Contains.as_str()])?;

    fn is_container(kind: &SymbolKind) -> bool {
        matches!(
            kind,
            SymbolKind::Module
                | SymbolKind::Impl
                | SymbolKind::Struct
                | SymbolKind::Class
                | SymbolKind::Enum
                | SymbolKind::Trait
        )
    }

    let mut file_stmt = tx.prepare("SELECT id FROM files")?;
    let file_ids: Vec<FileId> = file_stmt
        .query_map([], |row| row.get::<_, i64>(0).map(FileId))?
        .collect::<Result<Vec<_>, _>>()?;

    let conn: &rusqlite::Connection = tx;
    for file_id in file_ids {
        let path_str: String = conn.query_row(
            "SELECT path FROM files WHERE id = ?1",
            [file_id.0],
            |row| row.get(0),
        )?;
        let symbols =
            super::query::load_symbols_for_file(conn, Path::new(&path_str))?;
        if symbols.is_empty() {
            continue;
        }

        let mut ordered = symbols.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|s| (s.range.start_line, s.range.end_line));

        let mut stack: Vec<&ctx_codegraph_lang::model::Symbol> = Vec::new();
        for sym in ordered {
            while let Some(top) = stack.last() {
                if sym.range.start_line > top.range.end_line {
                    stack.pop();
                } else {
                    break;
                }
            }
            if let Some(parent) = stack.last()
                && parent.id != sym.id
                && is_container(&parent.kind)
                && let (Some(from_id), Some(to_id)) = (parent.id, sym.id)
            {
                tx.execute(
                    "INSERT INTO edges (kind, from_file_id, from_symbol_id, to_symbol_id, confidence)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        EdgeKind::Contains.as_str(),
                        file_id.0,
                        from_id.0,
                        to_id.0,
                        ResolutionConfidence::Syntax.as_str(),
                    ],
                )?;
            }
            if is_container(&sym.kind) {
                stack.push(sym);
            }
        }
    }

    Ok(())
}
