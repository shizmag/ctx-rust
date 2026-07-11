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
    canon_cache: Mutex<std::collections::HashMap<PathBuf, PathBuf>>,
    symbol_map: Mutex<Option<(usize, usize, std::collections::HashMap<PathBuf, Vec<usize>>)>>,
}

impl LspDefinitionResolver {
    pub fn new(config: LspServerConfig) -> Self {
        Self {
            config,
            client: Mutex::new(None),
            canon_cache: Mutex::new(std::collections::HashMap::new()),
            symbol_map: Mutex::new(None),
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
        let mut canon_cache_lock = self.canon_cache.lock().unwrap();
        let canon_cache = &mut *canon_cache_lock;

        let mut symbol_map_lock = self.symbol_map.lock().unwrap();
        let symbol_map = match &*symbol_map_lock {
            Some((ptr, len, map)) if *ptr == input.symbols.as_ptr() as usize && *len == input.symbols.len() => map,
            _ => {
                let mut map: std::collections::HashMap<PathBuf, Vec<usize>> = std::collections::HashMap::new();
                for (i, sym) in input.symbols.iter().enumerate() {
                    if sym.kind == SymbolKind::Impl {
                        continue;
                    }
                    let sym_canon = if let Some(canon) = canon_cache.get(&sym.file) {
                        canon.clone()
                    } else {
                        let canon = sym.file.canonicalize().unwrap_or_else(|_| sym.file.clone());
                        canon_cache.insert(sym.file.clone(), canon.clone());
                        canon
                    };
                    map.entry(sym_canon).or_default().push(i);
                }
                *symbol_map_lock = Some((input.symbols.as_ptr() as usize, input.symbols.len(), map));
                &symbol_map_lock.as_ref().unwrap().2
            }
        };

        let mut client_lock = self.client.lock().unwrap();
        let needs_new_client = match &*client_lock {
            Some((root, _)) => root != input.workspace_root,
            None => true,
        };

        let canon_file = if let Some(canon) = canon_cache.get(&input.occurrence.file) {
            canon.clone()
        } else {
            let canon = input.occurrence.file.canonicalize().unwrap_or_else(|_| input.occurrence.file.clone());
            canon_cache.insert(input.occurrence.file.clone(), canon.clone());
            canon
        };

        if needs_new_client {
            *client_lock = None;
            match GenericLspClient::new(
                input.workspace_root,
                self.config.command,
                self.config.args,
            ) {
                Ok(mut c) => {
                    let _ = c.ensure_document_open(&input.occurrence.file, &canon_file, &input.occurrence.language.0);
                    let start = std::time::Instant::now();
                    let timeout = Duration::from_secs(45);
                    let delay = Duration::from_millis(200);

                    while start.elapsed() < timeout {
                        let res = resolve_via_lsp(
                            &mut c,
                            input.occurrence,
                            input.symbols,
                            self.config.location_parser,
                            &canon_file,
                            canon_cache,
                            symbol_map,
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
                &canon_file,
                canon_cache,
                symbol_map,
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
    let sym_canon = if let Some(canon) = canon_cache.get(&sym.file) {
        canon
    } else {
        let canon = sym.file.canonicalize().unwrap_or_else(|_| sym.file.clone());
        canon_cache.insert(sym.file.clone(), canon);
        canon_cache.get(&sym.file).unwrap()
    };
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
    canon_cache: &mut std::collections::HashMap<PathBuf, PathBuf>,
    symbol_map: &std::collections::HashMap<PathBuf, Vec<usize>>,
) -> Option<usize> {
    let target_canon = if let Some(canon) = canon_cache.get(target_file) {
        canon.clone()
    } else {
        let canon = target_file.canonicalize().unwrap_or_else(|_| target_file.to_path_buf());
        canon_cache.insert(target_file.to_path_buf(), canon.clone());
        canon
    };
    let mut best_match: Option<(usize, usize)> = None;

    if let Some(indices) = symbol_map.get(&target_canon) {
        for &i in indices {
            let sym = &symbols[i];
            if matches_definition(
                sym,
                &target_canon,
                target_line_1,
                target_col_1,
                canon_cache,
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
    canon_file: &Path,
    canon_cache: &mut std::collections::HashMap<PathBuf, PathBuf>,
    symbol_map: &std::collections::HashMap<PathBuf, Vec<usize>>,
) -> Result<Option<usize>, String> {
    let _ = client.ensure_document_open(&occurrence.file, canon_file, &occurrence.language.0);
    let file_uri = format!("file://{}", canon_file.display());

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
                find_matching_symbol(symbols, &target_path, target_line, target_col, canon_cache, symbol_map)
        {
            return Ok(Some(sym_idx));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_codegraph_lang::model::{Language, SymbolKind};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn sample_range(line: usize) -> TextRange {
        TextRange {
            start_line: line,
            start_col: 1,
            end_line: line,
            end_col: 20,
        }
    }

    fn make_symbol(
        name: &str,
        file: &Path,
        range: TextRange,
        kind: SymbolKind,
        body_range: Option<TextRange>,
    ) -> Symbol {
        Symbol {
            id: None,
            file_id: None,
            name: name.to_string(),
            qualified_name: name.to_string(),
            kind,
            language: Language::rust(),
            file: file.to_path_buf(),
            range,
            body_range,
        }
    }

    #[test]
    fn is_inside_range_respects_bounds() {
        let range = TextRange {
            start_line: 2,
            start_col: 3,
            end_line: 4,
            end_col: 10,
        };
        assert!(!is_inside_range(&range, 1, 3));
        assert!(is_inside_range(&range, 2, 3));
        assert!(is_inside_range(&range, 3, 1));
        assert!(!is_inside_range(&range, 2, 2));
        assert!(!is_inside_range(&range, 4, 11));
        assert!(!is_inside_range(&range, 5, 1));
    }

    #[test]
    fn parse_location_standard_and_extended_variants() {
        let loc = serde_json::json!({
            "uri": "file:///tmp/foo.rs",
            "range": { "start": { "line": 4, "character": 4 }, "end": { "line": 4, "character": 8 } }
        });
        let (path, line, col) = parse_location(&loc, LocationParser::Standard).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/foo.rs"));
        assert_eq!((line, col), (5, 5));

        let extended = serde_json::json!({
            "targetUri": "file:///tmp/bar.py",
            "targetSelectionRange": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 3 }
            }
        });
        let (path, line, col) = parse_location(&extended, LocationParser::Extended).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/bar.py"));
        assert_eq!((line, col), (2, 1));
    }

    #[test]
    fn parse_location_rejects_non_file_uris_and_missing_fields() {
        let http_uri = serde_json::json!({
            "uri": "https://example.com/foo.rs",
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } }
        });
        assert!(parse_location(&http_uri, LocationParser::Standard).is_none());

        let missing_range = serde_json::json!({ "uri": "file:///tmp/x.rs" });
        assert!(parse_location(&missing_range, LocationParser::Standard).is_none());
    }

    #[test]
    fn find_matching_symbol_skips_impl_and_body_range() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("lib.rs");
        std::fs::write(&file, "fn outer() {}\nfn inner() {}\n").unwrap();

        let outer = make_symbol(
            "outer",
            &file,
            sample_range(1),
            SymbolKind::Function,
            Some(TextRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 15,
            }),
        );
        let inner = make_symbol(
            "inner",
            &file,
            sample_range(2),
            SymbolKind::Function,
            None,
        );
        let impl_sym = make_symbol(
            "Outer",
            &file,
            sample_range(1),
            SymbolKind::Impl,
            None,
        );

        // Target inside outer body should not match outer declaration.
        let canon = file.canonicalize().unwrap();
        let mut symbol_map = std::collections::HashMap::new();
        symbol_map.insert(canon.clone(), vec![0, 2]);
        assert!(find_matching_symbol(
            &[outer.clone(), impl_sym.clone(), inner.clone()],
            &file,
            1,
            10,
            &mut std::collections::HashMap::new(),
            &symbol_map
        )
        .is_none());
        assert_eq!(
            find_matching_symbol(
                &[outer, impl_sym, inner],
                &file,
                2,
                1,
                &mut std::collections::HashMap::new(),
                &symbol_map
            ),
            Some(2)
        );
    }

    #[test]
    fn find_matching_symbol_prefers_smaller_range() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("lib.rs");
        std::fs::write(&file, "fn a() {}\n").unwrap();

        let wide = make_symbol(
            "wide",
            &file,
            TextRange {
                start_line: 1,
                start_col: 1,
                end_line: 3,
                end_col: 1,
            },
            SymbolKind::Function,
            None,
        );
        let narrow = make_symbol(
            "narrow",
            &file,
            TextRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 5,
            },
            SymbolKind::Function,
            None,
        );

        let canon = file.canonicalize().unwrap();
        let mut symbol_map = std::collections::HashMap::new();
        symbol_map.insert(canon, vec![0, 1]);
        assert_eq!(
            find_matching_symbol(&[wide, narrow], &file, 1, 2, &mut std::collections::HashMap::new(), &symbol_map),
            Some(1)
        );
    }

    #[test]
    fn matches_definition_rejects_out_of_range_columns() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("lib.rs");
        std::fs::write(&file, "fn demo() {}\n").unwrap();
        let sym = make_symbol(
            "demo",
            &file,
            TextRange {
                start_line: 1,
                start_col: 4,
                end_line: 1,
                end_col: 8,
            },
            SymbolKind::Function,
            None,
        );
        let mut cache = std::collections::HashMap::new();
        let canon = file.canonicalize().unwrap();

        assert!(matches_definition(&sym, &canon, 1, 4, &mut cache));
        assert!(!matches_definition(&sym, &canon, 1, 3, &mut cache));
        assert!(!matches_definition(&sym, &canon, 1, 9, &mut cache));
    }

    #[test]
    fn find_matching_symbol_uses_canon_cache() {
        let file = PathBuf::from("nonexistent_file_a.rs");
        let fake_canon = PathBuf::from("fake_canonical_a.rs");

        let sym = make_symbol(
            "demo",
            &file,
            sample_range(1),
            SymbolKind::Function,
            None,
        );

        let mut cache = std::collections::HashMap::new();
        cache.insert(file.clone(), fake_canon.clone());

        let mut symbol_map = std::collections::HashMap::new();
        symbol_map.insert(fake_canon.clone(), vec![0]);
        let matched = find_matching_symbol(
            &[sym],
            &fake_canon,
            1,
            5,
            &mut cache,
            &symbol_map,
        );

        assert_eq!(matched, Some(0));
    }
}