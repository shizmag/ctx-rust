use crate::backend::{
    BackendId, BackendMetadata, LanguageBackend, ParseInput, ParsedFile, ParserBackend, ParserId,
    ResolverBackend, WorkspaceMarker,
};
use crate::error::CodeGraphError;
use crate::index::BuildIndexOptions;
use crate::model::{Language, Symbol, SymbolKind, TextRange};
use std::path::Path;

pub struct MockBackend {
    parser: MockParser,
}

impl MockBackend {
    pub fn new() -> Self {
        Self { parser: MockParser }
    }
}

impl LanguageBackend for MockBackend {
    fn id(&self) -> BackendId {
        BackendId("mock-backend".to_string())
    }
    fn language(&self) -> Language {
        Language("mock".to_string())
    }
    fn display_name(&self) -> &'static str {
        "Mock"
    }
    fn matches_path(&self, path: &Path) -> bool {
        path.extension().map(|e| e == "mock").unwrap_or(false)
    }
    fn parser(&self) -> &dyn ParserBackend {
        &self.parser
    }
    fn resolver(&self) -> Option<&dyn ResolverBackend> {
        None
    }
    fn workspace_markers(&self) -> &[WorkspaceMarker] {
        static MARKERS: [WorkspaceMarker; 1] = [WorkspaceMarker::File("mock.project")];
        &MARKERS
    }
    fn metadata(&self, config: &BuildIndexOptions) -> BackendMetadata {
        BackendMetadata {
            backend_id: self.id().0,
            language: self.language().0,
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

pub struct MockParser;

impl ParserBackend for MockParser {
    fn parser_id(&self) -> ParserId {
        ParserId("mock-parser".to_string())
    }
    fn parser_version(&self) -> String {
        "1.0.0".to_string()
    }
    fn parse_file(&self, input: ParseInput<'_>) -> Result<ParsedFile, CodeGraphError> {
        let content = std::fs::read_to_string(input.path)?;
        let mut symbols = Vec::new();
        for (idx, line) in content.lines().enumerate() {
            let line_trimmed = line.trim();
            if line_trimmed.starts_with("fn ") {
                let parts: Vec<&str> = line_trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    let name = parts[1].replace("()", "");
                    symbols.push(Symbol {
                        id: None,
                        file_id: None,
                        name: name.clone(),
                        qualified_name: format!("mock::{}", name),
                        kind: SymbolKind::Function,
                        language: Language("mock".to_string()),
                        file: input.path.to_path_buf(),
                        range: TextRange {
                            start_line: idx + 1,
                            start_col: 1,
                            end_line: idx + 1,
                            end_col: line.len() + 1,
                        },
                        body_range: None,
                    });
                }
            }
        }
        Ok(ParsedFile {
            symbols,
            occurrences: Vec::new(),
        })
    }
}
