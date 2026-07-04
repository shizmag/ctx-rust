use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::backend::{BackendRegistry, ParseInput, ResolveInput, global_registry};
use crate::error::CodeGraphError;
use crate::model::{CallEdge, CodeIndex, FileParseStatus, FileSnapshot, SymbolId, SymbolKind};
use crate::resolver::noop::resolve_name_only;

#[derive(Debug, Clone)]
pub struct BuildIndexOptions {
    pub use_lsp: bool,
    pub max_depth: Option<usize>,
    pub include_tests: bool,
    pub change_detection: crate::model::FileChangeDetection,
}

impl Default for BuildIndexOptions {
    fn default() -> Self {
        Self {
            use_lsp: false,
            max_depth: None,
            include_tests: true,
            change_detection: crate::model::FileChangeDetection::MtimeAndSize,
        }
    }
}

pub(crate) fn compute_file_hash(path: &Path) -> Option<String> {
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
) -> FileSnapshot {
    create_file_snapshot_with_registry(
        workspace_root,
        abs_path,
        change_detection,
        include_tests,
        global_registry(),
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
    });

    FileSnapshot {
        file_id: None,
        rel_path,
        abs_path: abs_path.to_path_buf(),
        language: backend.language().clone(),
        backend_id: backend.id().0.clone(),
        size_bytes,
        mtime_ms,
        mtime_ns: None,
        content_hash,
        parser_id: parser.parser_id().0.clone(),
        parser_version: parser.parser_version(),
        parser_config_hash,
        indexed_at_ms: None,
        parse_status: FileParseStatus::Success,
    }
}

pub(crate) fn should_index_path(path: &Path) -> bool {
    should_index_path_with_registry(path, global_registry())
}

pub(crate) fn should_index_path_with_registry(path: &Path, registry: &BackendRegistry) -> bool {
    for component in path.components() {
        if let Some(s) = component.as_os_str().to_str() {
            if s == "target" || s == ".git" || s == ".codegraph" || s == ".ctx-codegraph" {
                return false;
            }
        }
    }
    registry.find_by_path(path).is_some()
}

pub fn build_index(root: &Path, options: BuildIndexOptions) -> Result<CodeIndex, CodeGraphError> {
    build_index_with_registry(root, options, global_registry())
}

pub fn build_index_with_registry(
    root: &Path,
    options: BuildIndexOptions,
    registry: &BackendRegistry,
) -> Result<CodeIndex, CodeGraphError> {
    let mut files = Vec::new();
    let mut global_symbols = Vec::new();
    let mut global_call_sites = Vec::new();

    // Find files
    let walker = WalkDir::new(root).into_iter().filter_entry(|e| {
        let path = e.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name == "target"
                    || name == ".git"
                    || name == ".codegraph"
                    || name == ".ctx-codegraph"
                {
                    return false;
                }
            }
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

    // Process each file
    for path in matching_files {
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

        let (mut file_symbols, mut file_call_sites) = (parsed.symbols, parsed.call_sites);

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

            file_call_sites.retain(|cs| {
                if let Some(old_idx) = cs.from_temp_index {
                    index_map.contains_key(&old_idx)
                } else {
                    true
                }
            });

            for cs in &mut file_call_sites {
                if let Some(ref mut idx) = cs.from_temp_index {
                    if let Some(&new_idx) = index_map.get(idx) {
                        *idx = new_idx;
                    }
                }
            }
        }

        let start_sym_idx = global_symbols.len();
        for cs in &mut file_call_sites {
            if let Some(ref mut idx) = cs.from_temp_index {
                *idx += start_sym_idx;
            }
        }

        global_symbols.extend(file_symbols);
        global_call_sites.extend(file_call_sites);
    }

    // Set temporary symbol IDs
    for (i, sym) in global_symbols.iter_mut().enumerate() {
        sym.id = Some(SymbolId(i as i64));
    }

    // Set temporary from IDs on call sites
    for cs in &mut global_call_sites {
        if let Some(from_idx) = cs.from_temp_index {
            cs.from = Some(SymbolId(from_idx as i64));
        }
    }

    // Resolve call sites
    let mut edges = Vec::new();

    for (call_site_idx, cs) in global_call_sites.iter().enumerate() {
        let from_id = match cs.from {
            Some(id) => id,
            None => continue,
        };

        let mut resolved_idx = None;
        let mut confidence = crate::model::ResolutionConfidence::Unresolved;

        let backend = registry.find_by_path(&cs.file);
        let resolver = backend.and_then(|b| b.resolver());

        if options.use_lsp {
            if let Some(res) = resolver {
                let resolve_input = ResolveInput {
                    workspace_root: root,
                    call_site: cs,
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
                            cs.raw_name, err
                        );
                    }
                }
            }
        }

        if resolved_idx.is_none() {
            let (fallback_idx, fallback_conf) =
                resolve_name_only(&cs.raw_name, &global_symbols, &cs.file);
            resolved_idx = fallback_idx;
            confidence = fallback_conf;
        }

        let edge = CallEdge {
            from: from_id,
            to: resolved_idx.map(|idx| SymbolId(idx as i64)),
            call_site_id: Some(crate::model::CallId(call_site_idx as i64)),
            raw_name: cs.raw_name.clone(),
            call_range: cs.range.clone(),
            confidence,
        };
        edges.push(edge);
    }

    Ok(CodeIndex {
        root: root.to_path_buf(),
        files,
        symbols: global_symbols,
        call_sites: global_call_sites,
        edges,
    })
}
