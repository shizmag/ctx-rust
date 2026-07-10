use crate::SearchError;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct SearchQuery<'a> {
    pub workspace_root: &'a Path,
    pub text: &'a str,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub symbol_id: ctx_codegraph_lang::model::SymbolId,
    pub score: f32,
    pub snippet: Option<String>,
}

pub trait EmbeddingIndex: Send + Sync {
    fn dimension(&self) -> usize;
    fn upsert(&mut self, id: ctx_codegraph_lang::model::SymbolId, embedding: &[f32]) -> Result<(), SearchError>;
    fn remove(&mut self, id: ctx_codegraph_lang::model::SymbolId) -> Result<(), SearchError>;
}

pub trait DenseSearchBackend: Send + Sync {
    fn search(&self, query: SearchQuery<'_>) -> Result<Vec<SearchResult>, SearchError>;
}