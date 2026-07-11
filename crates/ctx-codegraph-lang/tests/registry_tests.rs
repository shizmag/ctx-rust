use ctx_codegraph_lang::backend::registry::BackendRegistry;
use ctx_codegraph_lang::backend::traits::{
    BackendId, BackendMetadata, LanguageBackend, ParseInput, ParsedFile, ParserBackend, ParserId,
    WorkspaceMarker,
};
use ctx_codegraph_lang::index::BuildIndexOptions;
use ctx_codegraph_lang::model::{Language, LanguageId};
use std::path::Path;

struct StubParser;

impl ParserBackend for StubParser {
    fn parser_id(&self) -> ParserId {
        ParserId::new("stub-parser")
    }

    fn parser_version(&self) -> String {
        "0.0.1".to_string()
    }

    fn parse_file(&self, _input: ParseInput<'_>) -> Result<ParsedFile, ctx_codegraph_lang::CodeGraphError> {
        Ok(ParsedFile {
            symbols: Vec::new(),
            occurrences: Vec::new(),
        })
    }
}

struct StubBackend {
    parser: StubParser,
}

impl StubBackend {
    fn new() -> Self {
        Self {
            parser: StubParser,
        }
    }
}

impl LanguageBackend for StubBackend {
    fn id(&self) -> BackendId {
        BackendId::new("stub-backend")
    }

    fn language(&self) -> Language {
        LanguageId::new("stub")
    }

    fn display_name(&self) -> &'static str {
        "Stub"
    }

    fn matches_path(&self, path: &Path) -> bool {
        path.extension().and_then(|e| e.to_str()) == Some("stub")
    }

    fn parser(&self) -> &dyn ParserBackend {
        &self.parser
    }

    fn resolver(&self) -> Option<&dyn ctx_codegraph_lang::backend::ResolverBackend> {
        None
    }

    fn workspace_markers(&self) -> &[WorkspaceMarker] {
        &[]
    }

    fn metadata(&self, config: &BuildIndexOptions) -> BackendMetadata {
        BackendMetadata {
            backend_id: self.id().0,
            language: self.language().as_str().to_string(),
            parser_id: self.parser().parser_id().0,
            parser_version: self.parser().parser_version(),
            resolver_id: None,
            resolver_version: None,
            config_hash: self.config_fingerprint(config),
        }
    }

    fn config_fingerprint(&self, config: &BuildIndexOptions) -> String {
        format!("include_tests={}", config.include_tests)
    }
}

#[test]
fn registry_lookup_by_path_and_language() {
    let mut reg = BackendRegistry::new();
    assert!(reg.all().is_empty());

    reg.register(Box::new(StubBackend::new()));
    assert_eq!(reg.all().len(), 1);

    let stub_path = Path::new("src/example.stub");
    let backend = reg.find_by_path(stub_path).unwrap();
    assert_eq!(backend.id().0, "stub-backend");
    assert_eq!(backend.language().as_str(), "stub");
    assert!(!backend.matches_path(Path::new("main.rs")));

    let by_lang = reg.find_by_language(&LanguageId::new("stub")).unwrap();
    assert_eq!(by_lang.display_name(), "Stub");
    assert!(reg.find_by_language(&LanguageId::new("unknown")).is_none());
}

#[test]
fn default_registry_is_empty() {
    let reg = BackendRegistry::default();
    assert!(reg.all().is_empty());
    assert!(reg.find_by_path(Path::new("any.file")).is_none());
}

#[test]
fn find_by_path_returns_none_for_unregistered_extension() {
    let mut reg = BackendRegistry::new();
    reg.register(Box::new(StubBackend::new()));
    assert!(reg.find_by_path(Path::new("main.rs")).is_none());
    assert!(reg.find_by_path(Path::new("readme.md")).is_none());
}

#[test]
fn multiple_backends_first_match_wins() {
    struct OtherStub;

    impl LanguageBackend for OtherStub {
        fn id(&self) -> BackendId {
            BackendId::new("other-backend")
        }
        fn language(&self) -> Language {
            LanguageId::new("other")
        }
        fn display_name(&self) -> &'static str {
            "Other"
        }
        fn matches_path(&self, path: &Path) -> bool {
            path.extension().and_then(|e| e.to_str()) == Some("stub")
        }
        fn parser(&self) -> &dyn ParserBackend {
            &StubParser
        }
        fn resolver(&self) -> Option<&dyn ctx_codegraph_lang::backend::ResolverBackend> {
            None
        }
        fn workspace_markers(&self) -> &[WorkspaceMarker] {
            &[]
        }
        fn metadata(&self, config: &BuildIndexOptions) -> BackendMetadata {
            BackendMetadata {
                backend_id: self.id().0,
                language: self.language().as_str().to_string(),
                parser_id: self.parser().parser_id().0,
                parser_version: self.parser().parser_version(),
                resolver_id: None,
                resolver_version: None,
                config_hash: self.config_fingerprint(config),
            }
        }
        fn config_fingerprint(&self, config: &BuildIndexOptions) -> String {
            format!("include_tests={}", config.include_tests)
        }
    }

    let mut reg = BackendRegistry::new();
    reg.register(Box::new(StubBackend::new()));
    reg.register(Box::new(OtherStub));

    let backend = reg.find_by_path(Path::new("x.stub")).unwrap();
    assert_eq!(backend.id().0, "stub-backend");
    assert_eq!(reg.all().len(), 2);
}

#[test]
fn find_by_language_returns_first_registered_backend() {
    let mut reg = BackendRegistry::new();
    reg.register(Box::new(StubBackend::new()));
    let by_lang = reg.find_by_language(&LanguageId::new("stub")).unwrap();
    assert_eq!(by_lang.id().0, "stub-backend");
}

#[test]
fn find_by_language_returns_first_when_multiple_share_language() {
    struct OtherStub;

    impl LanguageBackend for OtherStub {
        fn id(&self) -> BackendId {
            BackendId::new("other-backend")
        }
        fn language(&self) -> Language {
            LanguageId::new("stub")
        }
        fn display_name(&self) -> &'static str {
            "Other Stub"
        }
        fn matches_path(&self, path: &Path) -> bool {
            path.extension().and_then(|e| e.to_str()) == Some("other")
        }
        fn parser(&self) -> &dyn ParserBackend {
            &StubParser
        }
        fn resolver(&self) -> Option<&dyn ctx_codegraph_lang::backend::ResolverBackend> {
            None
        }
        fn workspace_markers(&self) -> &[WorkspaceMarker] {
            &[]
        }
        fn metadata(&self, config: &BuildIndexOptions) -> BackendMetadata {
            BackendMetadata {
                backend_id: self.id().0,
                language: self.language().as_str().to_string(),
                parser_id: self.parser().parser_id().0,
                parser_version: self.parser().parser_version(),
                resolver_id: None,
                resolver_version: None,
                config_hash: self.config_fingerprint(config),
            }
        }
        fn config_fingerprint(&self, config: &BuildIndexOptions) -> String {
            format!("include_tests={}", config.include_tests)
        }
    }

    let mut reg = BackendRegistry::new();
    reg.register(Box::new(StubBackend::new()));
    reg.register(Box::new(OtherStub));

    let by_lang = reg.find_by_language(&LanguageId::new("stub")).unwrap();
    assert_eq!(by_lang.id().0, "stub-backend");
}

#[test]
fn find_by_path_returns_none_without_extension() {
    let mut reg = BackendRegistry::new();
    reg.register(Box::new(StubBackend::new()));
    assert!(reg.find_by_path(Path::new("README")).is_none());
    assert!(reg.find_by_path(Path::new("src/noext")).is_none());
}

#[test]
fn register_appends_backends_in_order() {
    let mut reg = BackendRegistry::new();
    reg.register(Box::new(StubBackend::new()));
    assert_eq!(reg.all().len(), 1);
    assert_eq!(reg.all()[0].id().0, "stub-backend");

    struct OtherStub;
    impl LanguageBackend for OtherStub {
        fn id(&self) -> BackendId {
            BackendId::new("other-backend")
        }
        fn language(&self) -> Language {
            LanguageId::new("other")
        }
        fn display_name(&self) -> &'static str {
            "Other"
        }
        fn matches_path(&self, path: &Path) -> bool {
            path.extension().and_then(|e| e.to_str()) == Some("other")
        }
        fn parser(&self) -> &dyn ParserBackend {
            &StubParser
        }
        fn resolver(&self) -> Option<&dyn ctx_codegraph_lang::backend::ResolverBackend> {
            None
        }
        fn workspace_markers(&self) -> &[WorkspaceMarker] {
            &[]
        }
        fn metadata(&self, config: &BuildIndexOptions) -> BackendMetadata {
            BackendMetadata {
                backend_id: self.id().0,
                language: self.language().as_str().to_string(),
                parser_id: self.parser().parser_id().0,
                parser_version: self.parser().parser_version(),
                resolver_id: None,
                resolver_version: None,
                config_hash: self.config_fingerprint(config),
            }
        }
        fn config_fingerprint(&self, config: &BuildIndexOptions) -> String {
            format!("include_tests={}", config.include_tests)
        }
    }

    reg.register(Box::new(OtherStub));
    assert_eq!(reg.all().len(), 2);
    assert_eq!(reg.all()[1].id().0, "other-backend");
}

#[test]
fn should_index_path_respects_registry_and_skip_dirs() {
    let mut reg = BackendRegistry::new();
    reg.register(Box::new(StubBackend::new()));

    assert!(ctx_codegraph_lang::index::should_index_path_with_registry(
        Path::new("src/foo.stub"),
        &reg
    ));
    assert!(!ctx_codegraph_lang::index::should_index_path_with_registry(
        Path::new("target/foo.stub"),
        &reg
    ));
    assert!(!ctx_codegraph_lang::index::should_index_path_with_registry(
        Path::new("src/foo.rs"),
        &reg
    ));
}

#[test]
fn backend_metadata_includes_config_fingerprint() {
    let backend = StubBackend::new();
    let config = BuildIndexOptions { extraction_tier: None,
        include_tests: true,
        ..BuildIndexOptions::default()
    };
    let meta = backend.metadata(&config);
    assert_eq!(meta.backend_id, "stub-backend");
    assert_eq!(meta.language, "stub");
    assert_eq!(meta.parser_id, "stub-parser");
    assert_eq!(meta.config_hash, "include_tests=true");
}