pub mod registry;
pub mod traits;

pub use registry::{BackendRegistry, global_registry};
#[cfg(test)]
pub use registry::test_registry_with_mock;
pub use traits::{
    BackendId, BackendMetadata, LanguageBackend, ParseInput, ParsedFile, ParserBackend, ParserId,
    ResolveInput, ResolveOutput, ResolverBackend, ResolverId, WorkspaceMarker,
};
