use crate::error::CodeGraphError;
use crate::index::BuildIndexOptions;
use crate::model::{
    CallEdge, CallSite, CodeIndex, FileChangeDetection, FileId, FileSnapshot, FullRebuildReason,
    IndexDiff, IndexState, Language, LanguageObject, LanguageObjectKind, ResolutionConfidence,
    SourceFile, SourceRange, Symbol, SymbolId, SymbolKind, SymbolResolution, TextRange,
};
use std::path::{Path, PathBuf};

pub fn find_workspace_root(start_dir: &Path) -> PathBuf {
    let mut current = match start_dir.canonicalize() {
        Ok(path) => path,
        Err(_) => start_dir.to_path_buf(),
    };
    loop {
        if current.join(".git").exists()
            || current.join(".ctxconfig").exists()
            || current.join("Cargo.toml").exists()
        {
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
) -> Result<Option<FullRebuildReason>, CodeGraphError> {
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
        return Ok(Some(FullRebuildReason::IncompatibleSchema));
    }

    // Check schema version
    let schema_version: String = match conn.query_row(
        "SELECT value FROM metadata WHERE key = 'schema_version'",
        [],
        |row| row.get(0),
    ) {
        Ok(v) => v,
        Err(_) => return Ok(Some(FullRebuildReason::IncompatibleSchema)),
    };
    if schema_version != "2" {
        return Ok(Some(FullRebuildReason::IncompatibleSchema));
    }

    // Check base_index_ready
    let base_index_ready: String = match conn.query_row(
        "SELECT value FROM metadata WHERE key = 'base_index_ready'",
        [],
        |row| row.get(0),
    ) {
        Ok(v) => v,
        Err(_) => "false".to_string(),
    };
    if base_index_ready != "true" {
        return Ok(Some(FullRebuildReason::IncompatibleConfig));
    }

    // Check relevant settings: include_tests
    let db_inc_tests: String = match conn.query_row(
        "SELECT value FROM metadata WHERE key = 'include_tests'",
        [],
        |row| row.get(0),
    ) {
        Ok(v) => v,
        Err(_) => "true".to_string(),
    };
    if db_inc_tests != options.include_tests.to_string() {
        return Ok(Some(FullRebuildReason::IncompatibleConfig));
    }

    Ok(None)
}

pub fn compute_index_diff(
    conn: &rusqlite::Connection,
    workspace_root: &Path,
    options: &BuildIndexOptions,
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
            if crate::index::should_index_path(path) {
                disk_files.insert(path.to_path_buf());
            }
        }
    }

    let mut db_files = std::collections::HashMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT path, mtime_ms, size_bytes, content_hash FROM files")?;
        let db_files_rows = stmt.query_map([], |row| {
            let path_str: String = row.get(0)?;
            let mtime_ms: Option<i64> = row.get(1)?;
            let size_bytes: Option<i64> = row.get(2)?;
            let content_hash: Option<String> = row.get(3)?;
            Ok((PathBuf::from(path_str), mtime_ms, size_bytes, content_hash))
        })?;

        for row in db_files_rows {
            let (path, mtime_ms, size_bytes, content_hash) = row?;
            db_files.insert(path, (mtime_ms, size_bytes, content_hash));
        }
    }

    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut deleted = Vec::new();
    let mut unchanged = Vec::new();

    for path in &disk_files {
        let disk_mtime = crate::index::get_mtime_ms(path).unwrap_or(0);
        let disk_size = crate::index::get_size_bytes(path).unwrap_or(0) as u64;

        if let Some((db_mtime, db_size, db_hash)) = db_files.get(path) {
            let mut disk_hash = None;
            let is_modified = match options.change_detection {
                FileChangeDetection::MtimeAndSize => {
                    let db_mtime_val = db_mtime.unwrap_or(0);
                    let db_size_val = db_size.unwrap_or(0) as u64;
                    disk_mtime != db_mtime_val || disk_size != db_size_val
                }
                FileChangeDetection::ContentHash => {
                    let computed = crate::index::compute_file_hash(path);
                    disk_hash = computed.clone();
                    computed != *db_hash
                }
            };

            let snapshot = FileSnapshot {
                path: path.clone(),
                size: disk_size,
                mtime_ms: disk_mtime,
                content_hash: disk_hash.or_else(|| db_hash.clone()),
            };

            if is_modified {
                modified.push(snapshot);
            } else {
                unchanged.push(snapshot);
            }
        } else {
            let disk_hash = if options.change_detection == FileChangeDetection::ContentHash {
                crate::index::compute_file_hash(path)
            } else {
                None
            };
            added.push(FileSnapshot {
                path: path.clone(),
                size: disk_size,
                mtime_ms: disk_mtime,
                content_hash: disk_hash,
            });
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
    let workspace_root = find_workspace_root(root);
    let db_path = workspace_root.join(".ctx-codegraph/codegraph.sqlite");
    if !db_path.exists() {
        return Ok(IndexState::NeedsFullRebuild(
            FullRebuildReason::MissingDatabase,
        ));
    }

    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(_) => {
            return Ok(IndexState::NeedsFullRebuild(
                FullRebuildReason::CorruptDatabase,
            ));
        }
    };

    if let Err(_) = conn.execute("PRAGMA foreign_keys = ON;", []) {
        return Ok(IndexState::NeedsFullRebuild(
            FullRebuildReason::CorruptDatabase,
        ));
    }

    if let Some(reason) = check_db_compatibility(&conn, options)? {
        return Ok(IndexState::NeedsFullRebuild(reason));
    }

    let diff = compute_index_diff(&conn, &workspace_root, options)?;
    if diff.added.is_empty() && diff.modified.is_empty() && diff.deleted.is_empty() {
        if options.use_rust_analyzer {
            let lsp_enrichment: String = match conn.query_row(
                "SELECT value FROM metadata WHERE key = 'lsp_enrichment'",
                [],
                |row| row.get(0),
            ) {
                Ok(v) => v,
                Err(_) => "none".to_string(),
            };
            if lsp_enrichment != "complete" {
                return Ok(IndexState::NeedsIncrementalUpdate(diff));
            }
        }
        Ok(IndexState::Ready)
    } else {
        Ok(IndexState::NeedsIncrementalUpdate(diff))
    }
}

pub fn validate_index_db(root: &Path, options: &BuildIndexOptions) -> Result<bool, CodeGraphError> {
    match get_index_state(root, options)? {
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
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS files (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            language TEXT NOT NULL,
            mtime_ms INTEGER,
            size_bytes INTEGER,
            content_hash TEXT
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

        CREATE TABLE IF NOT EXISTS call_sites (
            id INTEGER PRIMARY KEY,
            file_id INTEGER NOT NULL,
            from_symbol_id INTEGER NOT NULL,
            raw_name TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            start_col INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            end_col INTEGER NOT NULL,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
            FOREIGN KEY(from_symbol_id) REFERENCES symbols(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS call_edges (
            id INTEGER PRIMARY KEY,
            from_symbol_id INTEGER NOT NULL,
            to_symbol_id INTEGER,
            call_site_id INTEGER NOT NULL,
            raw_name TEXT NOT NULL,
            confidence TEXT NOT NULL,
            FOREIGN KEY(from_symbol_id) REFERENCES symbols(id) ON DELETE CASCADE,
            FOREIGN KEY(to_symbol_id) REFERENCES symbols(id) ON DELETE SET NULL,
            FOREIGN KEY(call_site_id) REFERENCES call_sites(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
        CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
        CREATE INDEX IF NOT EXISTS idx_symbols_qualified_name ON symbols(qualified_name);
        CREATE INDEX IF NOT EXISTS idx_symbols_file_id ON symbols(file_id);
        CREATE INDEX IF NOT EXISTS idx_call_sites_from ON call_sites(from_symbol_id);
        CREATE INDEX IF NOT EXISTS idx_call_sites_raw_name ON call_sites(raw_name);
        CREATE INDEX IF NOT EXISTS idx_edges_from ON call_edges(from_symbol_id);
        CREATE INDEX IF NOT EXISTS idx_edges_to ON call_edges(to_symbol_id);
        CREATE INDEX IF NOT EXISTS idx_edges_confidence ON call_edges(confidence);
    ",
    )?;

    conn.execute(
        "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', '2')",
        [],
    )?;
    conn.execute("INSERT OR REPLACE INTO metadata (key, value) VALUES ('backend', 'tree-sitter-rust+optional-rust-analyzer-lsp')", [])?;

    Ok(())
}

pub fn clear_index(conn: &mut rusqlite::Connection) -> Result<(), CodeGraphError> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM call_edges", [])?;
    tx.execute("DELETE FROM call_sites", [])?;
    tx.execute("DELETE FROM symbols", [])?;
    tx.execute("DELETE FROM files", [])?;
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
            INSERT INTO files (path, language, mtime_ms, size_bytes, content_hash)
            VALUES (?1, ?2, ?3, ?4, ?5)
        ",
        )?;
        for file in &mut index.files {
            let path_str = file.path.to_string_lossy().to_string();
            let row_id = stmt.insert(rusqlite::params![
                path_str,
                "Rust",
                file.mtime_ms,
                file.size_bytes,
                file.content_hash,
            ])?;
            let file_id = FileId(row_id);
            file.id = Some(file_id);
            path_to_file_id.insert(file.path.clone(), file_id);
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
            let file_id = path_to_file_id.get(&sym.file).copied().ok_or_else(|| {
                CodeGraphError::Parse(format!("File not found for symbol: {}", sym.file.display()))
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
                "Rust",
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
            INSERT INTO call_sites (
                file_id, from_symbol_id, raw_name,
                start_line, start_col, end_line, end_col
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        )?;
        for (i, cs) in index.call_sites.iter_mut().enumerate() {
            let file_id = path_to_file_id.get(&cs.file).copied().ok_or_else(|| {
                CodeGraphError::Parse(format!(
                    "File not found for call site: {}",
                    cs.file.display()
                ))
            })?;
            cs.file_id = Some(file_id);

            let from_temp_id = cs.from.ok_or_else(|| {
                CodeGraphError::Parse("Call site without enclosing symbol id".to_string())
            })?;
            let from_db_id = temp_sym_to_db_id
                .get(&from_temp_id)
                .copied()
                .ok_or_else(|| {
                    CodeGraphError::Parse("Enclosing symbol not saved to DB".to_string())
                })?;

            let row_id = stmt.insert(rusqlite::params![
                file_id.0,
                from_db_id.0,
                cs.raw_name,
                cs.range.start_line,
                cs.range.start_col,
                cs.range.end_line,
                cs.range.end_col,
            ])?;

            let db_call_id = crate::model::CallId(row_id);
            let temp_call_id = crate::model::CallId(i as i64);
            cs.id = Some(db_call_id);
            cs.from = Some(from_db_id);
            temp_call_to_db_id.insert(temp_call_id, db_call_id);
        }
    }

    {
        let mut stmt = tx.prepare(
            "
            INSERT INTO call_edges (
                from_symbol_id, to_symbol_id, call_site_id, raw_name, confidence
            ) VALUES (?1, ?2, ?3, ?4, ?5)
        ",
        )?;
        for edge in &mut index.edges {
            let from_db_id = temp_sym_to_db_id.get(&edge.from).copied().ok_or_else(|| {
                CodeGraphError::Parse("Edge source symbol not saved to DB".to_string())
            })?;
            let to_db_id = match edge.to {
                Some(temp_to) => {
                    Some(temp_sym_to_db_id.get(&temp_to).copied().ok_or_else(|| {
                        CodeGraphError::Parse("Edge target symbol not saved to DB".to_string())
                    })?)
                }
                None => None,
            };
            let temp_call_id = edge
                .call_site_id
                .ok_or_else(|| CodeGraphError::Parse("Edge without call site ID".to_string()))?;
            let db_call_id = temp_call_to_db_id
                .get(&temp_call_id)
                .copied()
                .ok_or_else(|| {
                    CodeGraphError::Parse("Edge call site not saved to DB".to_string())
                })?;

            stmt.execute(rusqlite::params![
                from_db_id.0,
                to_db_id.map(|id| id.0),
                db_call_id.0,
                edge.raw_name,
                edge.confidence.as_str(),
            ])?;

            edge.from = from_db_id;
            edge.to = to_db_id;
            edge.call_site_id = Some(db_call_id);
        }
    }

    tx.commit()?;
    Ok(())
}

pub fn load_index(conn: &rusqlite::Connection, root: &Path) -> Result<CodeIndex, CodeGraphError> {
    let mut files = Vec::new();
    let mut stmt =
        conn.prepare("SELECT id, path, language, mtime_ms, size_bytes, content_hash FROM files")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let path_str: String = row.get(1)?;
        let mtime_ms: Option<i64> = row.get(3)?;
        let size_bytes: Option<i64> = row.get(4)?;
        let content_hash: Option<String> = row.get(5)?;
        files.push(SourceFile {
            id: Some(FileId(id)),
            path: PathBuf::from(path_str),
            language: Language::Rust,
            mtime_ms,
            size_bytes,
            content_hash,
        });
    }

    let file_map: std::collections::HashMap<FileId, PathBuf> = files
        .iter()
        .filter_map(|f| f.id.map(|id| (id, f.path.clone())))
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
            language: Language::Rust,
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

    let mut call_sites = Vec::new();
    let mut stmt = conn.prepare(
        "
        SELECT id, file_id, from_symbol_id, raw_name,
               start_line, start_col, end_line, end_col
        FROM call_sites
    ",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let file_id: i64 = row.get(1)?;
        let from_symbol_id: i64 = row.get(2)?;
        let raw_name: String = row.get(3)?;
        let start_line: usize = row.get(4)?;
        let start_col: usize = row.get(5)?;
        let end_line: usize = row.get(6)?;
        let end_col: usize = row.get(7)?;

        let file_path = file_map.get(&FileId(file_id)).cloned().unwrap_or_default();

        call_sites.push(CallSite {
            id: Some(crate::model::CallId(id)),
            file_id: Some(FileId(file_id)),
            from: Some(SymbolId(from_symbol_id)),
            from_temp_index: None,
            raw_name,
            file: file_path,
            range: TextRange {
                start_line,
                start_col,
                end_line,
                end_col,
            },
        });
    }

    let call_site_map: std::collections::HashMap<crate::model::CallId, TextRange> = call_sites
        .iter()
        .filter_map(|cs| cs.id.map(|id| (id, cs.range.clone())))
        .collect();

    let mut edges = Vec::new();
    let mut stmt = conn.prepare(
        "
        SELECT from_symbol_id, to_symbol_id, call_site_id, raw_name, confidence
        FROM call_edges
    ",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let from_symbol_id: i64 = row.get(0)?;
        let to_symbol_id: Option<i64> = row.get(1)?;
        let call_site_id: i64 = row.get(2)?;
        let raw_name: String = row.get(3)?;
        let confidence_str: String = row.get(4)?;

        let call_range = call_site_map
            .get(&crate::model::CallId(call_site_id))
            .cloned()
            .unwrap_or(TextRange {
                start_line: 0,
                start_col: 0,
                end_line: 0,
                end_col: 0,
            });

        edges.push(CallEdge {
            from: SymbolId(from_symbol_id),
            to: to_symbol_id.map(SymbolId),
            call_site_id: Some(crate::model::CallId(call_site_id)),
            raw_name,
            call_range,
            confidence: ResolutionConfidence::from_str(&confidence_str)
                .unwrap_or(ResolutionConfidence::Unresolved),
        });
    }

    Ok(CodeIndex {
        root: root.to_path_buf(),
        files,
        symbols,
        call_sites,
        edges,
    })
}

pub fn rebuild_index_db(
    root: &Path,
    options: BuildIndexOptions,
) -> Result<(CodeIndex, crate::model::BuildReport), CodeGraphError> {
    let workspace_root = find_workspace_root(root);
    let state = get_index_state(&workspace_root, &options)?;

    let mut conn = open_db(&workspace_root)?;
    init_schema(&conn)?;

    match state {
        IndexState::NeedsFullRebuild(reason) => {
            let (index, report) =
                run_full_rebuild(&mut conn, &workspace_root, options, Some(reason))?;
            Ok((index, report))
        }
        IndexState::Missing => {
            let (index, report) = run_full_rebuild(
                &mut conn,
                &workspace_root,
                options,
                Some(FullRebuildReason::MissingDatabase),
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
            let (index, report) =
                run_incremental_update(&mut conn, &workspace_root, options, diff)?;
            Ok((index, report))
        }
    }
}

fn run_lsp_enrichment(
    conn: &mut rusqlite::Connection,
    workspace_root: &Path,
    symbols: &[crate::model::Symbol],
    edges: &mut [crate::model::CallEdge],
) -> Result<usize, CodeGraphError> {
    let mut client = match crate::resolver::rust_analyzer_lsp::LspClient::new(workspace_root) {
        Ok(c) => c,
        Err(e) => return Err(CodeGraphError::RustAnalyzer(e)),
    };

    // Load call sites to match them
    let index = load_index(conn, workspace_root)?;
    let all_call_sites = index.call_sites;

    // Warm up LSP
    if let Some(first_cs) = all_call_sites.first() {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(30);
        let delay = std::time::Duration::from_millis(200);

        while start.elapsed() < timeout {
            let res =
                crate::resolver::rust_analyzer_lsp::resolve_via_lsp(&mut client, first_cs, symbols);
            match res {
                Err(err) if err.contains("-32603") || err.contains("file not found") => {
                    std::thread::sleep(delay);
                }
                Ok(None) if start.elapsed() < std::time::Duration::from_millis(30000) => {
                    std::thread::sleep(delay);
                }
                _ => {
                    break;
                }
            }
        }
    }

    let tx = conn.transaction()?;
    let mut exact_count = 0;

    {
        let mut update_stmt = tx.prepare(
            "UPDATE call_edges SET to_symbol_id = ?1, confidence = 'LspExact' WHERE call_site_id = ?2"
        )?;

        for cs in &all_call_sites {
            match crate::resolver::rust_analyzer_lsp::resolve_via_lsp(&mut client, cs, symbols) {
                Ok(Some(idx)) => {
                    let to_sym = &symbols[idx];
                    let to_db_id = to_sym.id.map(|id| id.0);
                    let cs_id = cs.id.map(|id| id.0).unwrap_or(0);
                    update_stmt.execute(rusqlite::params![to_db_id, cs_id])?;

                    if let Some(edge) = edges.iter_mut().find(|e| e.call_site_id == cs.id) {
                        edge.to = to_sym.id;
                        edge.confidence = ResolutionConfidence::LspExact;
                    }
                    exact_count += 1;
                }
                _ => {}
            }
        }
    }

    tx.commit()?;
    Ok(exact_count)
}

fn run_lsp_enrichment_in_tx(
    tx: &rusqlite::Transaction,
    workspace_root: &Path,
    symbols: &[crate::model::Symbol],
    all_call_sites: &[crate::model::CallSite],
    edges: &mut [crate::model::CallEdge],
) -> Result<usize, CodeGraphError> {
    let mut client = match crate::resolver::rust_analyzer_lsp::LspClient::new(workspace_root) {
        Ok(c) => c,
        Err(e) => return Err(CodeGraphError::RustAnalyzer(e)),
    };

    if let Some(first_cs) = all_call_sites.first() {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(30);
        let delay = std::time::Duration::from_millis(200);

        while start.elapsed() < timeout {
            let res =
                crate::resolver::rust_analyzer_lsp::resolve_via_lsp(&mut client, first_cs, symbols);
            match res {
                Err(err) if err.contains("-32603") || err.contains("file not found") => {
                    std::thread::sleep(delay);
                }
                Ok(None) if start.elapsed() < std::time::Duration::from_millis(30000) => {
                    std::thread::sleep(delay);
                }
                _ => {
                    break;
                }
            }
        }
    }

    let mut exact_count = 0;
    let mut update_stmt = tx.prepare(
        "UPDATE call_edges SET to_symbol_id = ?1, confidence = 'LspExact' WHERE call_site_id = ?2",
    )?;

    for cs in all_call_sites {
        match crate::resolver::rust_analyzer_lsp::resolve_via_lsp(&mut client, cs, symbols) {
            Ok(Some(idx)) => {
                let to_sym = &symbols[idx];
                let to_db_id = to_sym.id.map(|id| id.0);
                let cs_id = cs.id.map(|id| id.0).unwrap_or(0);
                update_stmt.execute(rusqlite::params![to_db_id, cs_id])?;

                if let Some(edge) = edges.iter_mut().find(|e| e.call_site_id == cs.id) {
                    edge.to = to_sym.id;
                    edge.confidence = ResolutionConfidence::LspExact;
                }
                exact_count += 1;
            }
            _ => {}
        }
    }

    Ok(exact_count)
}

fn run_full_rebuild(
    conn: &mut rusqlite::Connection,
    workspace_root: &Path,
    options: BuildIndexOptions,
    reason: Option<FullRebuildReason>,
) -> Result<(CodeIndex, crate::model::BuildReport), CodeGraphError> {
    clear_index(conn)?;

    let mut base_options = options.clone();
    base_options.use_rust_analyzer = false;
    let mut index = crate::index::build_index(workspace_root, base_options)?;

    save_index(conn, &mut index)?;

    conn.execute(
        "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', '2')",
        [],
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO metadata (key, value) VALUES ('include_tests', ?1)",
        [options.include_tests.to_string()],
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO metadata (key, value) VALUES ('base_index_ready', 'true')",
        [],
    )?;

    let mut lsp_edges_exact = 0;
    if options.use_rust_analyzer {
        match run_lsp_enrichment(conn, workspace_root, &index.symbols, &mut index.edges) {
            Ok(count) => {
                lsp_edges_exact = count;
                conn.execute("INSERT OR REPLACE INTO metadata (key, value) VALUES ('lsp_enrichment', 'complete')", [])?;
            }
            Err(e) => {
                eprintln!(
                    "Warning: LSP enrichment failed: {}. Keeping syntax/heuristic edges.",
                    e
                );
                conn.execute("INSERT OR REPLACE INTO metadata (key, value) VALUES ('lsp_enrichment', 'failed')", [])?;
            }
        }
    } else {
        conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('lsp_enrichment', 'none')",
            [],
        )?;
    }

    let loaded = load_index(conn, workspace_root)?;

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
        lsp_edges_exact,
        syntax_edges: syntax_count,
        heuristic_edges: heuristic_count,
        unresolved_edges: unresolved_count,
    };

    Ok((loaded, report))
}

fn run_incremental_update(
    conn: &mut rusqlite::Connection,
    workspace_root: &Path,
    options: BuildIndexOptions,
    diff: IndexDiff,
) -> Result<(CodeIndex, crate::model::BuildReport), CodeGraphError> {
    let tx = conn.transaction()?;

    let mut symbols_written = 0;
    let mut call_sites_written = 0;

    // 1. Delete deleted files
    let mut delete_file_stmt = tx.prepare("DELETE FROM files WHERE path = ?1")?;
    for path in &diff.deleted {
        delete_file_stmt.execute(rusqlite::params![path.to_string_lossy().to_string()])?;
    }

    // 2. Delete modified files
    for snapshot in &diff.modified {
        delete_file_stmt.execute(rusqlite::params![
            snapshot.path.to_string_lossy().to_string()
        ])?;
    }
    drop(delete_file_stmt);

    // 3. Parse and insert added and modified files
    let mut file_insert_stmt = tx.prepare(
        "INSERT INTO files (path, language, mtime_ms, size_bytes, content_hash)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;

    let mut sym_stmt = tx.prepare(
        "INSERT INTO symbols (
            file_id, name, qualified_name, kind, language,
            start_line, start_col, end_line, end_col,
            body_start_line, body_start_col, body_end_line, body_end_col
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?;

    let mut cs_stmt = tx.prepare(
        "INSERT INTO call_sites (
            file_id, from_symbol_id, raw_name,
            start_line, start_col, end_line, end_col
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;

    let files_to_parse: Vec<&FileSnapshot> =
        diff.added.iter().chain(diff.modified.iter()).collect();
    let mut parsed_files_count = 0;

    for snapshot in files_to_parse {
        let path = &snapshot.path;
        let mtime_ms = snapshot.mtime_ms;
        let size_bytes = snapshot.size as i64;
        let content_hash = snapshot
            .content_hash
            .clone()
            .or_else(|| crate::index::compute_file_hash(path));
        let path_str = path.to_string_lossy().to_string();

        file_insert_stmt.execute(rusqlite::params![
            path_str,
            "Rust",
            mtime_ms,
            size_bytes,
            content_hash,
        ])?;
        let file_id = tx.last_insert_rowid();
        parsed_files_count += 1;

        let (mut file_symbols, mut file_call_sites) =
            match crate::languages::rust::parse_rust_file(path) {
                Ok(res) => res,
                Err(e) => {
                    eprintln!("Warning: Failed to parse {}: {}", path.display(), e);
                    continue;
                }
            };

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

            file_call_sites.retain(|cs| {
                if let Some(old_idx) = cs.from_temp_index {
                    index_map.contains_key(&old_idx)
                } else {
                    true
                }
            });

            for cs in &mut file_call_sites {
                if let Some(ref mut idx) = cs.from_temp_index {
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
                "Rust",
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

        for cs in &file_call_sites {
            let from_db_id = match cs.from_temp_index {
                Some(idx) => sym_ids[idx],
                None => continue,
            };
            cs_stmt.execute(rusqlite::params![
                file_id,
                from_db_id,
                cs.raw_name,
                cs.range.start_line,
                cs.range.start_col,
                cs.range.end_line,
                cs.range.end_col,
            ])?;
            call_sites_written += 1;
        }
    }

    drop(file_insert_stmt);
    drop(sym_stmt);
    drop(cs_stmt);

    // 4. Delete old call edges
    tx.execute("DELETE FROM call_edges", [])?;

    // Load all symbols and call sites currently persisted to resolve edges globally
    let all_index = load_index(&tx, workspace_root)?;
    let all_symbols = all_index.symbols;
    let all_call_sites = all_index.call_sites;

    // 5. Recompute call edges globally (fast resolution first)
    // We incrementally update files/symbols/call_sites, then conservatively rebuild call_edges globally.
    // This is intentionally simpler and safer than partial edge invalidation. If edge resolution becomes
    // a bottleneck, we can later add affected-edge recomputation.
    let mut edge_stmt = tx.prepare(
        "INSERT INTO call_edges (from_symbol_id, to_symbol_id, call_site_id, raw_name, confidence)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;

    let mut edges = Vec::new();
    let mut syntax_count = 0;
    let mut heuristic_count = 0;
    let mut unresolved_count = 0;

    for (call_site_idx, cs) in all_call_sites.iter().enumerate() {
        let from_id = match cs.from {
            Some(id) => id,
            None => continue,
        };

        let (resolved_idx, confidence) =
            crate::resolver::noop::resolve_name_only(&cs.raw_name, &all_symbols, &cs.file);
        let to_db_id = resolved_idx.and_then(|idx| all_symbols[idx].id);
        let cs_id = cs.id.unwrap_or(crate::model::CallId(call_site_idx as i64));

        edge_stmt.execute(rusqlite::params![
            from_id.0,
            to_db_id.map(|id| id.0),
            cs_id.0,
            cs.raw_name,
            confidence.as_str(),
        ])?;

        edges.push(crate::model::CallEdge {
            from: from_id,
            to: to_db_id,
            call_site_id: Some(cs_id),
            raw_name: cs.raw_name.clone(),
            call_range: cs.range.clone(),
            confidence,
        });

        match confidence {
            ResolutionConfidence::Syntax => syntax_count += 1,
            ResolutionConfidence::Heuristic => heuristic_count += 1,
            ResolutionConfidence::Unresolved => unresolved_count += 1,
            _ => {}
        }
    }
    drop(edge_stmt);

    let mut lsp_edges_exact = 0;
    // 6. Optional LSP enrichment
    if options.use_rust_analyzer {
        match run_lsp_enrichment_in_tx(
            &tx,
            workspace_root,
            &all_symbols,
            &all_call_sites,
            &mut edges,
        ) {
            Ok(count) => {
                lsp_edges_exact = count;
                syntax_count = edges
                    .iter()
                    .filter(|e| e.confidence == ResolutionConfidence::Syntax)
                    .count();
                heuristic_count = edges
                    .iter()
                    .filter(|e| e.confidence == ResolutionConfidence::Heuristic)
                    .count();
                unresolved_count = edges
                    .iter()
                    .filter(|e| e.confidence == ResolutionConfidence::Unresolved)
                    .count();

                tx.execute("INSERT OR REPLACE INTO metadata (key, value) VALUES ('lsp_enrichment', 'complete')", [])?;
            }
            Err(e) => {
                eprintln!(
                    "Warning: LSP enrichment failed: {}. Keeping syntax/heuristic edges.",
                    e
                );
                tx.execute("INSERT OR REPLACE INTO metadata (key, value) VALUES ('lsp_enrichment', 'failed')", [])?;
            }
        }
    } else {
        tx.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('lsp_enrichment', 'none')",
            [],
        )?;
    }

    tx.commit()?;

    let final_index = load_index(conn, workspace_root)?;
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
        lsp_edges_exact,
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
                language: Language::Rust,
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
        let language = Some(format!("{:?}", sym.language).to_lowercase());

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
            e.call_site_id,
            e.raw_name,
            e.confidence,
            c.start_line,
            c.start_col,
            c.end_line,
            c.end_col,
            s.file_id,
            s.name,
            s.qualified_name,
            s.kind,
            s.start_line,
            s.start_col,
            s.end_line,
            s.end_col,
            s.body_start_line,
            s.body_start_col,
            s.body_end_line,
            s.body_end_col,
            f.path
        FROM call_edges e
        LEFT JOIN call_sites c ON e.call_site_id = c.id
        LEFT JOIN symbols s ON e.to_symbol_id = s.id
        LEFT JOIN files f ON s.file_id = f.id
        WHERE e.from_symbol_id = ?1
    ",
    )?;
    let mut rows = stmt.query(rusqlite::params![symbol_id.0])?;
    while let Some(row) = rows.next()? {
        let to_symbol_id: Option<i64> = row.get(0)?;
        let call_site_id: i64 = row.get(1)?;
        let raw_name: String = row.get(2)?;
        let confidence_str: String = row.get(3)?;

        let cs_start_line: usize = row.get(4)?;
        let cs_start_col: usize = row.get(5)?;
        let cs_end_line: usize = row.get(6)?;
        let cs_end_col: usize = row.get(7)?;

        let call_range = TextRange {
            start_line: cs_start_line,
            start_col: cs_start_col,
            end_line: cs_end_line,
            end_col: cs_end_col,
        };

        let edge = CallEdge {
            from: symbol_id,
            to: to_symbol_id.map(SymbolId),
            call_site_id: Some(crate::model::CallId(call_site_id)),
            raw_name,
            call_range,
            confidence: ResolutionConfidence::from_str(&confidence_str)
                .unwrap_or(ResolutionConfidence::Unresolved),
        };

        let target_symbol = if let Some(to_id) = to_symbol_id {
            let s_file_id: i64 = row.get(8)?;
            let s_name: String = row.get(9)?;
            let s_qualified_name: String = row.get(10)?;
            let s_kind_str: String = row.get(11)?;
            let s_start_line: usize = row.get(12)?;
            let s_start_col: usize = row.get(13)?;
            let s_end_line: usize = row.get(14)?;
            let s_end_col: usize = row.get(15)?;
            let s_body_start_line: Option<usize> = row.get(16)?;
            let s_body_start_col: Option<usize> = row.get(17)?;
            let s_body_end_line: Option<usize> = row.get(18)?;
            let s_body_end_col: Option<usize> = row.get(19)?;
            let s_file_path: String = row.get(20)?;

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
                language: Language::Rust,
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
            e.call_site_id,
            e.raw_name,
            e.confidence,
            c.start_line,
            c.start_col,
            c.end_line,
            c.end_col,
            s.file_id,
            s.name,
            s.qualified_name,
            s.kind,
            s.start_line,
            s.start_col,
            s.end_line,
            s.end_col,
            s.body_start_line,
            s.body_start_col,
            s.body_end_line,
            s.body_end_col,
            f.path
        FROM call_edges e
        LEFT JOIN call_sites c ON e.call_site_id = c.id
        LEFT JOIN symbols s ON e.from_symbol_id = s.id
        LEFT JOIN files f ON s.file_id = f.id
        WHERE e.to_symbol_id = ?1
    ",
    )?;
    let mut rows = stmt.query(rusqlite::params![symbol_id.0])?;
    while let Some(row) = rows.next()? {
        let from_symbol_id: i64 = row.get(0)?;
        let call_site_id: i64 = row.get(1)?;
        let raw_name: String = row.get(2)?;
        let confidence_str: String = row.get(3)?;

        let cs_start_line: usize = row.get(4)?;
        let cs_start_col: usize = row.get(5)?;
        let cs_end_line: usize = row.get(6)?;
        let cs_end_col: usize = row.get(7)?;

        let call_range = TextRange {
            start_line: cs_start_line,
            start_col: cs_start_col,
            end_line: cs_end_line,
            end_col: cs_end_col,
        };

        let edge = CallEdge {
            from: SymbolId(from_symbol_id),
            to: Some(symbol_id),
            call_site_id: Some(crate::model::CallId(call_site_id)),
            raw_name,
            call_range,
            confidence: ResolutionConfidence::from_str(&confidence_str)
                .unwrap_or(ResolutionConfidence::Unresolved),
        };

        let s_file_id: i64 = row.get(8)?;
        let s_name: String = row.get(9)?;
        let s_qualified_name: String = row.get(10)?;
        let s_kind_str: String = row.get(11)?;
        let s_start_line: usize = row.get(12)?;
        let s_start_col: usize = row.get(13)?;
        let s_end_line: usize = row.get(14)?;
        let s_end_col: usize = row.get(15)?;
        let s_body_start_line: Option<usize> = row.get(16)?;
        let s_body_start_col: Option<usize> = row.get(17)?;
        let s_body_end_line: Option<usize> = row.get(18)?;
        let s_body_end_col: Option<usize> = row.get(19)?;
        let s_file_path: String = row.get(20)?;

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
            language: Language::Rust,
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
            language: Language::Rust,
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
        assert!(tables.contains(&"call_sites".to_string()));
        assert!(tables.contains(&"call_edges".to_string()));
    }

    #[test]
    fn test_save_load_and_clear_index() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open_db(dir.path()).unwrap();
        init_schema(&conn).unwrap();

        let mut index = CodeIndex {
            root: dir.path().to_path_buf(),
            files: vec![SourceFile {
                id: None,
                path: PathBuf::from("src/lib.rs"),
                language: Language::Rust,
                mtime_ms: Some(100),
                size_bytes: Some(200),
                content_hash: Some("hash1".to_string()),
            }],
            symbols: vec![
                Symbol {
                    id: None,
                    file_id: None,
                    name: "run_pipeline".to_string(),
                    qualified_name: "mod::run_pipeline".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::Rust,
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
                    language: Language::Rust,
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
            call_sites: vec![CallSite {
                id: None,
                file_id: None,
                from: Some(SymbolId(0)),
                from_temp_index: Some(0),
                raw_name: "load".to_string(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 3,
                    start_col: 5,
                    end_line: 3,
                    end_col: 10,
                },
            }],
            edges: vec![CallEdge {
                from: SymbolId(0),
                to: Some(SymbolId(1)),
                call_site_id: Some(CallId(0)),
                raw_name: "load".to_string(),
                call_range: TextRange {
                    start_line: 3,
                    start_col: 5,
                    end_line: 3,
                    end_col: 10,
                },
                confidence: ResolutionConfidence::Heuristic,
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
        assert_eq!(edge.from, loaded.symbols[0].id.unwrap());
        assert_eq!(edge.to, Some(loaded.symbols[1].id.unwrap()));

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
            files: vec![SourceFile {
                id: None,
                path: PathBuf::from("src/lib.rs"),
                language: Language::Rust,
                mtime_ms: Some(100),
                size_bytes: Some(200),
                content_hash: Some("hash1".to_string()),
            }],
            symbols: vec![Symbol {
                id: None,
                file_id: None,
                name: "run_pipeline".to_string(),
                qualified_name: "mod::run_pipeline".to_string(),
                kind: SymbolKind::Function,
                language: Language::Rust,
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 5,
                    end_col: 1,
                },
                body_range: None,
            }],
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
            files: vec![SourceFile {
                id: None,
                path: PathBuf::from("src/lib.rs"),
                language: Language::Rust,
                mtime_ms: Some(100),
                size_bytes: Some(200),
                content_hash: Some("hash1".to_string()),
            }],
            symbols: vec![
                Symbol {
                    id: None,
                    file_id: None,
                    name: "run_pipeline".to_string(),
                    qualified_name: "mod::run_pipeline".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::Rust,
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
                    language: Language::Rust,
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
                    language: Language::Rust,
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
                    language: Language::Rust,
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
            files: vec![SourceFile {
                id: None,
                path: PathBuf::from("src/lib.rs"),
                language: Language::Rust,
                mtime_ms: Some(100),
                size_bytes: Some(200),
                content_hash: Some("hash1".to_string()),
            }],
            symbols: vec![
                Symbol {
                    id: None,
                    file_id: None,
                    name: "run_pipeline".to_string(),
                    qualified_name: "mod::run_pipeline".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::Rust,
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
                    language: Language::Rust,
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
                    language: Language::Rust,
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
                    language: Language::Rust,
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
            call_sites: vec![
                CallSite {
                    id: None,
                    file_id: None,
                    from: Some(SymbolId(0)),
                    from_temp_index: Some(0),
                    raw_name: "load".to_string(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 3,
                        start_col: 5,
                        end_line: 3,
                        end_col: 10,
                    },
                },
                CallSite {
                    id: None,
                    file_id: None,
                    from: Some(SymbolId(0)),
                    from_temp_index: Some(0),
                    raw_name: "process".to_string(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 4,
                        start_col: 5,
                        end_line: 4,
                        end_col: 15,
                    },
                },
                CallSite {
                    id: None,
                    file_id: None,
                    from: Some(SymbolId(3)),
                    from_temp_index: Some(3),
                    raw_name: "load".to_string(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 18,
                        start_col: 5,
                        end_line: 18,
                        end_col: 10,
                    },
                },
            ],
            edges: vec![
                CallEdge {
                    from: SymbolId(0),
                    to: Some(SymbolId(1)),
                    call_site_id: Some(CallId(0)),
                    raw_name: "load".to_string(),
                    call_range: TextRange {
                        start_line: 3,
                        start_col: 5,
                        end_line: 3,
                        end_col: 10,
                    },
                    confidence: ResolutionConfidence::Heuristic,
                },
                CallEdge {
                    from: SymbolId(0),
                    to: Some(SymbolId(2)),
                    call_site_id: Some(CallId(1)),
                    raw_name: "process".to_string(),
                    call_range: TextRange {
                        start_line: 4,
                        start_col: 5,
                        end_line: 4,
                        end_col: 15,
                    },
                    confidence: ResolutionConfidence::Heuristic,
                },
                CallEdge {
                    from: SymbolId(3),
                    to: Some(SymbolId(1)),
                    call_site_id: Some(CallId(2)),
                    raw_name: "load".to_string(),
                    call_range: TextRange {
                        start_line: 18,
                        start_col: 5,
                        end_line: 18,
                        end_col: 10,
                    },
                    confidence: ResolutionConfidence::Heuristic,
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
