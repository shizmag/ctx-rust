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
    for i in (range.start_line - 1)..end {
        result.push_str(lines[i]);
        result.push('\n');
    }
    Ok(result)
}
