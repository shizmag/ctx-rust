use crate::backend::traits::LanguageBackend;
use crate::model::Language;
use std::path::Path;
use std::sync::OnceLock;

pub struct BackendRegistry {
    backends: Vec<Box<dyn LanguageBackend>>,
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
}

pub fn global_registry() -> &'static BackendRegistry {
    static REGISTRY: OnceLock<BackendRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut reg = BackendRegistry::new();
        reg.register(Box::new(crate::languages::rust::RustBackend::new()));
        reg.register(Box::new(crate::languages::python::PythonBackend::new()));
        reg.register(Box::new(crate::languages::MockBackend::new()));
        reg
    })
}
