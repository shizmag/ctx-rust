use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use walkdir::WalkDir;

use crate::error::CodeGraphError;
use crate::languages::rust::parse_rust_file;
use crate::model::{CallEdge, CodeIndex, Language, SourceFile, SymbolId, SymbolKind};
use crate::resolver::noop::resolve_name_only;
use crate::resolver::rust_analyzer_lsp::{LspClient, resolve_via_lsp};

#[derive(Debug, Clone)]
pub struct BuildIndexOptions {
    pub use_rust_analyzer: bool,
    pub max_depth: Option<usize>,
    pub include_tests: bool,
}

impl Default for BuildIndexOptions {
    fn default() -> Self {
        Self {
            use_rust_analyzer: false,
            max_depth: None,
            include_tests: true,
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

pub(crate) fn get_mtime_ms(path: &Path) -> Option<i64> {
    let metadata = std::fs::metadata(path).ok()?;
    let mtime = metadata.modified().ok()?;
    let duration = mtime.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(duration.as_millis() as i64)
}

pub(crate) fn get_size_bytes(path: &Path) -> Option<i64> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(metadata.len() as i64)
}

pub(crate) fn should_index_path(path: &Path) -> bool {
    for component in path.components() {
        if let Some(s) = component.as_os_str().to_str() {
            if s == "target" || s == ".git" || s == ".codegraph" || s == ".ctx-codegraph" {
                return false;
            }
        }
    }
    path.extension().map(|e| e == "rs").unwrap_or(false)
}

pub fn build_index(root: &Path, options: BuildIndexOptions) -> Result<CodeIndex, CodeGraphError> {
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
    let mut rust_files = Vec::new();
    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if should_index_path(path) {
            rust_files.push(path.to_path_buf());
        }
    }

    // Process each file
    for path in rust_files {
        let mtime_ms = get_mtime_ms(&path);
        let size_bytes = get_size_bytes(&path);
        let content_hash = compute_file_hash(&path);

        let source_file = SourceFile {
            id: None,
            path: path.clone(),
            language: Language::Rust,
            mtime_ms,
            size_bytes,
            content_hash,
        };
        files.push(source_file);

        let (mut file_symbols, mut file_call_sites) = match parse_rust_file(&path) {
            Ok(res) => res,
            Err(e) => {
                // If it fails to parse, we log / skip or return error. Let's make it robust: skip or turn into a warning.
                eprintln!("Warning: Failed to parse {}: {}", path.display(), e);
                continue;
            }
        };

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
    let mut lsp_client = if options.use_rust_analyzer {
        match LspClient::new(root) {
            Ok(client) => Some(client),
            Err(err) => {
                eprintln!(
                    "Warning: Failed to start rust-analyzer LSP: {}. Falling back to name-only resolution.",
                    err
                );
                None
            }
        }
    } else {
        None
    };

    if let Some(ref mut client) = lsp_client {
        if let Some(first_cs) = global_call_sites.first() {
            let start = std::time::Instant::now();
            let timeout = std::time::Duration::from_secs(15);
            let delay = std::time::Duration::from_millis(200);

            while start.elapsed() < timeout {
                let res = resolve_via_lsp(client, first_cs, &global_symbols);
                match res {
                    Err(err) if err.contains("-32603") || err.contains("file not found") => {
                        std::thread::sleep(delay);
                    }
                    Ok(None) if start.elapsed() < std::time::Duration::from_millis(5000) => {
                        std::thread::sleep(delay);
                    }
                    _ => {
                        break;
                    }
                }
            }
        }
    }

    for (call_site_idx, cs) in global_call_sites.iter().enumerate() {
        let from_id = match cs.from {
            Some(id) => id,
            None => continue,
        };

        let mut resolved_idx = None;
        let mut confidence = crate::model::ResolutionConfidence::Unresolved;

        if let Some(ref mut client) = lsp_client {
            match resolve_via_lsp(client, cs, &global_symbols) {
                Ok(Some(idx)) => {
                    resolved_idx = Some(idx);
                    confidence = crate::model::ResolutionConfidence::Exact;
                }
                Ok(None) => {}
                Err(err) => {
                    eprintln!(
                        "LSP resolution warning for call to {}: {}",
                        cs.raw_name, err
                    );
                }
            }
        }

        if resolved_idx.is_none() {
            let (fallback_idx, fallback_conf) = resolve_name_only(&cs.raw_name, &global_symbols);
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
