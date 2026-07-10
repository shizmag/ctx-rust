pub mod registry;
pub mod traits;

pub use registry::BackendRegistry;
pub use traits::{
    BackendId, BackendMetadata, LanguageBackend, ParseInput, ParsedFile, ParserBackend, ParserId,
    ResolveInput, ResolveOutput, ResolverBackend, ResolverId, WorkspaceMarker,
};