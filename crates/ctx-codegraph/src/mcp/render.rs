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