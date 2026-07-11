use ctx_codegraph_chunk::builder::ChunkBuilder;
use ctx_codegraph_chunk::{Chunk, ChunkId};
use ctx_codegraph_dense::{DenseIndex, EmbeddingRecord};
use ctx_codegraph_lang::index::BuildIndexOptions;
use ctx_codegraph_lang::model::{EdgeKind, FileId, SymbolId};
use ctx_codegraph_lang::CodeGraphError;
use ctx_codegraph_lexical::{IndexDoc, LexicalIndex};
use ctx_codegraph_models::{batch_ranges, file_fingerprint, EmbeddingModel};
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

/// Returns the number of rows in the workspace dense embedding index.
pub fn dense_embedding_count(workspace_root: &Path) -> u64 {
    let path = workspace_root.join(".ctx-codegraph/dense.sqlite");
    if !path.exists() {
        return 0;
    }
    let Ok(conn) = Connection::open(&path) else {
        return 0;
    };
    conn.query_row("SELECT COUNT(*) FROM chunk_embeddings", [], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        .max(0) as u64
}

/// Whether search indexes should be built on a ready graph index.
pub fn needs_search_index_build(
    workspace_root: &Path,
    options: &BuildIndexOptions,
    config: &Config,
) -> bool {
    let auto = config.search_auto_enabled();
    if !options.builds_chunks(auto) {
        return false;
    }
    if options.force_search_rebuild {
        return true;
    }
    if options.with_embeddings == Some(true) || options.with_lexical == Some(true) {
        return true;
    }
    if options.builds_embeddings(auto) && dense_embedding_count(workspace_root) == 0 {
        return true;
    }
    if options.builds_lexical(auto)
        && !workspace_root
            .join(".ctx-codegraph/lexical/meta.json")
            .exists()
    {
        return true;
    }
    false
}

/// Builds search indexes when requested or missing. Errors are logged and ignored.
pub fn maybe_build_search_indexes(
    conn: &Connection,
    workspace_root: &Path,
    options: &BuildIndexOptions,
    config: &Config,
    force: bool,
) -> SearchBuildReport {
    let auto = config.search_auto_enabled();
    if !options.builds_chunks(auto) {
        return SearchBuildReport::default();
    }
    if !force && !needs_search_index_build(workspace_root, options, config) {
        return SearchBuildReport::default();
    }
    match build_search_indexes(conn, workspace_root, options, config) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("Warning: search index build failed: {}", err);
            SearchBuildReport::default()
        }
    }
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

    if options.force_search_rebuild {
        clear_chunks(conn)?;
        DenseIndex::open(workspace_root)
            .and_then(|mut dense| dense.clear())
            .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
    }

    let file_ids = collect_file_ids(conn)?;
    let file_batch_size = config.effective_build_batch_size();
    let embed_batch_size = file_batch_size;

    let mut report = SearchBuildReport::default();
    let mut all_chunks = Vec::new();
    let mut next_chunk_id = 0i64;
    let mut embedding_ctx: Option<EmbeddingBuildContext> = None;

    // Process files in sequential batches: chunks → embeddings → DB (lexical at end).
    for file_range in batch_ranges(file_ids.len(), file_batch_size) {
        let file_batch = &file_ids[file_range];
        let batch_chunks =
            build_chunks_for_files(conn, file_batch, options, &mut next_chunk_id)?;
        report.chunks_written += batch_chunks.len();

        if options.builds_embeddings(auto)
            && !batch_chunks.is_empty()
            && embedding_ctx.is_none()
        {
            embedding_ctx = Some(open_embedding_build_context(
                workspace_root,
                options,
                config,
            )?);
        }

        if let Some(ctx) = embedding_ctx.as_mut() {
            report.embeddings_written +=
                embed_and_store_chunks(ctx, &batch_chunks, embed_batch_size)?;
        }

        all_chunks.extend(batch_chunks);
    }

    if options.builds_lexical(auto) {
        report.lexical_docs_written =
            build_lexical_index(conn, workspace_root, &all_chunks)?;
    }

    if let Some(ctx) = embedding_ctx.as_mut() {
        finalize_embedding_metadata(conn, ctx)?;
    }

    Ok(report)
}

fn collect_file_ids(conn: &Connection) -> Result<Vec<(FileId, String)>, CodeGraphError> {
    let mut file_ids = Vec::new();
    let mut stmt = conn.prepare("SELECT id, path FROM files")?;
    let rows = stmt.query_map([], |row| {
        Ok((FileId(row.get::<_, i64>(0)?), row.get::<_, String>(1)?))
    })?;
    for row in rows {
        file_ids.push(row?);
    }
    Ok(file_ids)
}

fn build_chunks_for_files(
    conn: &Connection,
    file_ids: &[(FileId, String)],
    options: &BuildIndexOptions,
    next_chunk_id: &mut i64,
) -> Result<Vec<Chunk>, CodeGraphError> {
    let mut all_chunks = Vec::new();

    for (file_id, abs_path) in file_ids {
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
            chunk.id = Some(ChunkId(*next_chunk_id));
            *next_chunk_id += 1;
        }
        save_chunks(conn, &chunks)?;
        all_chunks.extend(chunks);
    }

    Ok(all_chunks)
}

fn build_lexical_index(
    conn: &Connection,
    workspace_root: &Path,
    all_chunks: &[Chunk],
) -> Result<usize, CodeGraphError> {
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
    write_meta(conn, "lexical_index_version", "0.1.0")?;
    Ok(docs.len())
}

fn resolve_embedding_model_path(
    options: &BuildIndexOptions,
    config: &Config,
) -> Result<std::path::PathBuf, CodeGraphError> {
    match config.resolved_embedding_model() {
        Some(path) => Ok(path),
        None if options.with_embeddings == Some(true) => {
            let default = ctx_config::Config::default_embedding_model_path();
            if default.exists() {
                Ok(default)
            } else {
                Err(CodeGraphError::Parse(
                    "embedding model path not configured".into(),
                ))
            }
        }
        None => Err(CodeGraphError::Parse(
            "embedding model path not configured".into(),
        )),
    }
}

fn chunk_embedding_text(chunk: &Chunk) -> String {
    chunk
        .text
        .clone()
        .unwrap_or_else(|| chunk.qualified_name.clone())
}

struct EmbeddingBuildContext {
    embedding_path: std::path::PathBuf,
    model: EmbeddingModel,
    dense: DenseIndex,
}

fn open_embedding_build_context(
    workspace_root: &Path,
    options: &BuildIndexOptions,
    config: &Config,
) -> Result<EmbeddingBuildContext, CodeGraphError> {
    let embedding_path = resolve_embedding_model_path(options, config)?;
    let tokenizer_dir = config.resolved_embedding_tokenizer(&embedding_path);
    let model = EmbeddingModel::load(&embedding_path, &tokenizer_dir)
        .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
    let dense = DenseIndex::open(workspace_root)
        .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
    Ok(EmbeddingBuildContext {
        embedding_path,
        model,
        dense,
    })
}

fn embed_and_store_chunks(
    ctx: &mut EmbeddingBuildContext,
    chunks: &[Chunk],
    embed_batch_size: usize,
) -> Result<usize, CodeGraphError> {
    if chunks.is_empty() {
        return Ok(0);
    }

    let texts: Vec<String> = chunks.iter().map(chunk_embedding_text).collect();
    let mut embeddings_written = 0usize;
    for range in batch_ranges(texts.len(), embed_batch_size) {
        let batch_texts = &texts[range.clone()];
        let batch_chunks = &chunks[range];
        let embeddings = ctx
            .model
            .embed_texts(batch_texts)
            .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
        let records: Vec<EmbeddingRecord> = batch_chunks
            .iter()
            .zip(embeddings.iter())
            .map(|(chunk, emb)| EmbeddingRecord {
                chunk_id: chunk.id.unwrap(),
                embedding: emb.clone(),
            })
            .collect();
        ctx.dense
            .upsert_batch(&records)
            .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
        embeddings_written += records.len();
    }
    Ok(embeddings_written)
}

fn finalize_embedding_metadata(
    conn: &Connection,
    ctx: &EmbeddingBuildContext,
) -> Result<(), CodeGraphError> {
    let fp = file_fingerprint(&ctx.embedding_path)
        .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
    write_meta(conn, "embedding_model_fingerprint", &fp)?;
    write_meta(
        conn,
        "embedding_model_path",
        &ctx.embedding_path.to_string_lossy(),
    )?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::schema::init_schema;
    use ctx_codegraph_lang::backend::BackendRegistry;
    use ctx_codegraph_models::DEFAULT_EMBED_BATCH_SIZE;
    use ctx_codegraph_lang::model::{OccurrenceKind, ResolutionConfidence};
    use std::path::PathBuf;

    fn seed_file_with_graph_data(
        conn: &Connection,
    ) -> (FileId, SymbolId, SymbolId, PathBuf) {
        let registry = BackendRegistry::new();
        init_schema(conn, &registry).unwrap();

        let abs_path = "/tmp/search_build_test/src/lib.rs";
        let rel_path = "src/lib.rs";
        conn.execute(
            "INSERT INTO files (
                path, rel_path, language, backend_id, mtime_ms, size_bytes,
                content_hash, parser_id, parser_version, parser_config_hash,
                indexed_at_ms, parse_status
            ) VALUES (?1, ?2, 'rust', 'rust-backend', 1, 100, NULL, 'p', '1', '', 1, 'success')",
            params![abs_path, rel_path],
        )
        .unwrap();
        let file_id = FileId(conn.last_insert_rowid());

        conn.execute(
            "INSERT INTO symbols (
                file_id, name, qualified_name, kind, language,
                start_line, start_col, end_line, end_col,
                body_start_line, body_start_col, body_end_line, body_end_col
            ) VALUES (?1, 'mod', 'lib', 'Module', 'rust', 1, 1, 10, 1, NULL, NULL, NULL, NULL)",
            params![file_id.0],
        )
        .unwrap();
        let parent_id = SymbolId(conn.last_insert_rowid());

        conn.execute(
            "INSERT INTO symbols (
                file_id, name, qualified_name, kind, language,
                start_line, start_col, end_line, end_col,
                body_start_line, body_start_col, body_end_line, body_end_col
            ) VALUES (?1, 'greet', 'lib::greet', 'Function', 'rust', 2, 1, 4, 1, 2, 1, 4, 1)",
            params![file_id.0],
        )
        .unwrap();
        let child_id = SymbolId(conn.last_insert_rowid());

        conn.execute(
            "INSERT INTO edges (
                kind, from_file_id, from_symbol_id, to_symbol_id, confidence
            ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                EdgeKind::Contains.as_str(),
                file_id.0,
                parent_id.0,
                child_id.0,
                ResolutionConfidence::LspExact.as_str(),
            ],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO occurrences (
                file_id, enclosing_symbol_id, kind, raw_text,
                start_line, start_col, end_line, end_col, language, backend_id
            ) VALUES (?1, ?2, ?3, ?4, 3, 5, 3, 11, 'rust', 'rust-backend')",
            params![file_id.0, child_id.0, OccurrenceKind::Call.as_str(), "helper()"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO occurrences (
                file_id, enclosing_symbol_id, kind, raw_text,
                start_line, start_col, end_line, end_col, language, backend_id
            ) VALUES (?1, NULL, ?2, ?3, 1, 1, 1, 4, 'rust', 'rust-backend')",
            params![
                file_id.0,
                "NotARealKind",
                "mod",
            ],
        )
        .unwrap();

        (file_id, parent_id, child_id, PathBuf::from(abs_path))
    }

    #[test]
    fn load_contains_parents_maps_child_to_parent() {
        let conn = Connection::open_in_memory().unwrap();
        let (file_id, parent_id, child_id, _) = seed_file_with_graph_data(&conn);

        let parents = load_contains_parents(&conn, file_id).unwrap();
        assert_eq!(parents.len(), 1);
        assert_eq!(parents.get(&child_id), Some(&parent_id));
    }

    #[test]
    fn load_contains_parents_returns_empty_for_unknown_file() {
        let conn = Connection::open_in_memory().unwrap();
        let registry = BackendRegistry::new();
        init_schema(&conn, &registry).unwrap();

        let parents = load_contains_parents(&conn, FileId(999)).unwrap();
        assert!(parents.is_empty());
    }

    #[test]
    fn load_occurrences_for_file_loads_rows_and_defaults_unknown_kind() {
        let conn = Connection::open_in_memory().unwrap();
        let (file_id, _parent_id, child_id, path) = seed_file_with_graph_data(&conn);

        let occurrences = load_occurrences_for_file(&conn, file_id, &path).unwrap();
        assert_eq!(occurrences.len(), 2);

        let call = occurrences
            .iter()
            .find(|o| o.raw_text == "helper()")
            .expect("call occurrence");
        assert_eq!(call.kind, OccurrenceKind::Call);
        assert_eq!(call.enclosing_symbol, Some(child_id));
        assert_eq!(call.file, path);
        assert_eq!(call.range.start_line, 3);

        let unknown = occurrences
            .iter()
            .find(|o| o.raw_text == "mod")
            .expect("unknown-kind occurrence");
        assert_eq!(unknown.kind, OccurrenceKind::Reference);
        assert!(unknown.enclosing_symbol.is_none());
    }

    #[test]
    fn file_path_for_chunk_returns_rel_path() {
        let conn = Connection::open_in_memory().unwrap();
        let (file_id, _, _, _) = seed_file_with_graph_data(&conn);

        let rel_path = file_path_for_chunk(&conn, file_id).unwrap();
        assert_eq!(rel_path, "src/lib.rs");
    }

    #[test]
    fn needs_search_index_build_when_embeddings_explicitly_requested() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let config = Config::default();
        let options = BuildIndexOptions {
            with_embeddings: Some(true),
            with_lexical: Some(false),
            ..Default::default()
        };
        assert!(needs_search_index_build(root, &options, &config));
    }

    #[test]
    fn needs_search_index_build_when_dense_index_missing_and_auto_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let paths = ctx_codegraph_models::ModelPaths::default_paths();
        let config = Config {
            embedding_model: Some(paths.embedding_onnx.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let options = BuildIndexOptions::default();
        assert!(needs_search_index_build(root, &options, &config));
        assert_eq!(dense_embedding_count(root), 0);
    }

    #[test]
    fn needs_search_index_build_false_when_search_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let options = BuildIndexOptions {
            with_embeddings: Some(false),
            with_lexical: Some(false),
            ..Default::default()
        };
        assert!(!needs_search_index_build(
            dir.path(),
            &options,
            &Config::default()
        ));
    }

    #[test]
    fn dense_embedding_batch_plan_covers_all_chunks() {
        let chunk_count = DEFAULT_EMBED_BATCH_SIZE * 2 + 5;
        let ranges: Vec<_> = batch_ranges(chunk_count, DEFAULT_EMBED_BATCH_SIZE).collect();

        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges.first().map(|r| r.start), Some(0));
        assert_eq!(ranges.last().map(|r| r.end), Some(chunk_count));
        let covered: usize = ranges.iter().map(|r| r.end - r.start).sum();
        assert_eq!(covered, chunk_count);
    }

    #[test]
    fn file_batch_plan_covers_all_files() {
        let file_count = 10;
        let batch_size = 3;
        let ranges: Vec<_> = batch_ranges(file_count, batch_size).collect();
        assert_eq!(ranges, vec![0..3, 3..6, 6..9, 9..10]);
        let covered: usize = ranges.iter().map(|r| r.end - r.start).sum();
        assert_eq!(covered, file_count);
    }

    #[test]
    fn write_meta_persists_key_value() {
        let conn = Connection::open_in_memory().unwrap();
        let registry = BackendRegistry::new();
        init_schema(&conn, &registry).unwrap();

        write_meta(&conn, "test_key", "test_value").unwrap();

        let value: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                params!["test_key"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, "test_value");

        write_meta(&conn, "test_key", "updated").unwrap();
        let updated: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                params!["test_key"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(updated, "updated");
    }
}