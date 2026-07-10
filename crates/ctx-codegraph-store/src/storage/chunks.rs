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
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
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
    }
    tx.commit()?;
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
        row_to_chunk(id, row)
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(CodeGraphError::from)
}

fn row_to_chunk(id: ChunkId, row: &rusqlite::Row<'_>) -> Result<Chunk, rusqlite::Error> {
    let symbol_id: Option<i64> = row.get(0)?;
    let parent_chunk_id: Option<i64> = row.get(1)?;
    let file_id = FileId(row.get::<_, i64>(2)?);
    let kind = ChunkKind::from_str(&row.get::<_, String>(3)?).unwrap_or(ChunkKind::SymbolBody);
    Ok(Chunk {
        id: Some(id),
        symbol_id: symbol_id.map(SymbolId),
        parent_chunk_id: parent_chunk_id.map(ChunkId),
        file_id,
        kind,
        text_hash: row.get(4)?,
        token_count: row.get::<_, i64>(5)? as usize,
        start_line: row.get::<_, i64>(6)? as usize,
        end_line: row.get::<_, i64>(7)? as usize,
        qualified_name: row.get(8)?,
        text: None,
    })
}