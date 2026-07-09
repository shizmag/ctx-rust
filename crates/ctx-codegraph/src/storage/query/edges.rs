use crate::error::CodeGraphError;
use crate::model::{
    CallEdge, EdgeDirection, EdgeId, EdgeKind, FileId, GraphEdge, Language, OccurrenceId,
    ResolutionConfidence, ResolvedEdgeTarget, Symbol, SymbolId, SymbolKind, TextRange,
};
use std::path::PathBuf;

enum EdgeQueryDirection {
    Outbound,
    Inbound,
}

fn parse_graph_edge(row: &rusqlite::Row<'_>) -> Result<GraphEdge, rusqlite::Error> {
    let edge_id: i64 = row.get(0)?;
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

    Ok(GraphEdge {
        id: Some(EdgeId(edge_id)),
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
    })
}

fn parse_joined_symbol(row: &rusqlite::Row<'_>, symbol_id: i64) -> Result<Symbol, rusqlite::Error> {
    let s_file_id: i64 = row.get(14)?;
    let s_name: String = row.get(15)?;
    let s_qualified_name: String = row.get(16)?;
    let s_kind_str: String = row.get(17)?;
    let s_lang_str: String = row.get(18)?;
    let s_start_line: usize = row.get(19)?;
    let s_start_col: usize = row.get(20)?;
    let s_end_line: usize = row.get(21)?;
    let s_end_col: usize = row.get(22)?;
    let s_body_start_line: Option<usize> = row.get(23)?;
    let s_body_start_col: Option<usize> = row.get(24)?;
    let s_body_end_line: Option<usize> = row.get(25)?;
    let s_body_end_col: Option<usize> = row.get(26)?;
    let s_file_path: String = row.get(27)?;

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

    Ok(Symbol {
        id: Some(SymbolId(symbol_id)),
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
}

fn load_edges(
    conn: &rusqlite::Connection,
    symbol_id: SymbolId,
    direction: EdgeQueryDirection,
    edge_kinds: &[EdgeKind],
) -> Result<Vec<(GraphEdge, Option<Symbol>)>, CodeGraphError> {
    if edge_kinds.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = edge_kinds
        .iter()
        .map(|k| format!("'{}'", k.as_str()))
        .collect();

    let (join_clause, where_clause) = match direction {
        EdgeQueryDirection::Outbound => (
            "LEFT JOIN symbols s ON e.to_symbol_id = s.id\n        LEFT JOIN files f ON s.file_id = f.id",
            "e.from_symbol_id = ?1",
        ),
        EdgeQueryDirection::Inbound => (
            "JOIN symbols s ON e.from_symbol_id = s.id\n        JOIN files f ON s.file_id = f.id",
            "e.to_symbol_id = ?1",
        ),
    };

    let sql = format!(
        "
        SELECT 
            e.id,
            e.kind,
            e.from_file_id,
            e.from_symbol_id,
            e.to_symbol_id,
            e.to_external,
            e.occurrence_id,
            e.raw_text,
            e.start_line,
            e.start_col,
            e.end_line,
            e.end_col,
            e.confidence,
            e.produced_by,
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
        {join_clause}
        WHERE {where_clause} AND e.kind IN ({placeholders})
    ",
        join_clause = join_clause,
        where_clause = where_clause,
        placeholders = placeholders.join(","),
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(rusqlite::params![symbol_id.0])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        let edge = parse_graph_edge(row)?;
        let joined_symbol = match direction {
            EdgeQueryDirection::Outbound => {
                let to_symbol_id: Option<i64> = row.get(4)?;
                to_symbol_id
                    .map(|id| parse_joined_symbol(row, id))
                    .transpose()?
            }
            EdgeQueryDirection::Inbound => {
                let from_symbol_id: Option<i64> = row.get(3)?;
                let from_id = from_symbol_id.ok_or_else(|| {
                    CodeGraphError::Parse("Edge without from_symbol_id".to_string())
                })?;
                Some(parse_joined_symbol(row, from_id)?)
            }
        };
        results.push((edge, joined_symbol));
    }
    Ok(results)
}

pub fn load_edges_from(
    conn: &rusqlite::Connection,
    symbol_id: SymbolId,
    edge_kinds: &[EdgeKind],
) -> Result<Vec<(GraphEdge, Option<Symbol>)>, CodeGraphError> {
    load_edges(conn, symbol_id, EdgeQueryDirection::Outbound, edge_kinds)
}

pub fn load_edges_to(
    conn: &rusqlite::Connection,
    symbol_id: SymbolId,
    edge_kinds: &[EdgeKind],
) -> Result<Vec<(GraphEdge, Symbol)>, CodeGraphError> {
    load_edges(conn, symbol_id, EdgeQueryDirection::Inbound, edge_kinds)?
        .into_iter()
        .map(|(edge, sym)| {
            sym.ok_or_else(|| CodeGraphError::Parse("Edge without from_symbol_id".to_string()))
                .map(|s| (edge, s))
        })
        .collect()
}

pub fn load_edges_for_symbol(
    conn: &rusqlite::Connection,
    symbol_id: SymbolId,
    direction: EdgeDirection,
    edge_kinds: &[EdgeKind],
) -> Result<Vec<(GraphEdge, ResolvedEdgeTarget)>, CodeGraphError> {
    match direction {
        EdgeDirection::Outbound => {
            let edges = load_edges_from(conn, symbol_id, edge_kinds)?;
            Ok(edges
                .into_iter()
                .map(|(edge, sym)| {
                    let target = if let Some(s) = sym {
                        ResolvedEdgeTarget::Symbol(s)
                    } else if let Some(ref ext) = edge.to_external {
                        ResolvedEdgeTarget::External(ext.clone())
                    } else {
                        ResolvedEdgeTarget::None
                    };
                    (edge, target)
                })
                .collect())
        }
        EdgeDirection::Inbound => {
            let edges = load_edges_to(conn, symbol_id, edge_kinds)?;
            Ok(edges
                .into_iter()
                .map(|(edge, sym)| (edge, ResolvedEdgeTarget::Symbol(sym)))
                .collect())
        }
    }
}
pub fn load_callees(
    conn: &rusqlite::Connection,
    symbol_id: SymbolId,
) -> Result<Vec<(CallEdge, Option<Symbol>)>, CodeGraphError> {
    load_edges_from(conn, symbol_id, &[EdgeKind::Call])
}

pub fn load_callers(
    conn: &rusqlite::Connection,
    symbol_id: SymbolId,
) -> Result<Vec<(CallEdge, Symbol)>, CodeGraphError> {
    load_edges_to(conn, symbol_id, &[EdgeKind::Call])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::persist::{load_index, save_index};
    use crate::storage::schema::init_schema;
    use crate::storage::workspace::open_db;
    use crate::model::{
        CallEdge, CodeIndex, EdgeKind, FileParseStatus, FileSnapshot, Language, LanguageId,
        Occurrence, OccurrenceId, OccurrenceKind, ResolutionConfidence, Symbol, SymbolKind,
        TextRange,
    };
    use std::path::PathBuf;

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
