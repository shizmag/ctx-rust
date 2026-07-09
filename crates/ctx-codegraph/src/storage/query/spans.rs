use crate::error::CodeGraphError;
use crate::model::{FileId, LanguageId, Occurrence, OccurrenceId, OccurrenceKind, SymbolId, TextRange};
use std::path::PathBuf;

pub fn load_occurrence(
    conn: &rusqlite::Connection,
    occurrence_id: OccurrenceId,
) -> Result<Occurrence, CodeGraphError> {
    let mut stmt = conn.prepare(
        "
        SELECT o.id, o.file_id, o.enclosing_symbol_id, o.kind, o.raw_text,
               o.start_line, o.start_col, o.end_line, o.end_col, o.language, o.backend_id,
               f.path
        FROM occurrences o
        JOIN files f ON o.file_id = f.id
        WHERE o.id = ?1
    ",
    )?;
    stmt.query_row(rusqlite::params![occurrence_id.0], |row| {
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
        let file_path: String = row.get(11)?;

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
    })
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => {
            CodeGraphError::Parse(format!("Occurrence not found: {:?}", occurrence_id))
        }
        other => CodeGraphError::from(other),
    })
}

pub fn load_file_span(
    conn: &rusqlite::Connection,
    file_id: FileId,
    range: TextRange,
) -> Result<String, CodeGraphError> {
    let mut stmt = conn.prepare("SELECT path FROM files WHERE id = ?1")?;
    let path_str: String = stmt.query_row(rusqlite::params![file_id.0], |row| row.get(0))?;
    let path = PathBuf::from(path_str);
    let content = std::fs::read_to_string(&path)?;
    let lines: Vec<&str> = content.lines().collect();
    if range.start_line == 0 || range.start_line > lines.len() {
        return Ok("".to_string());
    }
    let end = std::cmp::min(range.end_line, lines.len());
    if range.start_line > end {
        return Ok("".to_string());
    }
    let mut result = String::new();
    for line in &lines[(range.start_line - 1)..end] {
        result.push_str(line);
        result.push('\n');
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        CodeIndex, FileId, FileParseStatus, FileSnapshot, Language, LanguageId, Occurrence,
        OccurrenceId, OccurrenceKind, Symbol, SymbolId, SymbolKind, TextRange,
    };
    use crate::storage::persist::save_index;
    use crate::storage::schema::init_schema;
    use crate::storage::workspace::open_db;
    use std::fs;
    use std::path::PathBuf;

    fn sample_file_content() -> &'static str {
        "line one\nline two\nline three\nline four\n"
    }

    fn setup_index_with_occurrence(dir: &tempfile::TempDir) -> (rusqlite::Connection, OccurrenceId, FileId) {
        let file_path = dir.path().join("src/lib.rs");
        fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        fs::write(&file_path, sample_file_content()).unwrap();

        let mut conn = open_db(dir.path()).unwrap();
        init_schema(&conn).unwrap();

        let mut index = CodeIndex {
            root: dir.path().to_path_buf(),
            files: vec![FileSnapshot {
                file_id: None,
                rel_path: PathBuf::from("src/lib.rs"),
                abs_path: file_path.clone(),
                language: Language::rust(),
                backend_id: "rust-backend".to_string(),
                size_bytes: sample_file_content().len() as u64,
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
                    end_line: 2,
                    end_col: 1,
                },
                body_range: None,
            }],
            occurrences: vec![Occurrence {
                id: None,
                file_id: None,
                enclosing_symbol: Some(SymbolId(0)),
                enclosing_temp_index: Some(0),
                kind: OccurrenceKind::Call,
                raw_text: "helper".to_string(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 2,
                    start_col: 5,
                    end_line: 2,
                    end_col: 11,
                },
                language: LanguageId::rust(),
                backend_id: "rust-backend".to_string(),
            }],
            call_sites: vec![],
            edges: vec![],
        };

        save_index(&mut conn, &mut index).unwrap();

        let occurrence_id = index.occurrences[0].id.unwrap();
        let file_id = index.files[0].file_id.unwrap();
        (conn, occurrence_id, file_id)
    }

    #[test]
    fn test_load_occurrence_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, occurrence_id, file_id) = setup_index_with_occurrence(&dir);

        let loaded = load_occurrence(&conn, occurrence_id).unwrap();

        assert_eq!(loaded.id, Some(occurrence_id));
        assert_eq!(loaded.file_id, Some(file_id));
        assert_eq!(loaded.raw_text, "helper");
        assert_eq!(loaded.kind, OccurrenceKind::Call);
        assert_eq!(loaded.range.start_line, 2);
        assert_eq!(loaded.range.end_col, 11);
        assert_eq!(loaded.file, dir.path().join("src/lib.rs"));
        assert!(loaded.enclosing_symbol.is_some());
    }

    #[test]
    fn test_load_occurrence_invalid_id_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _, _) = setup_index_with_occurrence(&dir);

        let err = load_occurrence(&conn, OccurrenceId(99999)).unwrap_err();
        assert!(matches!(err, CodeGraphError::Parse(_)));
        assert!(err.to_string().contains("Occurrence not found"));
    }

    #[test]
    fn test_load_file_span_extracts_lines_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _, file_id) = setup_index_with_occurrence(&dir);

        let span = load_file_span(
            &conn,
            file_id,
            TextRange {
                start_line: 2,
                start_col: 1,
                end_line: 3,
                end_col: 1,
            },
        )
        .unwrap();

        assert_eq!(span, "line two\nline three\n");
    }

    #[test]
    fn test_load_file_span_start_line_zero_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _, file_id) = setup_index_with_occurrence(&dir);

        let span = load_file_span(
            &conn,
            file_id,
            TextRange {
                start_line: 0,
                start_col: 1,
                end_line: 2,
                end_col: 1,
            },
        )
        .unwrap();

        assert_eq!(span, "");
    }

    #[test]
    fn test_load_file_span_start_line_beyond_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _, file_id) = setup_index_with_occurrence(&dir);

        let span = load_file_span(
            &conn,
            file_id,
            TextRange {
                start_line: 100,
                start_col: 1,
                end_line: 101,
                end_col: 1,
            },
        )
        .unwrap();

        assert_eq!(span, "");
    }

    #[test]
    fn test_load_file_span_start_line_greater_than_end_line_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _, file_id) = setup_index_with_occurrence(&dir);

        let span = load_file_span(
            &conn,
            file_id,
            TextRange {
                start_line: 3,
                start_col: 1,
                end_line: 2,
                end_col: 1,
            },
        )
        .unwrap();

        assert_eq!(span, "");
    }
}
