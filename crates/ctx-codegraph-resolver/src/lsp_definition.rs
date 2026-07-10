use ctx_codegraph_lang::backend::{
    ResolveInput, ResolveOutput, ResolverBackend, ResolverId,
};
use ctx_codegraph_lang::error::CodeGraphError;
use ctx_codegraph_lang::model::{
    Occurrence, ResolutionConfidence, Symbol, SymbolKind, TextRange,
};
use ctx_codegraph_lang::noop::{parse_raw_name, resolve_name_only_occurrence};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use crate::lsp_transport::GenericLspClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationParser {
    Standard,
    Extended,
}

pub struct LspServerConfig {
    pub resolver_id: ResolverId,
    pub version: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub location_parser: LocationParser,
}

pub struct LspDefinitionResolver {
    config: LspServerConfig,
    client: Mutex<Option<(PathBuf, GenericLspClient)>>,
}

impl LspDefinitionResolver {
    pub fn new(config: LspServerConfig) -> Self {
        Self {
            config,
            client: Mutex::new(None),
        }
    }

    pub fn rust() -> Self {
        Self::new(LspServerConfig {
            resolver_id: ResolverId::new("rust-analyzer-lsp"),
            version: "0.1.0",
            command: "rust-analyzer",
            args: &[],
            location_parser: LocationParser::Standard,
        })
    }

    pub fn python() -> Self {
        Self::new(LspServerConfig {
            resolver_id: ResolverId::new("pyright-lsp"),
            version: "0.1.0",
            command: "pyright-langserver",
            args: &["--stdio"],
            location_parser: LocationParser::Extended,
        })
    }
}

impl ResolverBackend for LspDefinitionResolver {
    fn resolver_id(&self) -> ResolverId {
        self.config.resolver_id.clone()
    }

    fn resolver_version(&self) -> String {
        self.config.version.to_string()
    }

    fn resolve(&self, input: ResolveInput<'_>) -> Result<ResolveOutput, CodeGraphError> {
        let mut client_lock = self.client.lock().unwrap();
        let needs_new_client = match &*client_lock {
            Some((root, _)) => root != input.workspace_root,
            None => true,
        };

        if needs_new_client {
            *client_lock = None;
            match GenericLspClient::new(
                input.workspace_root,
                self.config.command,
                self.config.args,
            ) {
                Ok(mut c) => {
                    let start = std::time::Instant::now();
                    let timeout = Duration::from_secs(45);
                    let delay = Duration::from_millis(200);

                    while start.elapsed() < timeout {
                        let res = resolve_via_lsp(
                            &mut c,
                            input.occurrence,
                            input.symbols,
                            self.config.location_parser,
                        );
                        match res {
                            Err(err)
                                if err.contains("-32603") || err.contains("file not found") =>
                            {
                                std::thread::sleep(delay);
                            }
                            Ok(None)
                                if start.elapsed() < Duration::from_millis(5000) =>
                            {
                                std::thread::sleep(delay);
                            }
                            _ => break,
                        }
                    }
                    *client_lock = Some((input.workspace_root.to_path_buf(), c));
                }
                Err(err) => {
                    eprintln!(
                        "Warning: Failed to start {}: {}. Falling back to name-only resolution.",
                        self.config.command, err
                    );
                }
            }
        }

        let mut resolved_symbol_index = None;
        let mut confidence = ResolutionConfidence::Unresolved;

        if let Some((_, ref mut client)) = *client_lock {
            match resolve_via_lsp(
                client,
                input.occurrence,
                input.symbols,
                self.config.location_parser,
            ) {
                Ok(Some(idx)) => {
                    resolved_symbol_index = Some(idx);
                    confidence = ResolutionConfidence::LspExact;
                }
                Ok(None) => {}
                Err(err) => {
                    eprintln!(
                        "LSP resolution warning for call to {}: {}",
                        input.occurrence.raw_text, err
                    );
                }
            }
        }

        if resolved_symbol_index.is_none() {
            let (fallback_idx, fallback_conf) =
                resolve_name_only_occurrence(input.occurrence, input.symbols);
            resolved_symbol_index = fallback_idx;
            confidence = fallback_conf;
        }

        Ok(ResolveOutput {
            resolved_symbol_index,
            confidence,
        })
    }
}

fn is_inside_range(range: &TextRange, line: usize, col: usize) -> bool {
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

    if let Some(ref body) = sym.body_range
        && is_inside_range(body, target_line_1, target_col_1)
    {
        return false;
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

fn parse_location(loc: &serde_json::Value, parser: LocationParser) -> Option<(PathBuf, usize, usize)> {
    let uri_str = match parser {
        LocationParser::Standard => loc.get("uri")?.as_str()?,
        LocationParser::Extended => loc
            .get("uri")
            .or_else(|| loc.get("targetUri"))?
            .as_str()?,
    };
    if !uri_str.starts_with("file://") {
        return None;
    }
    let path = PathBuf::from(&uri_str["file://".len()..]);
    let range_val = match parser {
        LocationParser::Standard => loc.get("range")?,
        LocationParser::Extended => loc
            .get("range")
            .or_else(|| loc.get("targetSelectionRange"))
            .or_else(|| loc.get("targetRange"))?,
    };
    let start_pos = range_val.get("start")?;
    let line = start_pos.get("line")?.as_u64()? as usize + 1;
    let col = start_pos.get("character")?.as_u64()? as usize + 1;
    Some((path, line, col))
}

fn resolve_via_lsp(
    client: &mut GenericLspClient,
    occurrence: &Occurrence,
    symbols: &[Symbol],
    location_parser: LocationParser,
) -> Result<Option<usize>, String> {
    let file_uri = format!(
        "file://{}",
        occurrence
            .file
            .canonicalize()
            .unwrap_or_else(|_| occurrence.file.clone())
            .display()
    );

    let name_segment = parse_raw_name(&occurrence.raw_text);
    let offset = occurrence.raw_text.len() - name_segment.len();

    let params = serde_json::json!({
        "textDocument": {
            "uri": file_uri
        },
        "position": {
            "line": occurrence.range.start_line - 1,
            "character": occurrence.range.start_col - 1 + offset
        }
    });

    let resp = client.request(
        "textDocument/definition",
        params,
        Duration::from_millis(5000),
    )?;

    let locations = if resp.is_array() {
        resp.as_array().unwrap().clone()
    } else if resp.is_object() {
        vec![resp]
    } else {
        return Ok(None);
    };

    for loc in locations {
        if let Some((target_path, target_line, target_col)) = parse_location(&loc, location_parser)
            && let Some(sym_idx) =
                find_matching_symbol(symbols, &target_path, target_line, target_col)
        {
            return Ok(Some(sym_idx));
        }
    }

    Ok(None)
}