use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use walkdir::WalkDir;

use ctx_codegraph_models::batch_ranges;

use crate::backend::{BackendRegistry, ParseInput, ResolveInput};
use crate::error::CodeGraphError;
use crate::model::{CodeIndex, FileParseStatus, FileSnapshot, GraphEdge, SymbolId, SymbolKind};

#[derive(Debug, Clone)]
pub struct BuildIndexOptions {
    pub use_lsp: bool,
    pub max_depth: Option<usize>,
    pub include_tests: bool,
    pub change_detection: crate::model::FileChangeDetection,
    /// `None` = auto-enable when embedding model path is configured.
    pub with_embeddings: Option<bool>,
    /// `None` = auto-enable when embedding model path is configured (unless disabled).
    pub with_lexical: Option<bool>,
    pub force_search_rebuild: bool,
}

impl Default for BuildIndexOptions {
    fn default() -> Self {
        Self {
            use_lsp: false,
            max_depth: None,
            include_tests: true,
            change_detection: crate::model::FileChangeDetection::MtimeAndSize,
            with_embeddings: None,
            with_lexical: None,
            force_search_rebuild: false,
        }
    }
}

impl BuildIndexOptions {
    pub fn builds_embeddings(&self, auto: bool) -> bool {
        self.with_embeddings.unwrap_or(auto)
    }

    pub fn builds_lexical(&self, auto: bool) -> bool {
        self.with_lexical.unwrap_or(auto)
    }

    pub fn builds_chunks(&self, auto: bool) -> bool {
        self.builds_embeddings(auto) || self.builds_lexical(auto)
    }
}

pub fn compute_file_hash(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 4096];
    loop {
        let n = file.read(&mut buffer).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

pub fn get_mtime_ms(path: &Path) -> Option<i64> {
    let metadata = std::fs::metadata(path).ok()?;
    let mtime = metadata.modified().ok()?;
    let duration = mtime.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(duration.as_millis() as i64)
}

pub fn get_size_bytes(path: &Path) -> Option<i64> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(metadata.len() as i64)
}

pub fn create_file_snapshot(
    workspace_root: &Path,
    abs_path: &Path,
    change_detection: crate::model::FileChangeDetection,
    include_tests: bool,
    registry: &BackendRegistry,
) -> FileSnapshot {
    create_file_snapshot_with_registry(
        workspace_root,
        abs_path,
        change_detection,
        include_tests,
        registry,
    )
}

pub fn create_file_snapshot_with_registry(
    workspace_root: &Path,
    abs_path: &Path,
    change_detection: crate::model::FileChangeDetection,
    include_tests: bool,
    registry: &BackendRegistry,
) -> FileSnapshot {
    let rel_path = abs_path
        .strip_prefix(workspace_root)
        .unwrap_or(abs_path)
        .to_path_buf();
    let size_bytes = get_size_bytes(abs_path).unwrap_or(0) as u64;
    let mtime_ms = get_mtime_ms(abs_path).unwrap_or(0);
    let content_hash = if change_detection == crate::model::FileChangeDetection::ContentHash {
        compute_file_hash(abs_path)
    } else {
        None
    };

    let backend = registry
        .find_by_path(abs_path)
        .expect("No backend registered for path");
    let parser = backend.parser();
    let parser_config_hash = backend.config_fingerprint(&BuildIndexOptions {
        use_lsp: false,
        max_depth: None,
        include_tests,
        change_detection,
        with_embeddings: None,
        with_lexical: None,
        force_search_rebuild: false,
    });

    FileSnapshot {
        file_id: None,
        rel_path,
        abs_path: abs_path.to_path_buf(),
        language: backend.language().clone(),
        backend_id: backend.id().clone(),
        size_bytes,
        mtime_ms,
        mtime_ns: None,
        content_hash,
        parser_id: parser.parser_id().clone(),
        parser_version: parser.parser_version(),
        parser_config_hash,
        indexed_at_ms: None,
        parse_status: FileParseStatus::Success,
    }
}

pub fn should_index_path_with_registry(path: &Path, registry: &BackendRegistry) -> bool {
    for component in path.components() {
        if let Some(s) = component.as_os_str().to_str()
            && crate::discovery::should_skip_dir(s) {
                return false;
            }
    }
    registry.find_by_path(path).is_some()
}

pub fn build_index_with_registry(
    root: &Path,
    options: BuildIndexOptions,
    registry: &BackendRegistry,
) -> Result<CodeIndex, CodeGraphError> {
    // Load ctx_config early for effective_build_batch_size (simple direct load; no sig change).
    let batch_size = ctx_config::find_and_load_config(root)
        .map(|c| c.effective_build_batch_size())
        .unwrap_or(32);

    let mut files = Vec::new();
    let mut global_symbols = Vec::new();
    let mut global_occurrences = Vec::new();

    // Find files
    let walker = WalkDir::new(root).into_iter().filter_entry(|e| {
        let path = e.path();
        if path.is_dir()
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
                && crate::discovery::should_skip_dir(name) {
                    return false;
                }
        true
    });
    let mut matching_files = Vec::new();
    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if should_index_path_with_registry(path, registry) && path.is_file() {
            matching_files.push(path.to_path_buf());
        }
    }

    // Process files in build-batches: group for file I/O + TS parse (using now-resident parser per backend).
    for range in batch_ranges(matching_files.len(), batch_size) {
        let batch = &matching_files[range];
        for path in batch {
            let backend = match registry.find_by_path(&path) {
                Some(b) => b,
                None => continue,
            };

            let mut source_file = create_file_snapshot_with_registry(
                root,
                &path,
                options.change_detection,
                options.include_tests,
                registry,
            );

            let parsed = match backend.parser().parse_file(ParseInput { path: &path }) {
                Ok(res) => res,
                Err(e) => {
                    eprintln!("Warning: Failed to parse {}: {}", path.display(), e);
                    source_file.parse_status = FileParseStatus::Failed;
                    files.push(source_file);
                    continue;
                }
            };
            files.push(source_file);

            let (mut file_symbols, mut file_occurrences) = (parsed.symbols, parsed.occurrences);

            if !options.include_tests {
                let mut new_symbols = Vec::new();
                let mut index_map = std::collections::HashMap::new();
                for (i, sym) in file_symbols.into_iter().enumerate() {
                    if sym.kind != SymbolKind::Test {
                        index_map.insert(i, new_symbols.len());
                        new_symbols.push(sym);
                    }
                }
                file_symbols = new_symbols;

                file_occurrences.retain(|cs| {
                    if let Some(old_idx) = cs.enclosing_temp_index {
                        index_map.contains_key(&old_idx)
                    } else {
                        true
                    }
                });

                for cs in &mut file_occurrences {
                    if let Some(ref mut idx) = cs.enclosing_temp_index
                        && let Some(&new_idx) = index_map.get(idx) {
                            *idx = new_idx;
                        }
                }
            }

            let start_sym_idx = global_symbols.len();
            for cs in &mut file_occurrences {
                if let Some(ref mut idx) = cs.enclosing_temp_index {
                    *idx += start_sym_idx;
                }
            }

            global_symbols.extend(file_symbols);
            global_occurrences.extend(file_occurrences);
        }
        // After a batch is parsed, optionally note opportunity for early chunk (leave TODO; do not implement full embed here).
        // TODO: after batch parse, could flush/early-chunk here before next batch I/O (for future search build integration)
    }

    // Set temporary symbol IDs
    for (i, sym) in global_symbols.iter_mut().enumerate() {
        sym.id = Some(SymbolId(i as i64));
    }

    // Set temporary enclosing symbol IDs on occurrences
    for cs in &mut global_occurrences {
        if let Some(from_idx) = cs.enclosing_temp_index {
            cs.enclosing_symbol = Some(SymbolId(from_idx as i64));
        }
    }

    // Resolve call occurrences
    let mut edges = Vec::new();
    let name_index = crate::noop::SymbolNameIndex::new(&global_symbols);

    for (call_site_idx, cs) in global_occurrences.iter().enumerate() {
        if cs.kind != crate::model::OccurrenceKind::Call {
            continue;
        }

        let from_id = match cs.enclosing_symbol {
            Some(id) => id,
            None => {
                // Correct invariant: occurrence without enclosing symbol has no symbol-to-symbol edge
                continue;
            }
        };

        let mut resolved_idx = None;
        let mut confidence = crate::model::ResolutionConfidence::Unresolved;

        let backend = registry.find_by_path(&cs.file);
        let resolver = backend.and_then(|b| b.resolver());

        if options.use_lsp
            && let Some(res) = resolver {
                let resolve_input = ResolveInput {
                    workspace_root: root,
                    occurrence: cs,
                    symbols: &global_symbols,
                };
                match res.resolve(resolve_input) {
                    Ok(out) => {
                        resolved_idx = out.resolved_symbol_index;
                        confidence = out.confidence;
                    }
                    Err(err) => {
                        eprintln!(
                            "LSP resolution warning for call to {}: {}",
                            cs.raw_text, err
                        );
                    }
                }
            }

        if resolved_idx.is_none() {
            let (fallback_idx, fallback_conf) =
                name_index.resolve(&cs.raw_text, &global_symbols, &cs.file);
            resolved_idx = fallback_idx;
            confidence = fallback_conf;
        }

        let edge = GraphEdge {
            id: None,
            kind: crate::model::EdgeKind::Call,
            from_file_id: cs.file_id,
            from_symbol_id: Some(from_id),
            to_symbol_id: resolved_idx.map(|idx| SymbolId(idx as i64)),
            to_external: None,
            occurrence_id: Some(crate::model::OccurrenceId(call_site_idx as i64)),
            raw_text: Some(cs.raw_text.clone()),
            range: Some(cs.range.clone()),
            confidence,
            produced_by: Some(
                resolver
                    .map(|r| r.resolver_id().clone())
                    .unwrap_or_else(|| crate::backend::ResolverId::new("noop")),
            ),
        };
        edges.push(edge);
    }

    let call_sites_compat = global_occurrences
        .iter()
        .filter(|o| o.kind == crate::model::OccurrenceKind::Call)
        .cloned()
        .collect();

    Ok(CodeIndex {
        root: root.to_path_buf(),
        files,
        symbols: global_symbols,
        occurrences: global_occurrences,
        edges,
        call_sites: call_sites_compat,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::traits::{
        BackendId, BackendMetadata, LanguageBackend, ParseInput, ParsedFile, ParserBackend,
        ParserId, ResolveInput, ResolveOutput, ResolverBackend, ResolverId, WorkspaceMarker,
    };
    use crate::model::{
        FileChangeDetection, Language, LanguageId, Occurrence, OccurrenceKind,
        ResolutionConfidence, Symbol, SymbolKind, TextRange,
    };
    use std::path::Path;

    struct IndexTestParser {
        fail: bool,
    }

    impl ParserBackend for IndexTestParser {
        fn parser_id(&self) -> ParserId {
            ParserId::new("index-test-parser")
        }

        fn parser_version(&self) -> String {
            "0.0.1".to_string()
        }

        fn parse_file(&self, input: ParseInput<'_>) -> Result<ParsedFile, CodeGraphError> {
            if self.fail {
                return Err(CodeGraphError::Parse("forced failure".into()));
            }

            let path = input.path.to_path_buf();
            let caller = Symbol {
                id: None,
                file_id: None,
                name: "caller".to_string(),
                qualified_name: "caller".to_string(),
                kind: SymbolKind::Function,
                language: LanguageId::new("idxtest"),
                file: path.clone(),
                range: TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 3,
                    end_col: 1,
                },
                body_range: Some(TextRange {
                    start_line: 2,
                    start_col: 1,
                    end_line: 2,
                    end_col: 20,
                }),
            };
            let callee = Symbol {
                id: None,
                file_id: None,
                name: "callee".to_string(),
                qualified_name: "callee".to_string(),
                kind: SymbolKind::Function,
                language: LanguageId::new("idxtest"),
                file: path.clone(),
                range: TextRange {
                    start_line: 5,
                    start_col: 1,
                    end_line: 5,
                    end_col: 20,
                },
                body_range: None,
            };
            let test_sym = Symbol {
                id: None,
                file_id: None,
                name: "test_case".to_string(),
                qualified_name: "test_case".to_string(),
                kind: SymbolKind::Test,
                language: LanguageId::new("idxtest"),
                file: path.clone(),
                range: TextRange {
                    start_line: 7,
                    start_col: 1,
                    end_line: 7,
                    end_col: 20,
                },
                body_range: None,
            };

            let call_with_enclosing = Occurrence {
                id: None,
                file_id: None,
                enclosing_symbol: None,
                enclosing_temp_index: Some(0),
                kind: OccurrenceKind::Call,
                raw_text: "callee".to_string(),
                file: path.clone(),
                range: TextRange {
                    start_line: 2,
                    start_col: 5,
                    end_line: 2,
                    end_col: 11,
                },
                language: LanguageId::new("idxtest"),
                backend_id: BackendId::new("index-test-backend"),
            };
            let call_without_enclosing = Occurrence {
                id: None,
                file_id: None,
                enclosing_symbol: None,
                enclosing_temp_index: None,
                kind: OccurrenceKind::Call,
                raw_text: "orphan".to_string(),
                file: path,
                range: TextRange {
                    start_line: 4,
                    start_col: 1,
                    end_line: 4,
                    end_col: 7,
                },
                language: LanguageId::new("idxtest"),
                backend_id: BackendId::new("index-test-backend"),
            };

            Ok(ParsedFile {
                symbols: vec![caller, callee, test_sym],
                occurrences: vec![call_with_enclosing, call_without_enclosing],
            })
        }
    }

    struct IndexTestBackend {
        parser: IndexTestParser,
    }

    impl IndexTestBackend {
        fn new(fail_parse: bool) -> Self {
            Self {
                parser: IndexTestParser { fail: fail_parse },
            }
        }
    }

    impl LanguageBackend for IndexTestBackend {
        fn id(&self) -> BackendId {
            BackendId::new("index-test-backend")
        }

        fn language(&self) -> Language {
            LanguageId::new("idxtest")
        }

        fn display_name(&self) -> &'static str {
            "IndexTest"
        }

        fn matches_path(&self, path: &Path) -> bool {
            path.extension().and_then(|e| e.to_str()) == Some("idxtest")
        }

        fn parser(&self) -> &dyn ParserBackend {
            &self.parser
        }

        fn resolver(&self) -> Option<&dyn ResolverBackend> {
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

    struct FixedResolver;

    impl ResolverBackend for FixedResolver {
        fn resolver_id(&self) -> ResolverId {
            ResolverId::new("fixed-resolver")
        }

        fn resolver_version(&self) -> String {
            "1.0.0".to_string()
        }

        fn resolve(&self, _input: ResolveInput<'_>) -> Result<ResolveOutput, CodeGraphError> {
            Ok(ResolveOutput {
                resolved_symbol_index: Some(1),
                confidence: ResolutionConfidence::LspExact,
            })
        }
    }

    struct LspIndexTestBackend {
        inner: IndexTestBackend,
        resolver: FixedResolver,
    }

    impl LspIndexTestBackend {
        fn new() -> Self {
            Self {
                inner: IndexTestBackend::new(false),
                resolver: FixedResolver,
            }
        }
    }

    impl LanguageBackend for LspIndexTestBackend {
        fn id(&self) -> BackendId {
            self.inner.id()
        }
        fn language(&self) -> Language {
            self.inner.language()
        }
        fn display_name(&self) -> &'static str {
            self.inner.display_name()
        }
        fn matches_path(&self, path: &Path) -> bool {
            self.inner.matches_path(path)
        }
        fn parser(&self) -> &dyn ParserBackend {
            self.inner.parser()
        }
        fn resolver(&self) -> Option<&dyn ResolverBackend> {
            Some(&self.resolver)
        }
        fn workspace_markers(&self) -> &[WorkspaceMarker] {
            self.inner.workspace_markers()
        }
        fn metadata(&self, config: &BuildIndexOptions) -> BackendMetadata {
            self.inner.metadata(config)
        }
        fn config_fingerprint(&self, config: &BuildIndexOptions) -> String {
            self.inner.config_fingerprint(config)
        }
    }

    fn test_registry(fail_parse: bool) -> BackendRegistry {
        let mut reg = BackendRegistry::new();
        reg.register(Box::new(IndexTestBackend::new(fail_parse)));
        reg
    }

    #[test]
    fn build_index_options_embedding_and_lexical_flags() {
        let auto = BuildIndexOptions::default();
        assert!(auto.builds_embeddings(true));
        assert!(!auto.builds_embeddings(false));
        assert!(auto.builds_lexical(true));
        assert!(auto.builds_chunks(true));

        let disabled = BuildIndexOptions {
            with_embeddings: Some(false),
            with_lexical: Some(false),
            ..Default::default()
        };
        assert!(!disabled.builds_embeddings(true));
        assert!(!disabled.builds_lexical(true));
        assert!(!disabled.builds_chunks(true));
    }

    #[test]
    fn compute_file_hash_is_stable_sha256() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hash.txt");
        std::fs::write(&path, "hello index").unwrap();

        let hash = compute_file_hash(&path).unwrap();
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, compute_file_hash(&path).unwrap());
    }

    #[test]
    fn file_metadata_helpers_handle_missing_and_existing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meta.txt");
        std::fs::write(&path, "data").unwrap();

        assert!(get_size_bytes(&path).unwrap() > 0);
        assert!(get_mtime_ms(&path).unwrap() > 0);
        assert!(get_size_bytes(Path::new("/no/such/file")).is_none());
        assert!(get_mtime_ms(Path::new("/no/such/file")).is_none());
        assert!(compute_file_hash(Path::new("/no/such/file")).is_none());
    }

    #[test]
    fn create_file_snapshot_respects_content_hash_mode() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file_path = root.join("sample.idxtest");
        std::fs::write(&file_path, "fn sample()").unwrap();
        let registry = test_registry(false);

        let mtime_snapshot = create_file_snapshot_with_registry(
            root,
            &file_path,
            FileChangeDetection::MtimeAndSize,
            true,
            &registry,
        );
        assert!(mtime_snapshot.content_hash.is_none());
        assert_eq!(mtime_snapshot.language.as_str(), "idxtest");

        let hash_snapshot = create_file_snapshot_with_registry(
            root,
            &file_path,
            FileChangeDetection::ContentHash,
            false,
            &registry,
        );
        assert!(hash_snapshot.content_hash.is_some());
        assert_eq!(hash_snapshot.parser_config_hash, "include_tests=false");
    }

    #[test]
    fn should_index_path_skips_ignored_directories() {
        let registry = test_registry(false);
        assert!(!should_index_path_with_registry(
            Path::new("target/debug/foo.idxtest"),
            &registry
        ));
        assert!(!should_index_path_with_registry(
            Path::new("node_modules/pkg/foo.idxtest"),
            &registry
        ));
        assert!(should_index_path_with_registry(
            Path::new("src/foo.idxtest"),
            &registry
        ));
    }

    #[test]
    fn build_index_indexes_files_and_resolves_call_edges() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("main.idxtest"), "content").unwrap();

        let index = build_index_with_registry(root, BuildIndexOptions::default(), &test_registry(false))
            .unwrap();

        assert_eq!(index.files.len(), 1);
        assert_eq!(index.symbols.len(), 3);
        assert_eq!(index.edges.len(), 1);
        assert_eq!(index.edges[0].to_symbol_id, Some(SymbolId(1)));
        assert_eq!(index.edges[0].confidence, ResolutionConfidence::Syntax);
    }

    #[test]
    fn build_index_excludes_test_symbols_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("main.idxtest"), "content").unwrap();

        let options = BuildIndexOptions {
            include_tests: false,
            ..Default::default()
        };
        let index = build_index_with_registry(root, options, &test_registry(false)).unwrap();

        assert_eq!(index.symbols.len(), 2);
        assert!(!index.symbols.iter().any(|s| s.kind == SymbolKind::Test));
    }

    #[test]
    fn build_index_records_parse_failures() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("broken.idxtest"), "content").unwrap();

        let index = build_index_with_registry(root, BuildIndexOptions::default(), &test_registry(true))
            .unwrap();

        assert_eq!(index.files.len(), 1);
        assert_eq!(index.files[0].parse_status, FileParseStatus::Failed);
        assert!(index.symbols.is_empty());
    }

    #[test]
    fn build_index_uses_lsp_resolver_when_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("main.idxtest"), "content").unwrap();

        let mut registry = BackendRegistry::new();
        registry.register(Box::new(LspIndexTestBackend::new()));

        let options = BuildIndexOptions {
            use_lsp: true,
            ..Default::default()
        };
        let index = build_index_with_registry(root, options, &registry).unwrap();

        assert_eq!(index.edges.len(), 1);
        assert_eq!(index.edges[0].confidence, ResolutionConfidence::LspExact);
        assert_eq!(
            index.edges[0].produced_by.as_ref().map(|r| r.0.as_str()),
            Some("fixed-resolver")
        );
    }
}


