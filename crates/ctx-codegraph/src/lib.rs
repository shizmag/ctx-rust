pub mod backend;
pub mod context;
pub mod discovery;
pub mod error;
pub mod index;
pub mod languages;
pub mod model;
pub mod resolver;
pub mod service;
pub mod slice;
pub mod storage;
pub mod mcp;

pub use context::{
    ApproxTokenEstimator, ContextBudget, ContextCandidate, ContextPack, ContextPackingMode,
    ContextQuery, ContextRanker, ContextSection, ContextSectionKind, ContextSnippet, DepthLimit,
    GraphRanker, HybridRanker, LexicalRanker, OmittedContext, RankingMode, TokenEstimator,
    extract_snippet, is_subsequence, resolve_roots, retrieve_graph_context, tokenize,
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
    load_callees, load_callers, load_edges_for_symbol, load_edges_from, load_edges_to,
    load_file_span, load_index, load_occurrence, load_symbol, load_symbols_by_ids,
    load_symbols_for_file, open_codegraph_db, open_db, rebuild_index_db, resolve_symbol,
    validate_index_db,
};
