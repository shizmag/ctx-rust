use crate::error::CodeGraphError;
use crate::index::BuildIndexOptions;
use crate::model::{
    CallEdge, CallSite, CodeIndex, FileId, Language, ResolutionConfidence, SourceFile, Symbol,
    SymbolId, SymbolKind, TextRange,
};
use std::path::{Path, PathBuf};

fn ensure_gitignore_entries(root: &Path) {
    let gitignore_path = root.join(".gitignore");
    let has_git = root.join(".git").exists();

    if gitignore_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&gitignore_path) {
            let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let mut changed = false;

            let has_codegraph = lines.iter().any(|l| {
                let trimmed = l.trim();
                trimmed == ".ctx-codegraph" || trimmed == ".ctx-codegraph/"
            });
            let has_ctx_wildcard = lines.iter().any(|l| {
                let trimmed = l.trim();
                trimmed == ".ctx_*" || trimmed == ".ctx_*/"
            });

            if !has_codegraph {
                lines.push(".ctx-codegraph/".to_string());
                changed = true;
            }
            if !has_ctx_wildcard {
                lines.push(".ctx_*/".to_string());
                changed = true;
            }

            if changed {
                let mut new_content = lines.join("\n");
                if !new_content.ends_with('\n') {
                    new_content.push('\n');
                }
                let _ = std::fs::write(&gitignore_path, new_content);
            }
        }
    } else if has_git {
        let content = ".ctx-codegraph/\n.ctx_*/\n";
        let _ = std::fs::write(&gitignore_path, content);
    }
}

pub fn open_db(root: &Path) -> Result<rusqlite::Connection, CodeGraphError> {
    ensure_gitignore_entries(root);
    let db_dir = root.join(".ctx-codegraph");
    std::fs::create_dir_all(&db_dir)?;
    let db_path = db_dir.join("codegraph.sqlite");
    let conn = rusqlite::Connection::open(db_path)?;
    Ok(conn)
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
        "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', '1')",
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

        let file_path = files
            .iter()
            .find(|f| f.id == Some(FileId(file_id)))
            .map(|f| f.path.clone())
            .unwrap_or_default();

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

        let file_path = files
            .iter()
            .find(|f| f.id == Some(FileId(file_id)))
            .map(|f| f.path.clone())
            .unwrap_or_default();

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

        let call_range = call_sites
            .iter()
            .find(|cs| cs.id == Some(crate::model::CallId(call_site_id)))
            .map(|cs| cs.range.clone())
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
) -> Result<CodeIndex, CodeGraphError> {
    let mut conn = open_db(root)?;
    init_schema(&conn)?;
    let mut index = crate::index::build_index(root, options)?;
    clear_index(&mut conn)?;
    save_index(&mut conn, &mut index)?;
    Ok(index)
}

fn load_symbol_by_id(conn: &rusqlite::Connection, id: SymbolId) -> Result<Symbol, CodeGraphError> {
    conn.query_row(
        "
        SELECT file_id, name, qualified_name, kind, language,
               start_line, start_col, end_line, end_col,
               body_start_line, body_start_col, body_end_line, body_end_col
        FROM symbols WHERE id = ?1",
        rusqlite::params![id.0],
        |row| {
            let file_id: i64 = row.get(0)?;
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

            let file_path: String = conn.query_row(
                "SELECT path FROM files WHERE id = ?1",
                rusqlite::params![file_id],
                |r| r.get(0),
            )?;

            Ok(Symbol {
                id: Some(id),
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
            })
        },
    )
    .map_err(Into::into)
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

            let file_path: String = conn.query_row(
                "SELECT path FROM files WHERE id = ?1",
                rusqlite::params![file_id],
                |r| r.get(0),
            )?;

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
        "SELECT id, file_id, name, qualified_name, kind, language, start_line, start_col, end_line, end_col, body_start_line, body_start_col, body_end_line, body_end_col FROM symbols WHERE qualified_name = ?1",
        query,
    )?;

    add_candidates(
        "SELECT id, file_id, name, qualified_name, kind, language, start_line, start_col, end_line, end_col, body_start_line, body_start_col, body_end_line, body_end_col FROM symbols WHERE name = ?1",
        query,
    )?;

    add_candidates(
        "SELECT id, file_id, name, qualified_name, kind, language, start_line, start_col, end_line, end_col, body_start_line, body_start_col, body_end_line, body_end_col FROM symbols WHERE qualified_name LIKE ?1",
        &format!("%{}%", query),
    )?;

    add_candidates(
        "SELECT id, file_id, name, qualified_name, kind, language, start_line, start_col, end_line, end_col, body_start_line, body_start_col, body_end_line, body_end_col FROM symbols WHERE name LIKE ?1",
        &format!("%{}%", query),
    )?;

    Ok(results)
}

pub fn load_callees(
    conn: &rusqlite::Connection,
    symbol_id: SymbolId,
) -> Result<Vec<(CallEdge, Option<Symbol>)>, CodeGraphError> {
    let mut results = Vec::new();
    let mut stmt = conn.prepare(
        "
        SELECT to_symbol_id, call_site_id, raw_name, confidence
        FROM call_edges
        WHERE from_symbol_id = ?1
    ",
    )?;
    let mut rows = stmt.query(rusqlite::params![symbol_id.0])?;
    while let Some(row) = rows.next()? {
        let to_symbol_id: Option<i64> = row.get(0)?;
        let call_site_id: i64 = row.get(1)?;
        let raw_name: String = row.get(2)?;
        let confidence_str: String = row.get(3)?;

        let (_, start_line, start_col, end_line, end_col): (i64, usize, usize, usize, usize) = conn.query_row(
            "SELECT file_id, start_line, start_col, end_line, end_col FROM call_sites WHERE id = ?1",
            rusqlite::params![call_site_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;

        let call_range = TextRange {
            start_line,
            start_col,
            end_line,
            end_col,
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
            let sym = load_symbol_by_id(conn, SymbolId(to_id))?;
            Some(sym)
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
        SELECT from_symbol_id, call_site_id, raw_name, confidence
        FROM call_edges
        WHERE to_symbol_id = ?1
    ",
    )?;
    let mut rows = stmt.query(rusqlite::params![symbol_id.0])?;
    while let Some(row) = rows.next()? {
        let from_symbol_id: i64 = row.get(0)?;
        let call_site_id: i64 = row.get(1)?;
        let raw_name: String = row.get(2)?;
        let confidence_str: String = row.get(3)?;

        let (_, start_line, start_col, end_line, end_col): (i64, usize, usize, usize, usize) = conn.query_row(
            "SELECT file_id, start_line, start_col, end_line, end_col FROM call_sites WHERE id = ?1",
            rusqlite::params![call_site_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;

        let call_range = TextRange {
            start_line,
            start_col,
            end_line,
            end_col,
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

        let caller_symbol = load_symbol_by_id(conn, SymbolId(from_symbol_id))?;

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
                confidence: ResolutionConfidence::NameOnly,
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
                    confidence: ResolutionConfidence::NameOnly,
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
                    confidence: ResolutionConfidence::NameOnly,
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
                    confidence: ResolutionConfidence::NameOnly,
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
