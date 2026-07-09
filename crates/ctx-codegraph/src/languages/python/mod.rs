pub mod parser;
pub mod resolver;

#[cfg(test)]
mod parser_tests;
#[cfg(test)]
mod resolver_tests;

pub use parser::PythonParser;
pub use resolver::PythonResolver;

use crate::backend::{
    BackendId, BackendMetadata, LanguageBackend, ParserBackend, ResolverBackend, WorkspaceMarker,
};
use crate::index::BuildIndexOptions;
use crate::model::Language;
use std::path::Path;

pub struct PythonBackend {
    parser: PythonParser,
    resolver: PythonResolver,
}

impl PythonBackend {
    pub fn new() -> Self {
        Self {
            parser: PythonParser,
            resolver: PythonResolver::new(),
        }
    }
}

impl LanguageBackend for PythonBackend {
    fn id(&self) -> BackendId {
        BackendId("python-backend".to_string())
    }

    fn language(&self) -> Language {
        Language("python".to_string())
    }

    fn display_name(&self) -> &'static str {
        "Python"
    }

    fn matches_path(&self, path: &Path) -> bool {
        path.extension().map(|e| e == "py").unwrap_or(false)
    }

    fn parser(&self) -> &dyn ParserBackend {
        &self.parser
    }

    fn resolver(&self) -> Option<&dyn ResolverBackend> {
        Some(&self.resolver)
    }

    fn workspace_markers(&self) -> &[WorkspaceMarker] {
        static MARKERS: [WorkspaceMarker; 3] = [
            WorkspaceMarker::File("pyproject.toml"),
            WorkspaceMarker::File("requirements.txt"),
            WorkspaceMarker::File("setup.py"),
        ];
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
