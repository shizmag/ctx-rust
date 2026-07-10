pub mod hybrid;
pub mod rrf;
pub mod traits;

pub use hybrid::{
    HybridQuery, HybridSearchBackend, HybridSearchOptions, HybridSearcher,
};
pub use rrf::{ChunkHit, rrf_fuse};
pub use traits::{DenseSearchBackend, EmbeddingIndex, SearchQuery, SearchResult};

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("search index not found: {0}")]
    IndexNotFound(String),

    #[error("search error: {0}")]
    Other(String),
}