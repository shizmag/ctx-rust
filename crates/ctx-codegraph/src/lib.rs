pub mod backend;
pub mod error;
pub mod index;
pub mod languages;
pub mod model;
pub mod resolver;
pub mod service;
pub mod slice;
pub mod storage;

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
    rebuild_index_db, resolve_symbol, validate_index_db,
};
