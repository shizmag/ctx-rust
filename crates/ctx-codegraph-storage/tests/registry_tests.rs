use ctx_codegraph_lang::backend::{BackendRegistry, ParseInput};
use ctx_codegraph_lang::index::BuildIndexOptions;
use ctx_codegraph_lang::model::{FileChangeDetection, Language, LanguageId};
use ctx_codegraph_storage::languages::MockBackend;
use ctx_codegraph_storage::storage::rebuild_index_db_with_registry;
use ctx_lang_python::PythonBackend;
use ctx_lang_rust::RustBackend;
use std::path::Path;

fn test_registry_with_mock() -> BackendRegistry {
    let mut reg = production_registry();
    reg.register(Box::new(MockBackend::new()));
    reg
}

fn production_registry() -> BackendRegistry {
    let mut reg = BackendRegistry::new();
    reg.register(Box::new(RustBackend::new()));
    reg.register(Box::new(PythonBackend::new()));
    reg
}

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

    let config = BuildIndexOptions::default();
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
    let reg = production_registry();
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

    let options = BuildIndexOptions::default();

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