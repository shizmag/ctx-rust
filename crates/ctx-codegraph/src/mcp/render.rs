use crate::model::{
    GraphContextMode, GraphContextResult, LanguageObject, LanguageObjectKind, Symbol,
    SymbolResolution,
};
use std::path::Path;

pub fn kind_to_str(kind: LanguageObjectKind) -> &'static str {
    match kind {
        LanguageObjectKind::Function => "fn",
        LanguageObjectKind::Method => "method",
        LanguageObjectKind::Struct => "struct",
        LanguageObjectKind::Enum => "enum",
        LanguageObjectKind::Trait => "trait",
        LanguageObjectKind::Impl => "impl",
        LanguageObjectKind::Module => "mod",
        LanguageObjectKind::Class => "class",
        LanguageObjectKind::Interface => "interface",
        LanguageObjectKind::TypeAlias => "type",
        LanguageObjectKind::Constant => "const",
        LanguageObjectKind::Variable => "var",
        LanguageObjectKind::Unknown => "unknown",
    }
}

pub fn format_symbol_line(obj: &LanguageObject, root_path: &Path) -> String {
    let kind = kind_to_str(obj.kind);
    let rel_path = obj
        .file_path
        .strip_prefix(root_path)
        .unwrap_or(&obj.file_path);
    format!(
        "- {} {} — {}:{}-{}",
        kind,
        obj.qualified_name,
        rel_path.display(),
        obj.range.start_line,
        obj.range.end_line
    )
}

pub fn format_ambiguous_symbols(query: &str, candidates: &[LanguageObject]) -> String {
    let mut msg = format!(
        "Multiple symbols found matching query: '{}'. Please be more specific:\n",
        query
    );
    for c in candidates {
        let kind_str = kind_to_str(c.kind);
        let rel_path = c.file_path.display();
        msg.push_str(&format!("- {} {} in {}\n", kind_str, c.qualified_name, rel_path));
    }
    msg
}

pub fn format_symbol_not_found(query: &str) -> String {
    format!("Error: Symbol not found for query '{}'", query)
}

pub fn handle_symbol_resolution<F>(
    query: &str,
    resolution: SymbolResolution,
    on_unique: F,
) -> Result<String, String>
where
    F: FnOnce(LanguageObject) -> Result<String, String>,
{
    match resolution {
        SymbolResolution::Unique(obj) => on_unique(obj),
        SymbolResolution::Ambiguous(candidates) => {
            Ok(format_ambiguous_symbols(query, &candidates))
        }
        SymbolResolution::NotFound => Ok(format_symbol_not_found(query)),
    }
}

pub fn render_context_to_markdown(
    result: &GraphContextResult,
    root_path: &Path,
    mode: GraphContextMode,
    depth: usize,
    max_nodes: usize,
    max_files: usize,
) -> String {
    let mut out = String::new();

    out.push_str("# Graph Context\n\n");
    let root_kind = kind_to_str(result.root.kind);
    let root_rel_path = result
        .root
        .file_path
        .strip_prefix(root_path)
        .unwrap_or(&result.root.file_path);
    out.push_str(&format!("Root: {} {}\n", root_kind, result.root.name));
    out.push_str(&format!(
        "Path: {}:{}-{}\n",
        root_rel_path.display(),
        result.root.range.start_line,
        result.root.range.end_line
    ));
    out.push_str(&format!("Mode: {:?}\n", mode));
    out.push_str(&format!("Depth: {}\n", depth));
    out.push_str(&format!("Max nodes: {}\n", max_nodes));
    out.push_str(&format!("Max files: {}\n\n", max_files));

    out.push_str("## Graph\n\n");
    let mut symbol_names = std::collections::HashMap::new();
    symbol_names.insert(result.root.id, result.root.qualified_name.clone());
    for node in &result.nodes {
        symbol_names.insert(node.id, node.qualified_name.clone());
    }

    let mut edge_lines = Vec::new();
    for edge in &result.edges {
        let from_name = symbol_names
            .get(&edge.from)
            .cloned()
            .unwrap_or_else(|| format!("unknown_{:?}", edge.from));
        let to_name = symbol_names
            .get(&edge.to)
            .cloned()
            .unwrap_or_else(|| format!("unknown_{:?}", edge.to));
        edge_lines.push(format!("{} -> {}", from_name, to_name));
    }
    edge_lines.sort();
    for line in edge_lines {
        out.push_str(&line);
        out.push('\n');
    }
    out.push('\n');

    out.push_str("## Included Symbols\n\n");
    let mut symbols_list = Vec::new();
    symbols_list.push(format_symbol_line(&result.root, root_path));
    for node in &result.nodes {
        symbols_list.push(format_symbol_line(node, root_path));
    }
    symbols_list.sort();
    for sym_line in symbols_list {
        out.push_str(&sym_line);
        out.push('\n');
    }
    out.push('\n');

    out.push_str("## Files\n\n");

    let mut sorted_files = result.files.clone();
    sorted_files.sort_by(|a, b| match a.file_path.cmp(&b.file_path) {
        std::cmp::Ordering::Equal => a.range.start_line.cmp(&b.range.start_line),
        other => other,
    });

    let file_limit = if max_files == 0 {
        sorted_files.len()
    } else {
        max_files
    };

    for file_span in sorted_files.iter().take(file_limit) {
        let rel_path = file_span
            .file_path
            .strip_prefix(root_path)
            .unwrap_or(&file_span.file_path);
        out.push_str(&format!(
            "### {}:{}-{}\n\n",
            rel_path.display(),
            file_span.range.start_line,
            file_span.range.end_line
        ));

        let content = read_file_span(file_span.file_path.as_path(), &file_span.range);

        let lang = match file_span.file_path.extension().and_then(|ext| ext.to_str()) {
            Some("rs") => "rust",
            Some("py") => "python",
            Some("js") => "javascript",
            Some("ts") => "typescript",
            Some("tsx") => "tsx",
            Some("jsx") => "jsx",
            _ => "",
        };
        out.push_str(&format!("```{}\n", lang));
        out.push_str(&content);
        if !content.ends_with('\n') && !content.is_empty() {
            out.push('\n');
        }
        out.push_str("```\n\n");
    }

    if sorted_files.len() > file_limit {
        out.push_str(&format!(
            "_({} additional files omitted due to max_files limit)_\n",
            sorted_files.len() - file_limit
        ));
    }

    out
}

pub fn render_symbols_list(symbols: &[LanguageObject], root_path: &Path) -> String {
    let mut out = String::new();
    out.push_str("# Symbols\n\n");
    if symbols.is_empty() {
        out.push_str("No symbols found.\n");
        return out;
    }
    for sym in symbols {
        out.push_str(&format_symbol_line(sym, root_path));
        out.push('\n');
    }
    out
}

pub fn render_call_edges(
    title: &str,
    root: &LanguageObject,
    edges: &[(crate::model::CallEdge, Option<Symbol>)],
    root_path: &Path,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", title));
    out.push_str(&format!(
        "Symbol: {} {}\n\n",
        kind_to_str(root.kind),
        root.qualified_name
    ));

    if edges.is_empty() {
        out.push_str("No relationships found.\n");
        return out;
    }

    for (edge, target) in edges {
        let confidence = edge.confidence.as_str();
        match target {
            Some(sym) => {
                let rel_path = sym.file.strip_prefix(root_path).unwrap_or(&sym.file);
                out.push_str(&format!(
                    "- {} ({}) at {}:{}-{}\n",
                    sym.qualified_name,
                    confidence,
                    rel_path.display(),
                    sym.range.start_line,
                    sym.range.end_line
                ));
            }
            None => {
                out.push_str(&format!(
                    "- <unresolved> ({}) label: {}\n",
                    confidence,
                    edge.raw_text.as_deref().unwrap_or("<unknown>")
                ));
            }
        }
    }
    out
}

pub fn render_caller_edges(
    title: &str,
    root: &LanguageObject,
    edges: &[(crate::model::CallEdge, Symbol)],
    root_path: &Path,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", title));
    out.push_str(&format!(
        "Symbol: {} {}\n\n",
        kind_to_str(root.kind),
        root.qualified_name
    ));

    if edges.is_empty() {
        out.push_str("No relationships found.\n");
        return out;
    }

    for (edge, sym) in edges {
        let rel_path = sym.file.strip_prefix(root_path).unwrap_or(&sym.file);
        out.push_str(&format!(
            "- {} ({}) at {}:{}-{}\n",
            sym.qualified_name,
            edge.confidence.as_str(),
            rel_path.display(),
            sym.range.start_line,
            sym.range.end_line
        ));
    }
    out
}

pub fn render_affected_context_text(pack: &crate::ContextPack) -> String {
    let mut out = String::new();
    for section in &pack.sections {
        out.push_str(&section.text);
    }
    out
}

fn read_file_span(path: &Path, range: &crate::model::SourceRange) -> String {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            if range.start_line == 0 || range.start_line > lines.len() {
                String::new()
            } else {
                let end = std::cmp::min(range.end_line, lines.len());
                if range.start_line > end {
                    String::new()
                } else {
                    lines[(range.start_line - 1)..end]
                        .iter()
                        .fold(String::new(), |mut acc, line| {
                            acc.push_str(line);
                            acc.push('\n');
                            acc
                        })
                }
            }
        }
        Err(e) => format!("Error reading file: {}\n", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ContextPack, ContextSection, ContextSectionKind};
    use crate::model::{
        CallEdge, ContextFileSpan, EdgeKind, GraphContextEdge, GraphContextMode,
        GraphContextResult, LanguageId, ResolutionConfidence, SourceRange, Symbol, SymbolId,
        SymbolKind, TextRange,
    };
    use std::path::PathBuf;

    fn sample_language_object(id: i64, name: &str, rel_path: &str) -> LanguageObject {
        LanguageObject {
            id: SymbolId(id),
            name: name.to_string(),
            qualified_name: format!("lib::{}", name),
            kind: LanguageObjectKind::Function,
            file_path: PathBuf::from("/project").join(rel_path),
            range: SourceRange {
                start_line: 1,
                start_col: 0,
                end_line: 3,
                end_col: 1,
            },
            signature: None,
            language: Some("rust".to_string()),
        }
    }

    fn sample_symbol(name: &str, rel_path: &str) -> Symbol {
        Symbol {
            id: Some(SymbolId(99)),
            file_id: None,
            name: name.to_string(),
            qualified_name: format!("lib::{}", name),
            kind: SymbolKind::Function,
            language: LanguageId::rust(),
            file: PathBuf::from("/project").join(rel_path),
            range: TextRange {
                start_line: 10,
                start_col: 0,
                end_line: 12,
                end_col: 1,
            },
            body_range: None,
        }
    }

    fn sample_call_edge(raw_text: Option<&str>) -> CallEdge {
        CallEdge {
            id: None,
            kind: EdgeKind::Call,
            from_file_id: None,
            from_symbol_id: Some(SymbolId(1)),
            to_symbol_id: Some(SymbolId(2)),
            to_external: None,
            occurrence_id: None,
            raw_text: raw_text.map(|s| s.to_string()),
            range: None,
            confidence: ResolutionConfidence::Syntax,
            produced_by: None,
        }
    }

    #[test]
    fn kind_to_str_maps_known_kinds() {
        assert_eq!(kind_to_str(LanguageObjectKind::Function), "fn");
        assert_eq!(kind_to_str(LanguageObjectKind::Struct), "struct");
        assert_eq!(kind_to_str(LanguageObjectKind::Unknown), "unknown");
    }

    #[test]
    fn format_symbol_line_uses_relative_path() {
        let root = PathBuf::from("/project");
        let obj = sample_language_object(1, "run", "src/lib.rs");
        let line = format_symbol_line(&obj, &root);
        assert!(line.contains("fn lib::run"));
        assert!(line.contains("src/lib.rs:1-3"));
    }

    #[test]
    fn format_ambiguous_symbols_lists_candidates() {
        let candidates = vec![
            sample_language_object(1, "foo", "src/a.rs"),
            sample_language_object(2, "foo", "src/b.rs"),
        ];
        let text = format_ambiguous_symbols("foo", &candidates);
        assert!(text.contains("Multiple symbols found matching query: 'foo'"));
        assert!(text.contains("lib::foo"));
    }

    #[test]
    fn format_symbol_not_found_includes_query() {
        assert_eq!(
            format_symbol_not_found("missing"),
            "Error: Symbol not found for query 'missing'"
        );
    }

    #[test]
    fn handle_symbol_resolution_branches() {
        let unique = handle_symbol_resolution(
            "run",
            SymbolResolution::Unique(sample_language_object(1, "run", "src/lib.rs")),
            |obj| Ok(obj.name),
        )
        .unwrap();
        assert_eq!(unique, "run");

        let ambiguous = handle_symbol_resolution(
            "foo",
            SymbolResolution::Ambiguous(vec![sample_language_object(1, "foo", "src/a.rs")]),
            |_| Ok("should not run".to_string()),
        )
        .unwrap();
        assert!(ambiguous.contains("Multiple symbols found"));

        let not_found = handle_symbol_resolution("nope", SymbolResolution::NotFound, |_| {
            Ok("should not run".to_string())
        })
        .unwrap();
        assert!(not_found.contains("Symbol not found"));
    }

    #[test]
    fn render_symbols_list_handles_empty_and_nonempty() {
        let root = PathBuf::from("/project");
        let empty = render_symbols_list(&[], &root);
        assert!(empty.contains("No symbols found"));

        let nonempty = render_symbols_list(&[sample_language_object(1, "run", "src/lib.rs")], &root);
        assert!(nonempty.contains("# Symbols"));
        assert!(nonempty.contains("lib::run"));
    }

    #[test]
    fn render_call_edges_handles_resolved_and_unresolved() {
        let root = PathBuf::from("/project");
        let obj = sample_language_object(1, "run", "src/lib.rs");
        let empty = render_call_edges("Callees", &obj, &[], &root);
        assert!(empty.contains("No relationships found"));

        let edges = vec![
            (
                sample_call_edge(Some("helper")),
                Some(sample_symbol("helper", "src/helper.rs")),
            ),
            (sample_call_edge(Some("unknown_fn")), None),
        ];
        let text = render_call_edges("Callees", &obj, &edges, &root);
        assert!(text.contains("lib::helper"));
        assert!(text.contains("<unresolved>"));
        assert!(text.contains("label: unknown_fn"));
    }

    #[test]
    fn render_caller_edges_formats_callers() {
        let root = PathBuf::from("/project");
        let obj = sample_language_object(1, "load", "src/lib.rs");
        let edges = vec![(
            sample_call_edge(None),
            sample_symbol("run_pipeline", "src/lib.rs"),
        )];
        let text = render_caller_edges("Callers", &obj, &edges, &root);
        assert!(text.contains("# Callers"));
        assert!(text.contains("lib::run_pipeline"));
    }

    #[test]
    fn render_affected_context_text_concatenates_sections() {
        let pack = ContextPack {
            query: "run".to_string(),
            mode: GraphContextMode::Neighborhood,
            token_budget: 1000,
            requested_token_budget: None,
            effective_token_budget: None,
            estimated_tokens: 10,
            roots: vec![],
            nodes: vec![],
            edges: vec![],
            snippets: vec![],
            sections: vec![
                ContextSection {
                    kind: ContextSectionKind::Summary,
                    text: "section one\n".to_string(),
                    estimated_tokens: 5,
                },
                ContextSection {
                    kind: ContextSectionKind::Snippets,
                    text: "section two\n".to_string(),
                    estimated_tokens: 5,
                },
            ],
            omitted: vec![],
            diagnostics: vec![],
        };
        assert_eq!(
            render_affected_context_text(&pack),
            "section one\nsection two\n"
        );
    }

    #[test]
    fn render_context_to_markdown_includes_graph_and_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path().to_path_buf();
        let lib_path = root.join("src/lib.rs");
        std::fs::create_dir_all(lib_path.parent().unwrap()).unwrap();
        std::fs::write(&lib_path, "fn run() {}\nfn load() {}\n").unwrap();

        let root_obj = LanguageObject {
            id: SymbolId(1),
            name: "run".to_string(),
            qualified_name: "lib::run".to_string(),
            kind: LanguageObjectKind::Function,
            file_path: lib_path.clone(),
            range: SourceRange {
                start_line: 1,
                start_col: 0,
                end_line: 1,
                end_col: 10,
            },
            signature: None,
            language: Some("rust".to_string()),
        };
        let node = LanguageObject {
            id: SymbolId(2),
            name: "load".to_string(),
            qualified_name: "lib::load".to_string(),
            kind: LanguageObjectKind::Function,
            file_path: lib_path.clone(),
            range: SourceRange {
                start_line: 2,
                start_col: 0,
                end_line: 2,
                end_col: 10,
            },
            signature: None,
            language: Some("rust".to_string()),
        };
        let result = GraphContextResult {
            root: root_obj,
            nodes: vec![node],
            edges: vec![GraphContextEdge {
                from: SymbolId(1),
                to: SymbolId(2),
                label: None,
                confidence: None,
            }],
            files: vec![ContextFileSpan {
                file_path: lib_path.clone(),
                range: SourceRange {
                    start_line: 1,
                    end_line: 1,
                    start_col: 0,
                    end_col: 10,
                },
            }],
            diagnostics: vec![],
        };

        let text = render_context_to_markdown(
            &result,
            &root,
            GraphContextMode::Neighborhood,
            2,
            40,
            20,
        );
        assert!(text.contains("# Graph Context"));
        assert!(text.contains("lib::run -> lib::load"));
        assert!(text.contains("```rust"));
        assert!(text.contains("fn run()"));
    }
}