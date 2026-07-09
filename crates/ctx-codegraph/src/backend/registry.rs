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

fn production_registry() -> BackendRegistry {
    let mut reg = BackendRegistry::new();
    reg.register(Box::new(crate::languages::rust::RustBackend::new()));
    reg.register(Box::new(crate::languages::python::PythonBackend::new()));
    reg
}

pub fn global_registry() -> &'static BackendRegistry {
    static REGISTRY: OnceLock<BackendRegistry> = OnceLock::new();
    REGISTRY.get_or_init(production_registry)
}

/// Registry that includes the mock backend for tests.
#[cfg(test)]
pub fn test_registry_with_mock() -> BackendRegistry {
    let mut reg = production_registry();
    reg.register(Box::new(crate::languages::MockBackend::new()));
    reg
}

#[cfg(test)]
mod tests {
    use super::test_registry_with_mock;
    use crate::index::BuildIndexOptions;
    use crate::model::FileChangeDetection;
    use crate::storage::rebuild_index_db_with_registry;

    #[test]
    fn test_generic_pipeline_with_mock_backend() {
        let temp_dir = tempfile::tempdir().unwrap();
        let proj_dir = temp_dir.path().to_path_buf();

        std::fs::write(proj_dir.join("mock.project"), "mock project content").unwrap();

        let mock_code = "
        fn foo()
        fn bar()
    ";
        std::fs::write(proj_dir.join("test_file.mock"), mock_code).unwrap();

        let options = BuildIndexOptions {
            use_lsp: false,
            max_depth: None,
            include_tests: true,
            change_detection: FileChangeDetection::MtimeAndSize,
        };

        let registry = test_registry_with_mock();
        let (index, report) =
            rebuild_index_db_with_registry(&proj_dir, options, &registry).unwrap();

        assert!(report.full_rebuild);
        assert_eq!(index.files.len(), 1);
        assert_eq!(
            index.files[0].abs_path.file_name().unwrap(),
            "test_file.mock"
        );
        assert_eq!(index.files[0].language.as_str(), "mock");

        assert_eq!(index.symbols.len(), 2);
        let sym_names: std::collections::HashSet<String> =
            index.symbols.iter().map(|s| s.name.clone()).collect();
        assert!(sym_names.contains("foo"));
        assert!(sym_names.contains("bar"));
    }
}