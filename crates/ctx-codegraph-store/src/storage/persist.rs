use ctx_codegraph_lang::backend::BackendRegistry;
use ctx_codegraph_lang::CodeGraphError;
use ctx_codegraph_lang::model::{
    CodeIndex, EdgeId, EdgeKind, FileId, FileParseStatus, FileSnapshot, GraphEdge, Language,
    LanguageId, Occurrence, OccurrenceId, OccurrenceKind, ResolutionConfidence, Symbol,
    SymbolId, SymbolKind, TextRange,
};
use std::path::{Path, PathBuf};

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

            let db_call_id = ctx_codegraph_lang::model::OccurrenceId(row_id);
            let temp_call_id = ctx_codegraph_lang::model::OccurrenceId(i as i64);
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
                    let db_id =
                        temp_call_to_db_id
                            .get(&temp_call_id)
                            .copied()
                            .ok_or_else(|| {
                                CodeGraphError::Parse("Edge occurrence not saved to DB".to_string())
                            })?;
                    Some(db_id)
                }
                None => None,
            };

            let (from_file_db_id, raw_text, range) = match edge.occurrence_id {
                Some(temp_id) => {
                    let cs = &index.occurrences[temp_id.0 as usize];
                    (
                        cs.file_id,
                        Some(cs.raw_text.clone()),
                        Some(cs.range.clone()),
                    )
                }
                None => {
                    let file_id = edge.from_file_id.or(None);
                    (file_id, edge.raw_text.clone(), edge.range.clone())
                }
            };

            let from_file_db_id = from_file_db_id
                .ok_or_else(|| CodeGraphError::Parse("Edge without valid file ID".to_string()))?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::schema::init_schema;
    use crate::storage::workspace::open_db;
    use ctx_codegraph_lang::backend::{
        BackendId, BackendMetadata, BackendRegistry, LanguageBackend, ParseInput, ParsedFile,
        ParserBackend, ParserId, WorkspaceMarker,
    };
    use ctx_codegraph_lang::index::BuildIndexOptions;
    use ctx_codegraph_lang::model::{
        CallEdge, CodeIndex, EdgeKind, FileParseStatus, FileSnapshot, Language, LanguageId,
        Occurrence, OccurrenceId, OccurrenceKind, ResolutionConfidence, Symbol, SymbolKind,
        TextRange,
    };
    use std::path::{Path, PathBuf};

    struct TestRustParser;

    impl ParserBackend for TestRustParser {
        fn parser_id(&self) -> ParserId {
            ParserId::new("tree-sitter-rust")
        }

        fn parser_version(&self) -> String {
            "0.20.0".to_string()
        }

        fn parse_file(&self, _input: ParseInput<'_>) -> Result<ParsedFile, CodeGraphError> {
            Ok(ParsedFile {
                symbols: Vec::new(),
                occurrences: Vec::new(),
            })
        }
    }

    struct TestRustBackend {
        parser: TestRustParser,
    }

    impl TestRustBackend {
        fn new() -> Self {
            Self {
                parser: TestRustParser,
            }
        }
    }

    impl LanguageBackend for TestRustBackend {
        fn id(&self) -> BackendId {
            BackendId::new("rust-backend")
        }

        fn language(&self) -> Language {
            Language::rust()
        }

        fn display_name(&self) -> &'static str {
            "Rust"
        }

        fn matches_path(&self, path: &Path) -> bool {
            path.extension().and_then(|e| e.to_str()) == Some("rs")
        }

        fn parser(&self) -> &dyn ParserBackend {
            &self.parser
        }

        fn resolver(&self) -> Option<&dyn ctx_codegraph_lang::backend::ResolverBackend> {
            None
        }

        fn workspace_markers(&self) -> &[WorkspaceMarker] {
            &[]
        }

        fn metadata(&self, config: &BuildIndexOptions) -> BackendMetadata {
            BackendMetadata {
                backend_id: self.id().0,
                language: self.language().as_str().to_string(),
                parser_id: self.parser().parser_id().0,
                parser_version: self.parser().parser_version(),
                resolver_id: None,
                resolver_version: None,
                config_hash: self.config_fingerprint(config),
            }
        }

        fn config_fingerprint(&self, config: &BuildIndexOptions) -> String {
            format!("include_tests={}", config.include_tests)
        }
    }

    fn test_registry() -> BackendRegistry {
        let mut registry = BackendRegistry::new();
        registry.register(Box::new(TestRustBackend::new()));
        registry
    }

    #[test]
    fn test_save_load_and_clear_index() {
        let dir = tempfile::tempdir().unwrap();
        let registry = test_registry();
        let mut conn = open_db(dir.path(), &registry).unwrap();
        init_schema(&conn, &registry).unwrap();

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
        clear_index_with_registry(&mut conn, &registry).unwrap();
        let cleared = load_index(&conn, dir.path()).unwrap();
        assert!(cleared.files.is_empty());
        assert!(cleared.symbols.is_empty());
        assert!(cleared.call_sites.is_empty());
        assert!(cleared.edges.is_empty());
    }

}
