use ctx_codegraph_chunk::{Chunk, ChunkId, ChunkKind};
use ctx_codegraph_lang::CodeGraphError;
use ctx_codegraph_lang::model::{FileId, SymbolId};
use rusqlite::{params, Connection};

pub fn clear_chunks(conn: &Connection) -> Result<(), CodeGraphError> {
    conn.execute("DELETE FROM chunks", [])?;
    Ok(())
}

pub fn delete_chunks_for_file(conn: &Connection, file_id: FileId) -> Result<(), CodeGraphError> {
    conn.execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id.0])?;
    Ok(())
}

pub fn save_chunks(conn: &Connection, chunks: &[Chunk]) -> Result<(), CodeGraphError> {
    let mut stmt = conn.prepare_cached(
        "INSERT INTO chunks (
            id, symbol_id, parent_chunk_id, file_id, kind, text_hash,
            token_count, start_line, end_line, qualified_name
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    for chunk in chunks {
        let id = chunk.id.map(|c| c.0).unwrap_or(0);
        stmt.execute(params![
            id,
            chunk.symbol_id.map(|s| s.0),
            chunk.parent_chunk_id.map(|c| c.0),
            chunk.file_id.0,
            chunk.kind.as_str(),
            chunk.text_hash,
            chunk.token_count as i64,
            chunk.start_line as i64,
            chunk.end_line as i64,
            chunk.qualified_name,
        ])?;
    }
    Ok(())
}

pub fn load_chunk(conn: &Connection, chunk_id: ChunkId) -> Result<Option<Chunk>, CodeGraphError> {
    let mut stmt = conn.prepare(
        "SELECT symbol_id, parent_chunk_id, file_id, kind, text_hash, token_count,
                start_line, end_line, qualified_name
         FROM chunks WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![chunk_id.0])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(row_to_chunk(chunk_id, row)?))
}

pub fn load_chunks_by_ids(
    conn: &Connection,
    ids: &[ChunkId],
) -> Result<Vec<Chunk>, CodeGraphError> {
    let mut out = Vec::new();
    for id in ids {
        if let Some(chunk) = load_chunk(conn, *id)? {
            out.push(chunk);
        }
    }
    Ok(out)
}

pub fn load_child_chunks(
    conn: &Connection,
    parent_id: ChunkId,
    limit: usize,
) -> Result<Vec<Chunk>, CodeGraphError> {
    let mut stmt = conn.prepare(
        "SELECT id, symbol_id, parent_chunk_id, file_id, kind, text_hash, token_count,
                start_line, end_line, qualified_name
         FROM chunks WHERE parent_chunk_id = ?1
         ORDER BY start_line ASC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![parent_id.0, limit as i64], |row| {
        let id = ChunkId(row.get::<_, i64>(0)?);
        row_to_chunk_at(id, row, 1)
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(CodeGraphError::from)
}

pub fn load_chunks_for_symbol(
    conn: &Connection,
    symbol_id: SymbolId,
) -> Result<Vec<Chunk>, CodeGraphError> {
    let mut stmt = conn.prepare(
        "SELECT id, symbol_id, parent_chunk_id, file_id, kind, text_hash, token_count,
                start_line, end_line, qualified_name
         FROM chunks WHERE symbol_id = ?1",
    )?;
    let rows = stmt.query_map(params![symbol_id.0], |row| {
        let id = ChunkId(row.get::<_, i64>(0)?);
        row_to_chunk_at(id, row, 1)
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(CodeGraphError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::persist::save_index;
    use crate::storage::schema::init_schema;
    use crate::storage::workspace::open_db;
    use ctx_codegraph_lang::backend::{BackendId, BackendRegistry, ParserId};
    use ctx_codegraph_lang::model::{
        CodeIndex, FileParseStatus, FileSnapshot, Language, Symbol, SymbolKind, TextRange,
    };
    use std::path::PathBuf;

    fn sample_chunk(file_id: FileId, symbol_id: SymbolId, id: i64) -> Chunk {
        Chunk {
            id: Some(ChunkId(id)),
            symbol_id: Some(symbol_id),
            parent_chunk_id: None,
            file_id,
            kind: ChunkKind::SymbolBody,
            text_hash: "abc123".to_string(),
            token_count: 12,
            start_line: 1,
            end_line: 5,
            qualified_name: "mod::greet".to_string(),
            text: Some("pub fn greet() {}".to_string()),
        }
    }

    fn seed_index(conn: &mut rusqlite::Connection, root: &std::path::Path) -> (FileId, SymbolId) {
        let registry = BackendRegistry::new();
        init_schema(conn, &registry).unwrap();

        let root = root.to_path_buf();

        let mut index = CodeIndex {
            root: root.clone(),
            files: vec![FileSnapshot {
                file_id: None,
                rel_path: PathBuf::from("src/lib.rs"),
                abs_path: root.join("src/lib.rs"),
                language: Language::rust(),
                backend_id: BackendId::new("rust-backend"),
                size_bytes: 100,
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
                    name: "greet".to_string(),
                    qualified_name: "mod::greet".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::rust(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 1,
                        start_col: 1,
                        end_line: 3,
                        end_col: 1,
                    },
                    body_range: None,
                },
                Symbol {
                    id: None,
                    file_id: None,
                    name: "farewell".to_string(),
                    qualified_name: "mod::farewell".to_string(),
                    kind: SymbolKind::Function,
                    language: Language::rust(),
                    file: PathBuf::from("src/lib.rs"),
                    range: TextRange {
                        start_line: 5,
                        start_col: 1,
                        end_line: 7,
                        end_col: 1,
                    },
                    body_range: None,
                },
            ],
            occurrences: vec![],
            call_sites: vec![],
            edges: vec![],
        };
        save_index(conn, &mut index).unwrap();
        let file_id = index.files[0].file_id.unwrap();
        let symbol_id = index.symbols[0].id.unwrap();
        (file_id, symbol_id)
    }

    #[test]
    fn test_save_and_load_chunk_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open_db(dir.path(), &BackendRegistry::new()).unwrap();
        let (file_id, symbol_id) = seed_index(&mut conn, dir.path());

        let chunk = sample_chunk(file_id, symbol_id, 1);
        save_chunks(&conn, &[chunk]).unwrap();

        let loaded = load_chunk(&conn, ChunkId(1)).unwrap().unwrap();
        assert_eq!(loaded.symbol_id, Some(symbol_id));
        assert_eq!(loaded.file_id, file_id);
        assert_eq!(loaded.kind, ChunkKind::SymbolBody);
        assert_eq!(loaded.qualified_name, "mod::greet");
        assert_eq!(loaded.token_count, 12);
        assert!(loaded.text.is_none());
    }

    #[test]
    fn test_load_chunk_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open_db(dir.path(), &BackendRegistry::new()).unwrap();
        seed_index(&mut conn, dir.path());

        let missing = load_chunk(&conn, ChunkId(42)).unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_load_chunks_by_ids_skips_missing() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open_db(dir.path(), &BackendRegistry::new()).unwrap();
        let (file_id, symbol_id) = seed_index(&mut conn, dir.path());

        save_chunks(&conn, &[sample_chunk(file_id, symbol_id, 1)]).unwrap();

        let loaded = load_chunks_by_ids(&conn, &[ChunkId(1), ChunkId(99)]).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, Some(ChunkId(1)));
    }

    #[test]
    fn test_load_chunks_for_symbol_filters_by_symbol() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open_db(dir.path(), &BackendRegistry::new()).unwrap();
        let (file_id, symbol_id) = seed_index(&mut conn, dir.path());

        let other_symbol_id = SymbolId(2);
        save_chunks(
            &conn,
            &[
                sample_chunk(file_id, symbol_id, 1),
                sample_chunk(file_id, other_symbol_id, 2),
            ],
        )
        .unwrap();

        let greet_chunks = load_chunks_for_symbol(&conn, symbol_id).unwrap();
        assert_eq!(greet_chunks.len(), 1);
        assert_eq!(greet_chunks[0].symbol_id, Some(symbol_id));
    }

    #[test]
    fn test_clear_chunks_removes_all_rows() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open_db(dir.path(), &BackendRegistry::new()).unwrap();
        let (file_id, symbol_id) = seed_index(&mut conn, dir.path());
        save_chunks(&conn, &[sample_chunk(file_id, symbol_id, 1)]).unwrap();

        clear_chunks(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_delete_chunks_for_file_keeps_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open_db(dir.path(), &BackendRegistry::new()).unwrap();
        let (file_id, symbol_id) = seed_index(&mut conn, dir.path());

        let other_file_id = FileId(2);
        conn.execute(
            "INSERT INTO files (
                id, path, rel_path, language, backend_id, mtime_ms, size_bytes,
                parser_id, parser_version, parser_config_hash, parse_status
             ) VALUES (2, '/other.rs', 'other.rs', 'rust', 'rust-backend', 100, 50,
                       'tree-sitter-rust', '0.20.0', '', 'Success')",
            [],
        )
        .unwrap();

        save_chunks(
            &conn,
            &[
                sample_chunk(file_id, symbol_id, 1),
                sample_chunk(other_file_id, symbol_id, 2),
            ],
        )
        .unwrap();

        delete_chunks_for_file(&conn, file_id).unwrap();

        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 1);

        let kept = load_chunk(&conn, ChunkId(2)).unwrap().unwrap();
        assert_eq!(kept.file_id, other_file_id);
    }

    #[test]
    fn test_save_chunks_with_parent_and_unknown_kind_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open_db(dir.path(), &BackendRegistry::new()).unwrap();
        let (file_id, symbol_id) = seed_index(&mut conn, dir.path());

        let parent = Chunk {
            id: Some(ChunkId(10)),
            symbol_id: None,
            parent_chunk_id: None,
            file_id,
            kind: ChunkKind::ParentSummary,
            text_hash: "parent".to_string(),
            token_count: 4,
            start_line: 1,
            end_line: 1,
            qualified_name: "mod".to_string(),
            text: None,
        };
        let child = Chunk {
            id: Some(ChunkId(11)),
            symbol_id: Some(symbol_id),
            parent_chunk_id: Some(ChunkId(10)),
            file_id,
            kind: ChunkKind::SymbolDecl,
            text_hash: "child".to_string(),
            token_count: 6,
            start_line: 2,
            end_line: 4,
            qualified_name: "mod::greet".to_string(),
            text: None,
        };
        save_chunks(&conn, &[parent, child]).unwrap();

        let loaded_parent = load_chunk(&conn, ChunkId(10)).unwrap().unwrap();
        assert_eq!(loaded_parent.kind, ChunkKind::ParentSummary);
        let loaded_child = load_chunk(&conn, ChunkId(11)).unwrap().unwrap();
        assert_eq!(loaded_child.parent_chunk_id, Some(ChunkId(10)));
        assert_eq!(loaded_child.kind, ChunkKind::SymbolDecl);

        conn.execute(
            "UPDATE chunks SET kind = 'UnknownKind' WHERE id = 11",
            [],
        )
        .unwrap();
        let unknown_kind = load_chunk(&conn, ChunkId(11)).unwrap().unwrap();
        assert_eq!(unknown_kind.kind, ChunkKind::SymbolBody);
    }

    #[test]
    fn test_load_child_chunks_respects_limit_and_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = open_db(dir.path(), &BackendRegistry::new()).unwrap();
        let (file_id, symbol_id) = seed_index(&mut conn, dir.path());

        let parent = Chunk {
            id: Some(ChunkId(20)),
            symbol_id: None,
            parent_chunk_id: None,
            file_id,
            kind: ChunkKind::ParentSummary,
            text_hash: "parent".to_string(),
            token_count: 4,
            start_line: 1,
            end_line: 1,
            qualified_name: "mod".to_string(),
            text: None,
        };
        let child_a = Chunk {
            id: Some(ChunkId(21)),
            symbol_id: Some(symbol_id),
            parent_chunk_id: Some(ChunkId(20)),
            file_id,
            kind: ChunkKind::SymbolBody,
            text_hash: "child-a".to_string(),
            token_count: 6,
            start_line: 2,
            end_line: 4,
            qualified_name: "mod::child_a".to_string(),
            text: None,
        };
        let child_b = Chunk {
            id: Some(ChunkId(22)),
            symbol_id: Some(SymbolId(symbol_id.0 + 1)),
            parent_chunk_id: Some(ChunkId(20)),
            file_id,
            kind: ChunkKind::SymbolBody,
            text_hash: "child-b".to_string(),
            token_count: 6,
            start_line: 5,
            end_line: 7,
            qualified_name: "mod::child_b".to_string(),
            text: None,
        };
        save_chunks(&conn, &[parent, child_a, child_b]).unwrap();

        let all_children = load_child_chunks(&conn, ChunkId(20), 10).unwrap();
        assert_eq!(all_children.len(), 2);
        assert_eq!(all_children[0].id, Some(ChunkId(21)));

        let limited = load_child_chunks(&conn, ChunkId(20), 1).unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].id, Some(ChunkId(21)));
    }
}

fn row_to_chunk(id: ChunkId, row: &rusqlite::Row<'_>) -> Result<Chunk, rusqlite::Error> {
    row_to_chunk_at(id, row, 0)
}

fn row_to_chunk_at(
    id: ChunkId,
    row: &rusqlite::Row<'_>,
    offset: usize,
) -> Result<Chunk, rusqlite::Error> {
    let symbol_id: Option<i64> = row.get(offset)?;
    let parent_chunk_id: Option<i64> = row.get(offset + 1)?;
    let file_id = FileId(row.get::<_, i64>(offset + 2)?);
    let kind =
        ChunkKind::from_str(&row.get::<_, String>(offset + 3)?).unwrap_or(ChunkKind::SymbolBody);
    Ok(Chunk {
        id: Some(id),
        symbol_id: symbol_id.map(SymbolId),
        parent_chunk_id: parent_chunk_id.map(ChunkId),
        file_id,
        kind,
        text_hash: row.get(offset + 4)?,
        token_count: row.get::<_, i64>(offset + 5)? as usize,
        start_line: row.get::<_, i64>(offset + 6)? as usize,
        end_line: row.get::<_, i64>(offset + 7)? as usize,
        qualified_name: row.get(offset + 8)?,
        text: None,
    })
}