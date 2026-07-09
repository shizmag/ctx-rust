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
    use super::{BackendRegistry, test_registry_with_mock};
    use crate::backend::ParseInput;
    use crate::index::BuildIndexOptions;
    use crate::languages::MockBackend;
    use crate::model::{FileChangeDetection, Language, LanguageId};
    use crate::storage::rebuild_index_db_with_registry;
    use std::path::Path;

    #[test]
    fn test_backend_registry_new_and_lookup() {
        let mut reg = BackendRegistry::new();
        assert!(reg.all().is_empty());

        reg.register(Box::new(MockBackend::new()));
        assert_eq!(reg.all().len(), 1);

        let mock_path = Path::new("src/example.mock");
        let backend = reg.find_by_path(mock_path).unwrap();
        assert_eq!(backend.id().0, "mock-backend");
        assert_eq!(backend.language(), Language("mock".to_string()));
        assert!(backend.matches_path(mock_path));
        assert!(!backend.matches_path(Path::new("main.rs")));

        let by_lang = reg.find_by_language(&LanguageId::new("mock")).unwrap();
        assert_eq!(by_lang.display_name(), "Mock");
        assert!(reg.find_by_language(&LanguageId::new("unknown")).is_none());
    }

    #[test]
    fn test_mock_backend_traits_through_registry() {
        let reg = test_registry_with_mock();
        let backend = reg.find_by_path(Path::new("test.mock")).unwrap();

        assert_eq!(backend.id().0, "mock-backend");
        assert_eq!(backend.language().as_str(), "mock");
        assert_eq!(backend.display_name(), "Mock");
        assert_eq!(backend.workspace_markers().len(), 1);

        let parser = backend.parser();
        assert_eq!(parser.parser_id().0, "mock-parser");
        assert_eq!(parser.parser_version(), "1.0.0");
        assert!(backend.resolver().is_none());

        let config = BuildIndexOptions {
            use_lsp: false,
            max_depth: None,
            include_tests: true,
            change_detection: FileChangeDetection::MtimeAndSize,
        };
        let meta = backend.metadata(&config);
        assert_eq!(meta.backend_id, "mock-backend");
        assert_eq!(meta.language, "mock");
        assert_eq!(meta.parser_id, "mock-parser");
        assert_eq!(meta.config_hash, "include_tests=true");
        assert_eq!(backend.config_fingerprint(&config), "include_tests=true");
    }

    #[test]
    fn test_mock_parser_via_registry() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("sample.mock");
        std::fs::write(&file_path, "fn alpha()\nfn beta()\n").unwrap();

        let reg = test_registry_with_mock();
        let backend = reg.find_by_path(&file_path).unwrap();
        let parsed = backend
            .parser()
            .parse_file(ParseInput { path: &file_path })
            .unwrap();

        assert_eq!(parsed.symbols.len(), 2);
        let names: std::collections::HashSet<_> =
            parsed.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains("alpha"));
        assert!(names.contains("beta"));
    }

    #[test]
    fn test_production_registry_has_rust_and_python() {
        let reg = super::production_registry();
        assert_eq!(reg.all().len(), 2);
        assert!(reg.find_by_path(Path::new("lib.rs")).is_some());
        assert!(reg.find_by_path(Path::new("main.py")).is_some());
        assert!(reg.find_by_language(&LanguageId::rust()).is_some());
        assert!(reg.find_by_language(&LanguageId::new("python")).is_some());
    }

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