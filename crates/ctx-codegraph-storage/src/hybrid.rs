use ctx_codegraph_chunk::ChunkId;
use ctx_codegraph_dense::DenseIndex;
use ctx_codegraph_lang::model::SymbolId;
use ctx_codegraph_lexical::LexicalIndex;
use ctx_codegraph_models::{EmbeddingModel, RerankerModel};
use ctx_codegraph_search::traits::SearchResult;
use ctx_codegraph_search::{HybridQuery, HybridSearchBackend, SearchError};
use ctx_codegraph_store::storage::{load_chunk, load_symbol};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct WorkspaceHybridBackend {
    workspace: PathBuf,
    lexical: LexicalIndex,
    dense: Mutex<DenseIndex>,
    embedding: Option<Mutex<EmbeddingModel>>,
    reranker: Option<Mutex<RerankerModel>>,
}

impl WorkspaceHybridBackend {
    pub fn open(workspace: &Path) -> Result<Self, SearchError> {
        let lexical = LexicalIndex::open(workspace).map_err(|e| SearchError::Other(e.to_string()))?;
        let dense = DenseIndex::open(workspace).map_err(|e| SearchError::Other(e.to_string()))?;
        Ok(Self {
            workspace: workspace.to_path_buf(),
            lexical,
            dense: Mutex::new(dense),
            embedding: None,
            reranker: None,
        })
    }

    pub fn with_embedding(mut self, model: EmbeddingModel) -> Self {
        self.embedding = Some(Mutex::new(model));
        self
    }

    pub fn with_reranker(mut self, model: RerankerModel) -> Self {
        self.reranker = Some(Mutex::new(model));
        self
    }

    pub fn has_reranker(&self) -> bool {
        self.reranker.is_some()
    }

    pub fn try_with_config(
        workspace: &Path,
        config: &ctx_config::Config,
    ) -> Result<Option<Self>, SearchError> {
        if !config.search_auto_enabled() {
            return Ok(None);
        }
        let mut backend = Self::open(workspace)?;
        if let Some(path) = config.resolved_embedding_model() {
            let tokenizer_dir = config.resolved_embedding_tokenizer(&path);
            if let Ok(model) = EmbeddingModel::load(&path, &tokenizer_dir) {
                backend = backend.with_embedding(model);
            }
        }
        if config.enable_rerank.unwrap_or(false) {
            if let Some(path) = config.resolved_reranker_model() {
                let tokenizer_dir = config.resolved_rerank_tokenizer(&path);
                if let Ok(model) = RerankerModel::load(&path, &tokenizer_dir) {
                    backend = backend.with_reranker(model);
                }
            }
        }
        Ok(Some(backend))
    }

    pub fn rerank_results(
        &self,
        conn: &rusqlite::Connection,
        query: &str,
        results: &mut Vec<SearchResult>,
        top_k: usize,
    ) -> Result<(), SearchError> {
        let Some(reranker) = self.reranker.as_ref() else {
            return Ok(());
        };
        if results.is_empty() {
            return Ok(());
        }

        let take = top_k.min(results.len());
        let docs: Vec<String> = results[..take]
            .iter()
            .map(|r| chunk_doc_for_rerank(conn, r.chunk_id))
            .collect::<Result<_, _>>()
            .map_err(|e| SearchError::Other(e.to_string()))?;

        let mut model = reranker
            .lock()
            .map_err(|e| SearchError::Other(e.to_string()))?;
        let scores = model
            .score_pairs(query, &docs)
            .map_err(|e| SearchError::Other(e.to_string()))?;

        apply_rerank_scores(&mut results[..take], &scores);
        results[..take].sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(())
    }
}

fn chunk_doc_for_rerank(
    conn: &rusqlite::Connection,
    chunk_id: ChunkId,
) -> Result<String, ctx_codegraph_lang::CodeGraphError> {
    let Some(chunk) = load_chunk(conn, chunk_id)? else {
        return Ok(String::new());
    };
    if let Some(symbol_id) = chunk.symbol_id {
        let sym = load_symbol(conn, symbol_id)?;
        Ok(format!("{} {}", sym.qualified_name, sym.name))
    } else {
        Ok(chunk.qualified_name)
    }
}

pub fn apply_rerank_scores(results: &mut [SearchResult], scores: &[f32]) {
    for (result, score) in results.iter_mut().zip(scores.iter()) {
        result.score = *score;
    }
}

impl HybridSearchBackend for &WorkspaceHybridBackend {
    fn search_lexical(&self, query: HybridQuery<'_>) -> Result<Vec<SearchResult>, SearchError> {
        let hits = self
            .lexical
            .search(query.text, query.limit)
            .map_err(|e| SearchError::Other(e.to_string()))?;
        Ok(hits
            .into_iter()
            .map(|h| SearchResult {
                chunk_id: h.chunk_id,
                symbol_id: h.symbol_id.unwrap_or(SymbolId(0)),
                score: h.score,
                snippet: None,
            })
            .collect())
    }

    fn search_dense(&self, query: HybridQuery<'_>) -> Result<Vec<SearchResult>, SearchError> {
        let Some(model) = self.embedding.as_ref() else {
            return Ok(Vec::new());
        };
        let mut model = model.lock().map_err(|e| SearchError::Other(e.to_string()))?;
        let vectors = model
            .embed_texts(&[query.text.to_string()])
            .map_err(|e| SearchError::Other(e.to_string()))?;
        let query_vec = vectors
            .into_iter()
            .next()
            .ok_or_else(|| SearchError::Other("empty embedding".into()))?;
        let dense = self.dense.lock().map_err(|e| SearchError::Other(e.to_string()))?;
        let hits = dense
            .search_knn(&query_vec, query.limit)
            .map_err(|e| SearchError::Other(e.to_string()))?;
        Ok(hits
            .into_iter()
            .map(|h| SearchResult {
                chunk_id: h.chunk_id,
                symbol_id: SymbolId(0),
                score: h.score,
                snippet: None,
            })
            .collect())
    }
}

pub fn chunk_ids_from_results(results: &[SearchResult]) -> Vec<ChunkId> {
    results.iter().map(|r| r.chunk_id).collect()
}