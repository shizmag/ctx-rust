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
}

#[cfg(test)]
mod tests {
    use super::BackendRegistry;
    use crate::backend::traits::{
        BackendId, BackendMetadata, LanguageBackend, ParseInput, ParsedFile, ParserBackend,
        ParserId, WorkspaceMarker,
    };
    use crate::index::BuildIndexOptions;
    use crate::model::{Language, LanguageId};
    use std::path::Path;

    struct StubParser;

    impl ParserBackend for StubParser {
        fn parser_id(&self) -> ParserId {
            ParserId::new("stub-parser")
        }

        fn parser_version(&self) -> String {
            "0.0.1".to_string()
        }

        fn parse_file(&self, _input: ParseInput<'_>) -> Result<ParsedFile, crate::CodeGraphError> {
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

        fn resolver(&self) -> Option<&dyn crate::backend::ResolverBackend> {
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
    fn should_index_path_respects_registry_and_skip_dirs() {
        let mut reg = BackendRegistry::new();
        reg.register(Box::new(StubBackend::new()));

        assert!(crate::index::should_index_path_with_registry(
            Path::new("src/foo.stub"),
            &reg
        ));
        assert!(!crate::index::should_index_path_with_registry(
            Path::new("target/foo.stub"),
            &reg
        ));
        assert!(!crate::index::should_index_path_with_registry(
            Path::new("src/foo.rs"),
            &reg
        ));
    }
}