use ctx_codegraph_lang::CodeGraphError;
use ctx_codegraph_lang::model::{
    FileId, Language, LanguageObject, LanguageObjectKind, SourceRange, Symbol, SymbolId,
    SymbolKind, SymbolResolution, TextRange,
};
use std::path::{Path, PathBuf};

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
        let kind = LanguageObjectKind::from(sym.kind.clone());
        let file_path = sym.file.clone();
        let range = SourceRange::from(sym.range.clone());
        let language = Some(sym.language.0.clone());
        let signature = ctx_codegraph_lang::model::extract_signature(&sym.file, &sym.range, sym.kind.clone());

        let obj = LanguageObject {
            id,
            name,
            qualified_name,
            kind,
            file_path,
            range,
            signature,
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
pub fn load_symbol(
    conn: &rusqlite::Connection,
    symbol_id: SymbolId,
) -> Result<Symbol, CodeGraphError> {
    let mut stmt = conn.prepare(
        "
        SELECT s.id, s.file_id, s.name, s.qualified_name, s.kind, s.language,
               s.start_line, s.start_col, s.end_line, s.end_col,
               s.body_start_line, s.body_start_col, s.body_end_line, s.body_end_col,
               f.path
        FROM symbols s
        JOIN files f ON s.file_id = f.id
        WHERE s.id = ?1
    ",
    )?;
    stmt.query_row(rusqlite::params![symbol_id.0], |row| {
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
            body_range,
        })
    })
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => {
            CodeGraphError::SymbolNotFound(format!("{:?}", symbol_id))
        }
        other => CodeGraphError::from(other),
    })
}
pub fn load_symbols_by_ids(
    conn: &rusqlite::Connection,
    ids: &[SymbolId],
) -> Result<Vec<Symbol>, CodeGraphError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = ids.iter().map(|id| id.0.to_string()).collect();
    let sql = format!(
        "
        SELECT s.id, s.file_id, s.name, s.qualified_name, s.kind, s.language,
               s.start_line, s.start_col, s.end_line, s.end_col,
               s.body_start_line, s.body_start_col, s.body_end_line, s.body_end_col,
               f.path
        FROM symbols s
        JOIN files f ON s.file_id = f.id
        WHERE s.id IN ({})
    ",
        placeholders.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut results = Vec::new();
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
    use crate::storage::persist::save_index;
    use crate::storage::schema::init_schema;
    use crate::storage::workspace::open_db;
    use ctx_codegraph_lang::backend::{BackendId, BackendRegistry, ParserId};
    use ctx_codegraph_lang::model::{
        CodeIndex, FileParseStatus, FileSnapshot, Language, Symbol, SymbolId, SymbolKind,
        SymbolResolution, TextRange,
    };
    use ctx_codegraph_lang::model::LanguageObjectKind;
    use std::path::{Path, PathBuf};

    #[test]
    fn test_find_symbols_exact_and_partial() {
        let dir = tempfile::tempdir().unwrap();
        let registry = BackendRegistry::new();
        let mut conn = open_db(dir.path(), &registry).unwrap();
        init_schema(&conn, &registry).unwrap();

        let mut index = CodeIndex {
            root: dir.path().to_path_buf(),
            files: vec![FileSnapshot {
                file_id: None,
                rel_path: PathBuf::from("src/lib.rs"),
                abs_path: dir.path().join("src/lib.rs"),
                language: Language::rust(),
                backend_id: BackendId::new("rust-backend"),
                size_bytes: 200,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: Some("hash1".to_string()),
                parser_id: ParserId::new("tree-sitter-rust"),
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
        let registry = BackendRegistry::new();
        let mut conn = open_db(dir.path(), &registry).unwrap();
        init_schema(&conn, &registry).unwrap();

        let mut index = CodeIndex {
            root: dir.path().to_path_buf(),
            files: vec![FileSnapshot {
                file_id: None,
                rel_path: PathBuf::from("src/lib.rs"),
                abs_path: dir.path().join("src/lib.rs"),
                language: Language::rust(),
                backend_id: BackendId::new("rust-backend"),
                size_bytes: 200,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: Some("hash1".to_string()),
                parser_id: ParserId::new("tree-sitter-rust"),
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

    fn setup_symbols_index(dir: &tempfile::TempDir) -> (rusqlite::Connection, SymbolId, SymbolId) {
        let file_path = dir.path().join("src/lib.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();

        let registry = BackendRegistry::new();
        let mut conn = open_db(dir.path(), &registry).unwrap();
        init_schema(&conn, &registry).unwrap();

        let mut index = CodeIndex {
            root: dir.path().to_path_buf(),
            files: vec![FileSnapshot {
                file_id: None,
                rel_path: PathBuf::from("src/lib.rs"),
                abs_path: file_path.clone(),
                language: Language::rust(),
                backend_id: BackendId::new("rust-backend"),
                size_bytes: 200,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: Some("hash1".to_string()),
                parser_id: ParserId::new("tree-sitter-rust"),
                parser_version: "0.20.0".to_string(),
                parser_config_hash: "".to_string(),
                indexed_at_ms: None,
                parse_status: FileParseStatus::Success,
            }],
            symbols: vec![
                Symbol {
                    id: None,
                    file_id: None,
                    name: "alpha".to_string(),
                    qualified_name: "crate::alpha".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::rust(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 1,
                        start_col: 1,
                        end_line: 3,
                        end_col: 1,
                    },
                    body_range: Some(TextRange {
                        start_line: 2,
                        start_col: 1,
                        end_line: 2,
                        end_col: 10,
                    }),
                },
                Symbol {
                    id: None,
                    file_id: None,
                    name: "beta".to_string(),
                    qualified_name: "crate::beta".to_string(),
                    kind: SymbolKind::Struct,
                    language: Language::rust(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 4,
                        start_col: 1,
                        end_line: 6,
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

        let alpha_id = index.symbols[0].id.unwrap();
        let beta_id = index.symbols[1].id.unwrap();
        (conn, alpha_id, beta_id)
    }

    #[test]
    fn test_load_symbol_by_id() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, alpha_id, _) = setup_symbols_index(&dir);

        let loaded = load_symbol(&conn, alpha_id).unwrap();
        assert_eq!(loaded.id, Some(alpha_id));
        assert_eq!(loaded.name, "alpha");
        assert_eq!(loaded.qualified_name, "crate::alpha");
        assert_eq!(loaded.kind, SymbolKind::Function);
        assert_eq!(loaded.file, dir.path().join("src/lib.rs"));
        let body = loaded.body_range.unwrap();
        assert_eq!(body.start_line, 2);
        assert_eq!(body.end_col, 10);
    }

    #[test]
    fn test_load_symbol_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _, _) = setup_symbols_index(&dir);

        let err = load_symbol(&conn, SymbolId(99999)).unwrap_err();
        assert!(matches!(err, CodeGraphError::SymbolNotFound(_)));
    }

    #[test]
    fn test_load_symbols_by_ids() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, alpha_id, beta_id) = setup_symbols_index(&dir);

        let empty = load_symbols_by_ids(&conn, &[]).unwrap();
        assert!(empty.is_empty());

        let loaded = load_symbols_by_ids(&conn, &[alpha_id, beta_id]).unwrap();
        assert_eq!(loaded.len(), 2);
        let names: Vec<&str> = loaded.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
    }

    #[test]
    fn test_load_symbols_for_file() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, alpha_id, beta_id) = setup_symbols_index(&dir);
        let file_path = dir.path().join("src/lib.rs");

        let loaded = load_symbols_for_file(&conn, &file_path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().any(|s| s.id == Some(alpha_id)));
        assert!(loaded.iter().any(|s| s.id == Some(beta_id)));
        assert!(loaded.iter().all(|s| s.file == file_path));
    }

    #[test]
    fn test_load_symbols_for_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _, _) = setup_symbols_index(&dir);

        let loaded = load_symbols_for_file(&conn, Path::new("/no/such/file.rs")).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_resolve_symbol_partial_unique() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _, _) = setup_symbols_index(&dir);

        let unique_partial = resolve_symbol(&conn, "alp").unwrap();
        if let SymbolResolution::Unique(ref obj) = unique_partial {
            assert_eq!(obj.name, "alpha");
        } else {
            panic!("Expected Unique partial match, got {:?}", unique_partial);
        }
    }

    #[test]
    fn test_resolve_symbol_partial_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let registry = BackendRegistry::new();
        let mut conn = open_db(dir.path(), &registry).unwrap();
        init_schema(&conn, &registry).unwrap();

        let mut index = CodeIndex {
            root: dir.path().to_path_buf(),
            files: vec![FileSnapshot {
                file_id: None,
                rel_path: PathBuf::from("src/other.rs"),
                abs_path: dir.path().join("src/other.rs"),
                language: Language::rust(),
                backend_id: BackendId::new("rust-backend"),
                size_bytes: 100,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: None,
                parser_id: ParserId::new("tree-sitter-rust"),
                parser_version: "0.20.0".to_string(),
                parser_config_hash: "".to_string(),
                indexed_at_ms: None,
                parse_status: FileParseStatus::Success,
            }],
            symbols: vec![
                Symbol {
                    id: None,
                    file_id: None,
                    name: "foo_handler".to_string(),
                    qualified_name: "a::foo_handler".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::rust(),
                    file: PathBuf::from("src/other.rs"),
                    range: TextRange {
                        start_line: 1,
                        start_col: 1,
                        end_line: 2,
                        end_col: 1,
                    },
                    body_range: None,
                },
                Symbol {
                    id: None,
                    file_id: None,
                    name: "bar_handler".to_string(),
                    qualified_name: "b::bar_handler".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::rust(),
                    file: PathBuf::from("src/other.rs"),
                    range: TextRange {
                        start_line: 3,
                        start_col: 1,
                        end_line: 4,
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

        let ambiguous_partial = resolve_symbol(&conn, "handler").unwrap();
        if let SymbolResolution::Ambiguous(ref objs) = ambiguous_partial {
            assert_eq!(objs.len(), 2);
        } else {
            panic!(
                "Expected Ambiguous partial match, got {:?}",
                ambiguous_partial
            );
        }
    }

}
