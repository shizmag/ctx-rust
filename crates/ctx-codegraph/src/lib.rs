pub mod backend;
pub mod error;
pub mod index;
pub mod languages;
pub mod model;
pub mod resolver;
pub mod service;
pub mod slice;
pub mod storage;
pub mod context;

pub use context::{
    DepthLimit, RankingMode, ContextPackingMode, ContextCandidate, OmittedContext,
    ContextSnippet, ContextSectionKind, ContextSection, ContextPack, ContextBudget,
    TokenEstimator, ApproxTokenEstimator, ContextRanker, ContextQuery, GraphRanker,
    LexicalRanker, HybridRanker, tokenize, is_subsequence, extract_snippet,
    resolve_roots, retrieve_graph_context,
};

pub use backend::{
    BackendMetadata, LanguageBackend, ParserBackend, ResolverBackend, WorkspaceMarker,
    global_registry,
};
pub use error::CodeGraphError;
pub use index::{BuildIndexOptions, build_index};
pub use model::*;
pub use service::GraphContextService;
pub use slice::{SliceOptions, forward_slice, reverse_slice};
pub use storage::{
    check_db_compatibility, compute_index_diff, find_symbols, find_workspace_root, get_index_state,
    load_callees, load_callers, load_index, load_symbols_for_file, open_codegraph_db, open_db,
    rebuild_index_db, resolve_symbol, validate_index_db, load_edges_from, load_edges_to,
    load_edges_for_symbol, load_symbol, load_symbols_by_ids, load_occurrence, load_file_span,
};
