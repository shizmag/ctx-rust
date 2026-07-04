use crate::backend::{ResolveInput, ResolveOutput, ResolverBackend, ResolverId};
use crate::error::CodeGraphError;
use crate::model::{CallSite, ResolutionConfidence, Symbol, SymbolKind};
use crate::resolver::lsp_transport::GenericLspClient;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub struct RustResolver {
    client: Mutex<Option<(PathBuf, GenericLspClient)>>,
}

impl RustResolver {
    pub fn new() -> Self {
        Self {
            client: Mutex::new(None),
        }
    }
}

impl ResolverBackend for RustResolver {
    fn resolver_id(&self) -> ResolverId {
        ResolverId("rust-analyzer-lsp".to_string())
    }

    fn resolver_version(&self) -> String {
        "0.1.0".to_string()
    }

    fn resolve(&self, input: ResolveInput<'_>) -> Result<ResolveOutput, CodeGraphError> {
        let mut client_lock = self.client.lock().unwrap();
        let needs_new_client = match &*client_lock {
            Some((root, _)) => root != input.workspace_root,
            None => true,
        };

        if needs_new_client {
            *client_lock = None;
            match GenericLspClient::new(input.workspace_root, "rust-analyzer", &[]) {
                Ok(mut c) => {
                    // Warm up loop to wait for initialization
                    let start = std::time::Instant::now();
                    let timeout = std::time::Duration::from_secs(45);
                    let delay = std::time::Duration::from_millis(200);

                    while start.elapsed() < timeout {
                        let res = resolve_via_lsp(&mut c, input.call_site, input.symbols);
                        match res {
                            Err(err)
                                if err.contains("-32603") || err.contains("file not found") =>
                            {
                                std::thread::sleep(delay);
                            }
                            Ok(None)
                                if start.elapsed() < std::time::Duration::from_millis(5000) =>
                            {
                                std::thread::sleep(delay);
                            }
                            _ => {
                                break;
                            }
                        }
                    }
                    *client_lock = Some((input.workspace_root.to_path_buf(), c));
                }
                Err(err) => {
                    eprintln!(
                        "Warning: Failed to start rust-analyzer LSP: {}. Falling back to name-only resolution.",
                        err
                    );
                }
            }
        }

        let mut resolved_symbol_index = None;
        let mut confidence = ResolutionConfidence::Unresolved;

        if let Some((_, ref mut client)) = *client_lock {
            match resolve_via_lsp(client, input.call_site, input.symbols) {
                Ok(Some(idx)) => {
                    resolved_symbol_index = Some(idx);
                    confidence = ResolutionConfidence::LspExact;
                }
                Ok(None) => {}
                Err(err) => {
                    eprintln!(
                        "LSP resolution warning for call to {}: {}",
                        input.call_site.raw_name, err
                    );
                }
            }
        }

        if resolved_symbol_index.is_none() {
            let (fallback_idx, fallback_conf) = crate::resolver::noop::resolve_name_only(
                &input.call_site.raw_name,
                input.symbols,
                &input.call_site.file,
            );
            resolved_symbol_index = fallback_idx;
            confidence = fallback_conf;
        }

        Ok(ResolveOutput {
            resolved_symbol_index,
            confidence,
        })
    }
}

fn is_inside_range(range: &crate::model::TextRange, line: usize, col: usize) -> bool {
    if line < range.start_line || line > range.end_line {
        return false;
    }
    if line == range.start_line && col < range.start_col {
        return false;
    }
    if line == range.end_line && col > range.end_col {
        return false;
    }
    true
}

fn matches_definition(
    sym: &Symbol,
    target_canon: &Path,
    target_line_1: usize,
    target_col_1: usize,
    canon_cache: &mut std::collections::HashMap<PathBuf, PathBuf>,
) -> bool {
    let sym_canon = canon_cache
        .entry(sym.file.clone())
        .or_insert_with(|| sym.file.canonicalize().unwrap_or_else(|_| sym.file.clone()));
    if sym_canon != target_canon {
        return false;
    }

    let start_line = sym.range.start_line;
    let end_line = sym.range.end_line;

    if target_line_1 < start_line || target_line_1 > end_line {
        return false;
    }

    if target_line_1 == start_line && target_col_1 < sym.range.start_col {
        return false;
    }

    if target_line_1 == end_line && target_col_1 > sym.range.end_col {
        return false;
    }

    if let Some(ref body) = sym.body_range {
        if is_inside_range(body, target_line_1, target_col_1) {
            return false;
        }
    }

    true
}

fn find_matching_symbol(
    symbols: &[Symbol],
    target_file: &Path,
    target_line_1: usize,
    target_col_1: usize,
) -> Option<usize> {
    let target_canon = target_file
        .canonicalize()
        .unwrap_or_else(|_| target_file.to_path_buf());
    let mut best_match: Option<(usize, usize)> = None;
    let mut canon_cache = std::collections::HashMap::new();

    for (i, sym) in symbols.iter().enumerate() {
        if sym.kind == SymbolKind::Impl {
            continue;
        }
        if matches_definition(
            sym,
            &target_canon,
            target_line_1,
            target_col_1,
            &mut canon_cache,
        ) {
            let range_size = sym.range.end_line - sym.range.start_line;
            match best_match {
                None => best_match = Some((i, range_size)),
                Some((_, best_size)) => {
                    if range_size < best_size {
                        best_match = Some((i, range_size));
                    }
                }
            }
        }
    }

    best_match.map(|(i, _)| i)
}

fn resolve_via_lsp(
    client: &mut GenericLspClient,
    call_site: &CallSite,
    symbols: &[Symbol],
) -> Result<Option<usize>, String> {
    let file_uri = format!(
        "file://{}",
        call_site
            .file
            .canonicalize()
            .unwrap_or_else(|_| call_site.file.clone())
            .display()
    );

    let name_segment = crate::resolver::noop::parse_raw_name(&call_site.raw_name);
    let offset = call_site.raw_name.len() - name_segment.len();

    let params = serde_json::json!({
        "textDocument": {
            "uri": file_uri
        },
        "position": {
            "line": call_site.range.start_line - 1,
            "character": call_site.range.start_col - 1 + offset
        }
    });

    let resp = client.request(
        "textDocument/definition",
        params,
        Duration::from_millis(5000),
    )?;

    let parse_location = |loc: &serde_json::Value| -> Option<(PathBuf, usize, usize)> {
        let uri_str = loc.get("uri")?.as_str()?;
        if !uri_str.starts_with("file://") {
            return None;
        }
        let path = PathBuf::from(&uri_str["file://".len()..]);
        let start_pos = loc.get("range")?.get("start")?;
        let line = start_pos.get("line")?.as_u64()? as usize + 1;
        let col = start_pos.get("character")?.as_u64()? as usize + 1;
        Some((path, line, col))
    };

    let locations = if resp.is_array() {
        resp.as_array().unwrap().clone()
    } else if resp.is_object() {
        vec![resp]
    } else {
        return Ok(None);
    };

    for loc in locations {
        if let Some((target_path, target_line, target_col)) = parse_location(&loc) {
            if let Some(sym_idx) =
                find_matching_symbol(symbols, &target_path, target_line, target_col)
            {
                return Ok(Some(sym_idx));
            }
        }
    }

    Ok(None)
}
