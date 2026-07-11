pub mod context;
pub mod hybrid_service;
pub mod slice;

// Reexports from extracted ctx-codegraph-storage crate (to preserve public API and
// intra-crate `crate::xxx` references in remaining modules like context/).
pub use ctx_codegraph_storage::backend;
pub use ctx_codegraph_storage::discovery;
pub use ctx_codegraph_storage::error;
pub use ctx_codegraph_storage::index;
pub use ctx_codegraph_storage::languages;
pub use ctx_codegraph_storage::model;
pub use ctx_codegraph_storage::resolver;
pub use ctx_codegraph_storage::service;
pub use ctx_codegraph_storage::storage;

pub use context::{
    ApproxTokenEstimator, ContextBudget, ContextCandidate, ContextPack, ContextPackingMode,
    ContextQuery, ContextRanker, ContextRetrievalOptions, ContextSection, ContextSectionKind,
    ContextSnippet, DepthLimit, GraphRanker, HybridRanker, LexicalRanker, OmittedContext,
    RankingMode, TokenEstimator, extract_snippet, is_subsequence, resolve_roots,
    retrieve_context_with_options, retrieve_graph_context, retrieve_graph_context_with_options,
    tokenize, HybridRetrievalOptions, RetrievalStrategy,
};

pub use backend::{
    BackendId, BackendMetadata, LanguageBackend, ParserBackend, ParserId, ResolverBackend,
    ResolverId, WorkspaceMarker, global_registry,
};
pub use error::CodeGraphError;
pub use index::{BuildIndexOptions, BuildProgressHook, build_index};
pub use model::*;
pub use ctx_codegraph_storage::hybrid::WorkspaceHybridBackend;
pub use hybrid_service::retrieve_context_for_service;
pub use service::GraphContextService;
pub use slice::{SliceOptions, forward_slice, reverse_slice};
pub use storage::{
    check_db_compatibility, compute_index_diff, dense_embedding_count, ensure_index, find_symbols,
    find_workspace_root, get_index_state, load_callees, load_callers, load_edges_for_symbol,
    load_edges_from, load_edges_to, load_file_span, load_index, load_occurrence, load_symbol,
    load_symbols_by_ids, load_symbols_for_file, open_codegraph_db, open_db, rebuild_index_db,
    resolve_symbol, validate_index_db,
};
