pub mod traits;

pub use traits::{DenseSearchBackend, EmbeddingIndex, SearchQuery, SearchResult};

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("search index not found: {0}")]
    IndexNotFound(String),

    #[error("search error: {0}")]
    Other(String),
}