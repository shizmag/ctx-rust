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
            .map(fused_hit_to_result)
            .collect())
    }
}

fn fused_hit_to_result(hit: ChunkHit) -> SearchResult {
    SearchResult {
        chunk_id: hit.chunk_id,
        symbol_id: hit
            .symbol_id
            .unwrap_or(ctx_codegraph_lang::model::SymbolId(0)),
        score: hit.score,
        snippet: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_codegraph_chunk::ChunkId;
    use ctx_codegraph_lang::model::SymbolId;

    #[test]
    fn fused_hit_to_result_defaults_missing_symbol_id_to_zero() {
        let hit = ChunkHit {
            chunk_id: ChunkId(9),
            symbol_id: None,
            score: 0.25,
        };
        let result = fused_hit_to_result(hit);
        assert_eq!(result.chunk_id, ChunkId(9));
        assert_eq!(result.symbol_id, SymbolId(0));
        assert_eq!(result.score, 0.25);
        assert_eq!(result.snippet, None);
    }

    #[test]
    fn hits_from_results_wraps_symbol_ids() {
        let results = vec![SearchResult {
            chunk_id: ChunkId(3),
            symbol_id: SymbolId(30),
            score: 0.75,
            snippet: Some("snippet".to_string()),
        }];
        let hits = hits_from_results(&results);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].symbol_id, Some(SymbolId(30)));
        assert_eq!(hits[0].chunk_id, ChunkId(3));
    }
}