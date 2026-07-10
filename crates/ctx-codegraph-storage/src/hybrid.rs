use ctx_codegraph_chunk::ChunkId;
use ctx_codegraph_dense::DenseIndex;
use ctx_codegraph_lang::model::SymbolId;
use ctx_codegraph_lexical::LexicalIndex;
use ctx_codegraph_models::EmbeddingModel;
use ctx_codegraph_search::traits::SearchResult;
use ctx_codegraph_search::{HybridQuery, HybridSearchBackend, SearchError};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct WorkspaceHybridBackend {
    workspace: PathBuf,
    lexical: LexicalIndex,
    dense: Mutex<DenseIndex>,
    embedding: Option<Mutex<EmbeddingModel>>,
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
        })
    }

    pub fn with_embedding(mut self, model: EmbeddingModel) -> Self {
        self.embedding = Some(Mutex::new(model));
        self
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
        Ok(Some(backend))
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