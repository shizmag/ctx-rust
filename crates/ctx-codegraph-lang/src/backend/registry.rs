use crate::backend::traits::LanguageBackend;
use crate::model::Language;
use std::path::Path;

pub struct BackendRegistry {
    backends: Vec<Box<dyn LanguageBackend>>,
}

impl Default for BackendRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl BackendRegistry {
    pub fn new() -> Self {
        Self {
            backends: Vec::new(),
        }
    }

    pub fn register(&mut self, backend: Box<dyn LanguageBackend>) {
        self.backends.push(backend);
    }

    pub fn find_by_path(&self, path: &Path) -> Option<&dyn LanguageBackend> {
        self.backends
            .iter()
            .find(|b| b.matches_path(path))
            .map(|b| b.as_ref())
    }

    pub fn find_by_language(&self, lang: &Language) -> Option<&dyn LanguageBackend> {
        self.backends
            .iter()
            .find(|b| b.language() == *lang)
            .map(|b| b.as_ref())
    }

    pub fn all(&self) -> &[Box<dyn LanguageBackend>] {
        &self.backends
    }

    /// Shut down all language-server clients held by registered resolvers.
    pub fn shutdown_lsp_clients(&self) {
        for backend in &self.backends {
            if let Some(resolver) = backend.resolver() {
                resolver.shutdown_lsp();
            }
        }
    }
}