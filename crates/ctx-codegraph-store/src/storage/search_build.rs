use ctx_codegraph_chunk::builder::ChunkBuilder;
use ctx_codegraph_chunk::{Chunk, ChunkId};
use ctx_codegraph_dense::{DenseIndex, EmbeddingRecord};
use ctx_codegraph_lang::index::BuildIndexOptions;
use ctx_codegraph_lang::model::{EdgeKind, FileId, SymbolId};
use ctx_codegraph_lang::CodeGraphError;
use ctx_codegraph_lexical::{IndexDoc, LexicalIndex};
use ctx_codegraph_models::{file_fingerprint, EmbeddingModel, ModelPaths};
use ctx_config::Config;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;

use super::chunks::{clear_chunks, delete_chunks_for_file, save_chunks};
use super::query::load_symbols_for_file;

#[derive(Debug, Default, Clone)]
pub struct SearchBuildReport {
    pub chunks_written: usize,
    pub embeddings_written: usize,
    pub lexical_docs_written: usize,
}

pub fn build_search_indexes(
    conn: &Connection,
    workspace_root: &Path,
    options: &BuildIndexOptions,
    config: &Config,
) -> Result<SearchBuildReport, CodeGraphError> {
    let auto = config.search_auto_enabled();
    if !options.builds_chunks(auto) {
        return Ok(SearchBuildReport::default());
    }

    let mut report = SearchBuildReport::default();
    if options.force_search_rebuild {
        clear_chunks(conn)?;
    }

    let mut all_chunks: Vec<Chunk> = Vec::new();
    let mut next_chunk_id = 0i64;

    let mut file_ids = Vec::new();
    {
        let mut stmt = conn.prepare("SELECT id, path FROM files")?;
        let rows = stmt.query_map([], |row| {
            Ok((FileId(row.get::<_, i64>(0)?), row.get::<_, String>(1)?))
        })?;
        for row in rows {
            file_ids.push(row?);
        }
    }

    for (file_id, abs_path) in &file_ids {
        if options.force_search_rebuild {
            delete_chunks_for_file(conn, *file_id)?;
        }
        let path = Path::new(abs_path);
        let symbols = load_symbols_for_file(conn, path)?;
        let contains_parent = load_contains_parents(conn, *file_id)?;
        let occurrences = load_occurrences_for_file(conn, *file_id, path)?;
        let mut builder = ChunkBuilder::new(*file_id, path)
            .include_text(true)
            .context_lines(2);
        let mut chunks = builder
            .build(&symbols, &contains_parent, &occurrences)
            .map_err(CodeGraphError::Io)?;
        for chunk in &mut chunks {
            chunk.id = Some(ChunkId(next_chunk_id));
            next_chunk_id += 1;
        }
        save_chunks(conn, &chunks)?;
        all_chunks.extend(chunks);
    }
    report.chunks_written = all_chunks.len();

    if options.builds_lexical(auto) {
        let docs: Vec<IndexDoc> = all_chunks
            .iter()
            .filter_map(|c| {
                let text = c.text.as_ref()?;
                Some(IndexDoc {
                    chunk_id: c.id.unwrap(),
                    symbol_id: c.symbol_id,
                    path: file_path_for_chunk(conn, c.file_id).ok()?,
                    qualified_name: c.qualified_name.clone(),
                    text: text.clone(),
                })
            })
            .collect();
        let mut lexical = LexicalIndex::open(workspace_root)
            .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
        lexical
            .build(&docs)
            .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
        report.lexical_docs_written = docs.len();
        write_meta(conn, "lexical_index_version", "0.1.0")?;
    }

    if options.builds_embeddings(auto) {
        let embedding_path = config
            .resolved_embedding_model()
            .ok_or_else(|| CodeGraphError::Parse("embedding model path not configured".into()))?;
        let tokenizer_dir = config.resolved_embedding_tokenizer(&embedding_path);
        let mut model = EmbeddingModel::load(&embedding_path, &tokenizer_dir)
            .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
        let texts: Vec<String> = all_chunks
            .iter()
            .map(|c| {
                c.text
                    .clone()
                    .unwrap_or_else(|| c.qualified_name.clone())
            })
            .collect();
        let embeddings = model
            .embed_texts(&texts)
            .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
        let records: Vec<EmbeddingRecord> = all_chunks
            .iter()
            .zip(embeddings.iter())
            .map(|(chunk, emb)| EmbeddingRecord {
                chunk_id: chunk.id.unwrap(),
                embedding: emb.clone(),
            })
            .collect();
        let mut dense = DenseIndex::open(workspace_root)
            .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
        dense
            .upsert_batch(&records)
            .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
        report.embeddings_written = records.len();
        let fp = file_fingerprint(&embedding_path)
            .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
        write_meta(conn, "embedding_model_fingerprint", &fp)?;
        write_meta(
            conn,
            "embedding_model_path",
            &embedding_path.to_string_lossy(),
        )?;
    }

    Ok(report)
}

fn write_meta(conn: &Connection, key: &str, value: &str) -> Result<(), CodeGraphError> {
    conn.execute(
        "INSERT OR REPLACE INTO metadata (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}

fn file_path_for_chunk(conn: &Connection, file_id: FileId) -> Result<String, CodeGraphError> {
    conn.query_row(
        "SELECT rel_path FROM files WHERE id = ?1",
        params![file_id.0],
        |row| row.get::<_, String>(0),
    )
    .map_err(CodeGraphError::from)
}

fn load_contains_parents(
    conn: &Connection,
    file_id: FileId,
) -> Result<HashMap<SymbolId, SymbolId>, CodeGraphError> {
    let mut stmt = conn.prepare(
        "SELECT to_symbol_id, from_symbol_id FROM edges
         WHERE kind = ?1 AND from_file_id = ?2 AND to_symbol_id IS NOT NULL AND from_symbol_id IS NOT NULL",
    )?;
    let rows = stmt.query_map(params![EdgeKind::Contains.as_str(), file_id.0], |row| {
        Ok((SymbolId(row.get(0)?), SymbolId(row.get(1)?)))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (child, parent) = row?;
        map.insert(child, parent);
    }
    Ok(map)
}

fn load_occurrences_for_file(
    conn: &Connection,
    file_id: FileId,
    path: &Path,
) -> Result<Vec<ctx_codegraph_lang::model::Occurrence>, CodeGraphError> {
    let mut stmt = conn.prepare(
        "SELECT id, enclosing_symbol_id, kind, raw_text, start_line, start_col, end_line, end_col, language, backend_id
         FROM occurrences WHERE file_id = ?1",
    )?;
    let rows = stmt.query_map(params![file_id.0], |row| {
        let id = row.get::<_, i64>(0)?;
        let enclosing: Option<i64> = row.get(1)?;
        let kind_str: String = row.get(2)?;
        Ok(ctx_codegraph_lang::model::Occurrence {
            id: Some(ctx_codegraph_lang::model::OccurrenceId(id)),
            file_id: Some(file_id),
            enclosing_symbol: enclosing.map(SymbolId),
            enclosing_temp_index: None,
            kind: ctx_codegraph_lang::model::OccurrenceKind::from_str(&kind_str)
                .unwrap_or(ctx_codegraph_lang::model::OccurrenceKind::Reference),
            raw_text: row.get(3)?,
            range: ctx_codegraph_lang::model::TextRange {
                start_line: row.get::<_, i64>(4)? as usize,
                start_col: row.get::<_, i64>(5)? as usize,
                end_line: row.get::<_, i64>(6)? as usize,
                end_col: row.get::<_, i64>(7)? as usize,
            },
            file: path.to_path_buf(),
            language: row.get(8)?,
            backend_id: row.get(9)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(CodeGraphError::from)
}