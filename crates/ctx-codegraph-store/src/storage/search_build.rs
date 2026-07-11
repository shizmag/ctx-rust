use ctx_codegraph_chunk::builder::ChunkBuilder;
use ctx_codegraph_chunk::{Chunk, ChunkId, ChunkKind};
use ctx_codegraph_dense::{dense_embedding_count as lance_dense_count, DenseIndex, EmbeddingRecord};
use ctx_codegraph_lang::index::BuildIndexOptions;
use ctx_codegraph_lang::model::{EdgeKind, FileId, SymbolId};
use ctx_codegraph_lang::CodeGraphError;
use ctx_codegraph_lexical::{IndexDoc, LexicalIndex};
use ctx_codegraph_models::{
    batch_ranges, file_fingerprint, EmbeddingExecutionProvider, EmbeddingModel,
};
use ctx_config::Config;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use super::chunks::{clear_chunks, delete_chunks_for_file, save_chunks};
use super::query::load_symbols_for_file;

/// LanceDB rows buffered before a single `merge_insert` flush.
const LANCE_UPSERT_BATCH_SIZE: usize = 256;

#[derive(Debug, Default, Clone)]
pub struct SearchBuildProfile {
    pub chunk_build_ms: u64,
    pub embed_ms: u64,
    pub lance_upsert_ms: u64,
    pub lexical_ms: u64,
    pub embed_batches: usize,
    pub embeddable_chunks: usize,
    pub file_batch_size: usize,
    pub embed_batch_size: usize,
}

#[derive(Debug, Default, Clone)]
pub struct SearchBuildReport {
    pub chunks_written: usize,
    pub embeddings_written: usize,
    pub lexical_docs_written: usize,
    pub profile: Option<SearchBuildProfile>,
}

fn profile_enabled() -> bool {
    std::env::var("CTX_PROFILE_BUILD")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

fn log_profile(profile: &SearchBuildProfile) {
    if !profile_enabled() {
        return;
    }
    eprintln!(
        "Search build profile: chunk_build={}ms embed={}ms ({} batches) lance={}ms lexical={}ms \
         embeddable_chunks={} file_batch={} embed_batch={}",
        profile.chunk_build_ms,
        profile.embed_ms,
        profile.embed_batches,
        profile.lance_upsert_ms,
        profile.lexical_ms,
        profile.embeddable_chunks,
        profile.file_batch_size,
        profile.embed_batch_size,
    );
}

/// Returns the number of rows in the workspace dense embedding index.
pub fn dense_embedding_count(workspace_root: &Path) -> u64 {
    lance_dense_count(workspace_root)
}

fn is_dense_embeddable(chunk: &Chunk) -> bool {
    !matches!(chunk.kind, ChunkKind::Occurrence)
}

fn count_dense_embeddable_chunks(conn: &Connection) -> Result<usize, CodeGraphError> {
    conn.query_row(
        "SELECT COUNT(*) FROM chunks WHERE kind != ?1",
        params![ChunkKind::Occurrence.as_str()],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count.max(0) as usize)
    .map_err(CodeGraphError::from)
}

fn embeddings_need_rebuild(
    conn: &Connection,
    workspace_root: &Path,
    options: &BuildIndexOptions,
) -> Result<bool, CodeGraphError> {
    if options.force_search_rebuild {
        return Ok(true);
    }
    let dense_count = dense_embedding_count(workspace_root);
    if dense_count == 0 {
        return Ok(true);
    }
    let expected = count_dense_embeddable_chunks(conn)?;
    Ok(dense_count < expected as u64)
}

/// Whether search indexes should be built on a ready graph index.
pub fn needs_search_index_build(
    conn: &Connection,
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
    if options.with_lexical == Some(true) {
        return !workspace_root
            .join(".ctx-codegraph/lexical/meta.json")
            .exists();
    }
    if options.builds_lexical(auto)
        && !workspace_root
            .join(".ctx-codegraph/lexical/meta.json")
            .exists()
    {
        return true;
    }
    if options.with_embeddings == Some(true) || options.builds_embeddings(auto) {
        return embeddings_need_rebuild(conn, workspace_root, options).unwrap_or(true);
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
    if !force && !needs_search_index_build(conn, workspace_root, options, config) {
        return SearchBuildReport::default();
    }
    match build_search_indexes_impl(conn, workspace_root, options, config, force) {
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
    build_search_indexes_impl(conn, workspace_root, options, config, false)
}

pub fn build_search_indexes_impl(
    conn: &Connection,
    workspace_root: &Path,
    options: &BuildIndexOptions,
    config: &Config,
    full_rebuild: bool,
) -> Result<SearchBuildReport, CodeGraphError> {
    let auto = config.search_auto_enabled();
    if !options.builds_chunks(auto) {
        return Ok(SearchBuildReport::default());
    }

    let is_rebuild = full_rebuild || options.force_search_rebuild;
    if is_rebuild {
        clear_chunks(conn)?;
        DenseIndex::open(workspace_root)
            .and_then(|mut dense| dense.clear())
            .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
    }

    let file_ids = collect_file_ids(conn)?;
    let file_batch_size = config.effective_build_batch_size();
    let embed_batch_size = config.effective_embed_batch_size();

    let needs_embeddings = options.builds_embeddings(auto);
    let needs_lexical = options.builds_lexical(auto);

    let mut report = SearchBuildReport::default();
    let mut profile = SearchBuildProfile {
        file_batch_size,
        embed_batch_size,
        ..SearchBuildProfile::default()
    };
    let mut lexical_chunks = if needs_lexical {
        Some(Vec::new())
    } else {
        None
    };
    let mut next_chunk_id = 0i64;
    let mut embedding_ctx: Option<EmbeddingBuildContext> = None;
    let mut embed_buffer: Vec<Chunk> = Vec::new();
    let total_files = file_ids.len();
    let mut files_processed = 0usize;

    options.report_progress("Building search chunks...");
    let chunk_build_started = Instant::now();
    let tx = conn.unchecked_transaction()?;
    for file_range in batch_ranges(file_ids.len(), file_batch_size) {
        let file_batch = &file_ids[file_range];
        files_processed += file_batch.len();
        options.report_progress(&format!(
            "Building chunks ({files_processed}/{total_files} files)..."
        ));

        let batch_chunks =
            build_chunks_for_files(&tx, file_batch, options, &mut next_chunk_id)?;
        report.chunks_written += batch_chunks.len();

        if needs_embeddings {
            for chunk in &batch_chunks {
                if is_dense_embeddable(chunk) {
                    profile.embeddable_chunks += 1;
                    embed_buffer.push(chunk.clone());
                }
            }
            if !embed_buffer.is_empty() && embedding_ctx.is_none() {
                options.report_progress("Loading embedding model...");
                embedding_ctx = Some(open_embedding_build_context(
                    workspace_root,
                    options,
                    config,
                    full_rebuild,
                )?);
            }
            if let Some(ctx) = embedding_ctx.as_mut() {
                while embed_buffer.len() >= embed_batch_size {
                    let batch: Vec<Chunk> = embed_buffer.drain(..embed_batch_size).collect();
                    report.embeddings_written +=
                        embed_and_store_chunks(ctx, &batch, embed_batch_size, &mut profile)?;
                    options.report_progress(&format!(
                        "Embedding chunks ({} / {} embedded)...",
                        report.embeddings_written, profile.embeddable_chunks
                    ));
                }
            }
        }

        if let Some(chunks) = lexical_chunks.as_mut() {
            chunks.extend(batch_chunks);
        }
    }
    tx.commit()?;
    profile.chunk_build_ms = chunk_build_started.elapsed().as_millis() as u64;

    if let Some(ctx) = embedding_ctx.as_mut() {
        if !embed_buffer.is_empty() {
            let remainder = std::mem::take(&mut embed_buffer);
            report.embeddings_written +=
                embed_and_store_chunks(ctx, &remainder, embed_batch_size, &mut profile)?;
        }
        options.report_progress(&format!(
            "Flushing embeddings ({} total)...",
            report.embeddings_written
        ));
        ctx.flush_pending(&mut profile)?;
        finalize_embedding_metadata(conn, ctx)?;
    }

    if needs_lexical {
        options.report_progress("Building lexical index...");
        let lexical_started = Instant::now();
        let chunks = lexical_chunks.as_deref().unwrap_or(&[]);
        report.lexical_docs_written = build_lexical_index(conn, workspace_root, chunks)?;
        profile.lexical_ms = lexical_started.elapsed().as_millis() as u64;
    }

    if profile_enabled() {
        report.profile = Some(profile.clone());
        log_profile(&profile);
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
    _options: &BuildIndexOptions,
    next_chunk_id: &mut i64,
) -> Result<Vec<Chunk>, CodeGraphError> {
    let mut all_chunks = Vec::new();

    for (file_id, abs_path) in file_ids {
        delete_chunks_for_file(conn, *file_id)?;
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
    let mut file_paths = std::collections::HashMap::new();
    let mut stmt = conn.prepare_cached("SELECT id, rel_path FROM files")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (id, rel_path) = row?;
        file_paths.insert(FileId(id), rel_path);
    }

    let docs: Vec<IndexDoc> = all_chunks
        .iter()
        .filter_map(|c| {
            let text = c.text.as_ref()?;
            let path = file_paths.get(&c.file_id)?.clone();
            Some(IndexDoc {
                chunk_id: c.id.unwrap(),
                symbol_id: c.symbol_id,
                path,
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
    pending_records: Vec<EmbeddingRecord>,
    lance_flush_size: usize,
    bulk_append: bool,
}

impl EmbeddingBuildContext {
    fn queue_records(&mut self, records: Vec<EmbeddingRecord>) -> Result<(), CodeGraphError> {
        self.pending_records.extend(records);
        Ok(())
    }

    fn flush_pending(&mut self, profile: &mut SearchBuildProfile) -> Result<(), CodeGraphError> {
        while self.pending_records.len() >= self.lance_flush_size {
            let batch: Vec<EmbeddingRecord> = self
                .pending_records
                .drain(..self.lance_flush_size)
                .collect();
            let started = Instant::now();
            if self.bulk_append {
                self.dense
                    .add_batch(&batch)
                    .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
            } else {
                self.dense
                    .upsert_batch(&batch)
                    .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
            }
            profile.lance_upsert_ms += started.elapsed().as_millis() as u64;
        }
        if !self.pending_records.is_empty() {
            let batch = std::mem::take(&mut self.pending_records);
            let started = Instant::now();
            if self.bulk_append {
                self.dense
                    .add_batch(&batch)
                    .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
            } else {
                self.dense
                    .upsert_batch(&batch)
                    .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
            }
            profile.lance_upsert_ms += started.elapsed().as_millis() as u64;
        }
        Ok(())
    }
}

fn open_embedding_build_context(
    workspace_root: &Path,
    options: &BuildIndexOptions,
    config: &Config,
    full_rebuild: bool,
) -> Result<EmbeddingBuildContext, CodeGraphError> {
    let embedding_path = resolve_embedding_model_path(options, config)?;
    let tokenizer_dir = config.resolved_embedding_tokenizer(&embedding_path);
    let provider = EmbeddingExecutionProvider::from_config_str(
        config.effective_embedding_execution_provider(),
    );
    let model = EmbeddingModel::load_with_provider(&embedding_path, &tokenizer_dir, provider)
        .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
    let dense = DenseIndex::open(workspace_root)
        .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
    Ok(EmbeddingBuildContext {
        embedding_path,
        model,
        dense,
        pending_records: Vec::new(),
        lance_flush_size: LANCE_UPSERT_BATCH_SIZE,
        bulk_append: full_rebuild || options.force_search_rebuild,
    })
}

fn embed_and_store_chunks(
    ctx: &mut EmbeddingBuildContext,
    chunks: &[Chunk],
    embed_batch_size: usize,
    profile: &mut SearchBuildProfile,
) -> Result<usize, CodeGraphError> {
    if chunks.is_empty() {
        return Ok(0);
    }

    let embeddable: Vec<&Chunk> = chunks.iter().filter(|c| is_dense_embeddable(c)).collect();
    if embeddable.is_empty() {
        return Ok(0);
    }

    let texts: Vec<String> = embeddable.iter().map(|c| chunk_embedding_text(c)).collect();
    let mut embeddings_written = 0usize;
    for range in batch_ranges(texts.len(), embed_batch_size) {
        let batch_texts = &texts[range.clone()];
        let batch_chunks: Vec<&Chunk> = embeddable[range].iter().copied().collect();
        let started = Instant::now();
        let embeddings = ctx
            .model
            .embed_texts(batch_texts)
            .map_err(|e| CodeGraphError::Parse(e.to_string()))?;
        profile.embed_ms += started.elapsed().as_millis() as u64;
        profile.embed_batches += 1;

        let records: Vec<EmbeddingRecord> = batch_chunks
            .iter()
            .zip(embeddings.iter())
            .map(|(chunk, emb)| EmbeddingRecord {
                chunk_id: chunk.id.unwrap(),
                embedding: emb.clone(),
            })
            .collect();
        embeddings_written += records.len();
        ctx.queue_records(records)?;
        if ctx.pending_records.len() >= ctx.lance_flush_size {
            ctx.flush_pending(profile)?;
        }
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
    let mut stmt = conn.prepare_cached(
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
    let mut stmt = conn.prepare_cached(
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
        let conn = Connection::open_in_memory().unwrap();
        let config = Config::default();
        let options = BuildIndexOptions { extraction_tier: None,
            with_embeddings: Some(true),
            with_lexical: Some(false),
            ..Default::default()
        };
        assert!(needs_search_index_build(&conn, root, &options, &config));
    }

    #[test]
    fn needs_search_index_build_when_dense_index_missing_and_auto_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let conn = Connection::open_in_memory().unwrap();
        let paths = ctx_codegraph_models::ModelPaths::default_paths();
        let config = Config {
            embedding_model: Some(paths.embedding_onnx.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let options = BuildIndexOptions::default();
        assert!(needs_search_index_build(&conn, root, &options, &config));
        assert_eq!(dense_embedding_count(root), 0);
    }

    #[test]
    fn needs_search_index_build_false_when_search_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open_in_memory().unwrap();
        let options = BuildIndexOptions { extraction_tier: None,
            with_embeddings: Some(false),
            with_lexical: Some(false),
            ..Default::default()
        };
        assert!(!needs_search_index_build(
            &conn,
            dir.path(),
            &options,
            &Config::default()
        ));
    }

    #[test]
    fn lance_flush_size_is_bounded() {
        assert_eq!(LANCE_UPSERT_BATCH_SIZE, 256);
        assert!(LANCE_UPSERT_BATCH_SIZE < usize::MAX);
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