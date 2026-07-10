use crate::rrf::{ChunkHit, rrf_fuse};
use crate::traits::SearchResult;
use crate::SearchError;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct HybridQuery<'a> {
    pub workspace_root: &'a Path,
    pub text: &'a str,
    pub limit: usize,
}

#[derive(Debug, Clone)]
pub struct HybridSearchOptions {
    pub rrf_k: usize,
    pub lexical_top_k: usize,
    pub dense_top_k: usize,
}

impl Default for HybridSearchOptions {
    fn default() -> Self {
        Self {
            rrf_k: 60,
            lexical_top_k: 50,
            dense_top_k: 50,
        }
    }
}

pub trait HybridSearchBackend: Send + Sync {
    fn search_lexical(&self, query: HybridQuery<'_>) -> Result<Vec<SearchResult>, SearchError>;
    fn search_dense(&self, query: HybridQuery<'_>) -> Result<Vec<SearchResult>, SearchError>;
}

pub struct HybridSearcher<B: HybridSearchBackend> {
    backend: B,
    options: HybridSearchOptions,
}

impl<B: HybridSearchBackend> HybridSearcher<B> {
    pub fn new(backend: B, options: HybridSearchOptions) -> Self {
        Self { backend, options }
    }

    pub fn search(&self, query: HybridQuery<'_>) -> Result<Vec<SearchResult>, SearchError> {
        let lexical_query = HybridQuery {
            workspace_root: query.workspace_root,
            text: query.text,
            limit: self.options.lexical_top_k,
        };
        let dense_query = HybridQuery {
            workspace_root: query.workspace_root,
            text: query.text,
            limit: self.options.dense_top_k,
        };

        let lexical = self.backend.search_lexical(lexical_query)?;
        let dense = self.backend.search_dense(dense_query)?;

        let lists = [
            hits_from_results(&lexical),
            hits_from_results(&dense),
        ];
        let fused = rrf_fuse(&lists, self.options.rrf_k);

        let limit = query.limit.max(1);
        Ok(fused
            .into_iter()
            .take(limit)
            .map(|hit| SearchResult {
                chunk_id: hit.chunk_id,
                symbol_id: hit.symbol_id.unwrap_or(ctx_codegraph_lang::model::SymbolId(0)),
                score: hit.score,
                snippet: None,
            })
            .collect())
    }
}

fn hits_from_results(results: &[SearchResult]) -> Vec<ChunkHit> {
    results
        .iter()
        .map(|r| ChunkHit {
            chunk_id: r.chunk_id,
            symbol_id: Some(r.symbol_id),
            score: r.score,
        })
        .collect()
}