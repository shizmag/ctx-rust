pub mod backend;
pub mod discovery;
pub mod error;
pub mod index;
pub mod model;
pub mod noop;

pub use backend::{
    BackendMetadata, BackendRegistry, LanguageBackend, ParseInput, ParsedFile, ParserBackend,
    ResolveInput, ResolveOutput, ResolverBackend, WorkspaceMarker,
};
pub use error::CodeGraphError;
pub use index::{
    BuildIndexOptions, build_index_with_registry, compute_file_hash, create_file_snapshot,
    create_file_snapshot_with_registry, get_mtime_ms, get_size_bytes,
    should_index_path_with_registry,
};
pub use model::*;
pub use noop::{parse_raw_name, resolve_name_only, resolve_name_only_occurrence};