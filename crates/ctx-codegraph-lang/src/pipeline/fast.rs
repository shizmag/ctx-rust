//! Tier 1: Fast structural extraction — parallel Tree-Sitter parsing.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use ctx_codegraph_models::batch_ranges;
use rayon::prelude::*;

use crate::backend::{BackendRegistry, ParseInput};
use crate::error::CodeGraphError;
use crate::index::{create_file_snapshot_with_registry, BuildIndexOptions};
use crate::model::{FileParseStatus, FileSnapshot, Occurrence, Symbol, SymbolKind};
use crate::pipeline::{PipelineTimings, discover_source_files};

/// Result of parsing a single file.
#[derive(Debug)]
pub struct ParsedFileBatch {
    pub snapshot: FileSnapshot,
    pub symbols: Vec<Symbol>,
    pub occurrences: Vec<Occurrence>,
}

/// Configure the global rayon thread pool for indexing (no-op if already initialized).
pub fn configure_parallel_pool(thread_count: usize) -> Result<(), CodeGraphError> {
    let threads = thread_count.max(1);
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|i| format!("ctx-parse-{i}"))
        .build_global()
        .map_err(|e| CodeGraphError::Internal(format!("rayon pool init failed: {e}")))?;
    Ok(())
}

/// Tier 1: discover and parse all matching files in parallel batches.
pub fn run_fast_structural(
    root: &Path,
    options: &BuildIndexOptions,
    registry: &BackendRegistry,
    batch_size: usize,
    parallel_threads: usize,
    timings: &mut PipelineTimings,
) -> Result<(Vec<FileSnapshot>, Vec<Symbol>, Vec<Occurrence>), CodeGraphError> {
    let started = Instant::now();
    let _ = configure_parallel_pool(parallel_threads);

    let matching_files = discover_source_files(root, registry)?;
    let registry = Arc::new(registry);

    let mut files = Vec::with_capacity(matching_files.len());
    let mut global_symbols = Vec::new();
    let mut global_occurrences = Vec::new();

    let target_tier = options
        .extraction_tier
        .unwrap_or(crate::model::ExtractionTier::Fast);

    for range in batch_ranges(matching_files.len(), batch_size.max(1)) {
        let batch = &matching_files[range];

        let batch_results: Vec<ParsedFileBatch> = batch
            .par_iter()
            .map(|path| parse_single_file(root, path, options, registry.as_ref(), target_tier))
            .collect();

        for result in batch_results {
            files.push(result.snapshot);

            let (file_symbols, file_occurrences) = filter_and_offset_symbols(
                result.symbols,
                result.occurrences,
                options.include_tests,
                global_symbols.len(),
            );

            global_symbols.extend(file_symbols);
            global_occurrences.extend(file_occurrences);
        }
    }

    timings.record("fast_structural", started);
    Ok((files, global_symbols, global_occurrences))
}

fn parse_single_file(
    root: &Path,
    path: &PathBuf,
    options: &BuildIndexOptions,
    registry: &BackendRegistry,
    target_tier: crate::model::ExtractionTier,
) -> ParsedFileBatch {
    let backend = match registry.find_by_path(path) {
        Some(b) => b,
        None => {
            return ParsedFileBatch {
                snapshot: empty_failed_snapshot(root, path, options, registry),
                symbols: Vec::new(),
                occurrences: Vec::new(),
            };
        }
    };

    let mut source_file = create_file_snapshot_with_registry(
        root,
        path,
        options.change_detection,
        options.include_tests,
        registry,
    );
    source_file.max_tier = target_tier;

    match backend.parser().parse_file(ParseInput { path }) {
        Ok(parsed) => ParsedFileBatch {
            snapshot: source_file,
            symbols: parsed.symbols,
            occurrences: parsed.occurrences,
        },
        Err(e) => {
            eprintln!("Warning: Failed to parse {}: {}", path.display(), e);
            source_file.parse_status = FileParseStatus::Failed;
            ParsedFileBatch {
                snapshot: source_file,
                symbols: Vec::new(),
                occurrences: Vec::new(),
            }
        }
    }
}

fn empty_failed_snapshot(
    root: &Path,
    path: &PathBuf,
    options: &BuildIndexOptions,
    registry: &BackendRegistry,
) -> FileSnapshot {
    let mut snap = create_file_snapshot_with_registry(
        root,
        path,
        options.change_detection,
        options.include_tests,
        registry,
    );
    snap.parse_status = FileParseStatus::Failed;
    snap
}

fn filter_and_offset_symbols(
    file_symbols: Vec<Symbol>,
    mut file_occurrences: Vec<Occurrence>,
    include_tests: bool,
    start_sym_idx: usize,
) -> (Vec<Symbol>, Vec<Occurrence>) {
    let (file_symbols, index_map) = if include_tests {
        (file_symbols, None)
    } else {
        let mut new_symbols = Vec::new();
        let mut index_map = std::collections::HashMap::new();
        for (i, sym) in file_symbols.into_iter().enumerate() {
            if sym.kind != SymbolKind::Test {
                index_map.insert(i, new_symbols.len());
                new_symbols.push(sym);
            }
        }
        (new_symbols, Some(index_map))
    };

    if let Some(ref index_map) = index_map {
        file_occurrences.retain(|cs| {
            cs.enclosing_temp_index
                .map(|idx| index_map.contains_key(&idx))
                .unwrap_or(true)
        });
        for cs in &mut file_occurrences {
            if let Some(ref mut idx) = cs.enclosing_temp_index
                && let Some(&new_idx) = index_map.get(idx)
            {
                *idx = new_idx;
            }
        }
    }

    for cs in &mut file_occurrences {
        if let Some(ref mut idx) = cs.enclosing_temp_index {
            *idx += start_sym_idx;
        }
    }

    (file_symbols, file_occurrences)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::traits::{
        BackendId, BackendMetadata, LanguageBackend, ParseInput, ParsedFile, ParserBackend,
        ParserId, ResolverBackend, ResolverId, WorkspaceMarker,
    };
    use crate::model::{
        Language, LanguageId, OccurrenceKind, Symbol, SymbolKind, TextRange,
    };

    struct FastTestParser;

    impl ParserBackend for FastTestParser {
        fn parser_id(&self) -> ParserId {
            ParserId::new("fast-test-parser")
        }

        fn parser_version(&self) -> String {
            "0.0.1".to_string()
        }

        fn parse_file(&self, input: ParseInput<'_>) -> Result<ParsedFile, CodeGraphError> {
            let path = input.path.to_path_buf();
            let sym = Symbol {
                nesting_depth: 2,
                lines_of_code: 5,
                complexity_proxy: 3,
                param_count: 1,
                parent_symbol_id: None,
                fan_in: 0,
                fan_out: 0,
                coupling: 0.0,
                cohesion: 0.0,
                id: None,
                file_id: None,
                name: "main".to_string(),
                qualified_name: "main".to_string(),
                kind: SymbolKind::Function,
                language: LanguageId::new("fasttest"),
                file: path.clone(),
                range: TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 5,
                    end_col: 1,
                },
                body_range: None,
            };
            Ok(ParsedFile {
                symbols: vec![sym],
                occurrences: vec![],
            })
        }
    }

    struct FastTestBackend {
        parser: FastTestParser,
    }

    impl LanguageBackend for FastTestBackend {
        fn id(&self) -> BackendId {
            BackendId::new("fast-test-backend")
        }
        fn language(&self) -> Language {
            LanguageId::new("fasttest")
        }
        fn display_name(&self) -> &'static str {
            "FastTest"
        }
        fn matches_path(&self, path: &Path) -> bool {
            path.extension().and_then(|e| e.to_str()) == Some("fasttest")
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

    #[test]
    fn parallel_fast_structural_parses_multiple_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.fasttest"), "fn a()").unwrap();
        std::fs::write(root.join("b.fasttest"), "fn b()").unwrap();

        let mut registry = BackendRegistry::new();
        registry.register(Box::new(FastTestBackend {
            parser: FastTestParser,
        }));

        let options = BuildIndexOptions {
            extraction_tier: Some(crate::model::ExtractionTier::Fast),
            ..Default::default()
        };
        let mut timings = PipelineTimings::default();
        let (files, symbols, occurrences) = run_fast_structural(
            root,
            &options,
            &registry,
            32,
            2,
            &mut timings,
        )
        .unwrap();

        assert_eq!(files.len(), 2);
        assert_eq!(symbols.len(), 2);
        assert!(occurrences.is_empty());
        assert!(!timings.steps.is_empty());
    }
}