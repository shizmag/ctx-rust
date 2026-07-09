pub mod parser;
pub mod resolver;

#[cfg(test)]
mod parser_tests;

pub use parser::{RustParser, parse_rust_file};
pub use resolver::RustResolver;

use crate::backend::{
    BackendId, BackendMetadata, LanguageBackend, ParserBackend, ResolverBackend, WorkspaceMarker,
};
use crate::index::BuildIndexOptions;
use crate::model::Language;
use std::path::Path;

pub struct RustBackend {
    parser: RustParser,
    resolver: RustResolver,
}

impl RustBackend {
    pub fn new() -> Self {
        Self {
            parser: RustParser,
            resolver: RustResolver::new(),
        }
    }
}

impl LanguageBackend for RustBackend {
    fn id(&self) -> BackendId {
        BackendId("rust-backend".to_string())
    }

    fn language(&self) -> Language {
        Language("rust".to_string())
    }

    fn display_name(&self) -> &'static str {
        "Rust"
    }

    fn matches_path(&self, path: &Path) -> bool {
        path.extension().map(|e| e == "rs").unwrap_or(false)
    }

    fn parser(&self) -> &dyn ParserBackend {
        &self.parser
    }

    fn resolver(&self) -> Option<&dyn ResolverBackend> {
        Some(&self.resolver)
    }

    fn workspace_markers(&self) -> &[WorkspaceMarker] {
        static MARKERS: [WorkspaceMarker; 1] = [WorkspaceMarker::File("Cargo.toml")];
        &MARKERS
    }

    fn metadata(&self, config: &BuildIndexOptions) -> BackendMetadata {
        BackendMetadata {
            backend_id: self.id().0,
            language: self.language().0,
            parser_id: self.parser().parser_id().0,
            parser_version: self.parser().parser_version(),
            resolver_id: self.resolver().map(|r| r.resolver_id().0),
            resolver_version: self.resolver().map(|r| r.resolver_version()),
            config_hash: self.config_fingerprint(config),
        }
    }

    fn config_fingerprint(&self, config: &BuildIndexOptions) -> String {
        format!(
            "use_lsp={},include_tests={}",
            config.use_lsp, config.include_tests
        )
    }
}
