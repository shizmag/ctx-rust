use crate::app::{TuiApp, collect_checked_files};
use ctx_models::get_relative_path;

pub(crate) fn copy_selection_to_clipboard(app: &TuiApp) -> Result<String, crate::error::TuiError> {
    let mut out = String::new();
    let root_name = app
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    let mut checked_files = Vec::new();
    collect_checked_files(
        &app.scan_result.root,
        &app.checked_paths,
        &mut checked_files,
    );
    let total_tokens: usize = checked_files.iter().map(|f| f.stats.tokens).sum();

    out.push_str(&format!("Project Context: {}\n", root_name));
    out.push_str(&format!(
        "Selected files: {} | Total tokens: {}\n\n",
        checked_files.len(),
        total_tokens
    ));

    out.push_str("=== DIRECTORY STRUCTURE (SELECTED FILES) ===\n");
    for f in &checked_files {
        let rel_path = get_relative_path(&f.path, &app.path);
        out.push_str(&format!("├── {} ({} tokens)\n", rel_path, f.stats.tokens));
    }
    out.push_str("\n=== FILE CONTENTS ===\n\n");

    for f in &checked_files {
        let rel_path = get_relative_path(&f.path, &app.path);
        out.push_str(&format!(
            "--- FILE: {} ({} tokens) ---\n",
            rel_path, f.stats.tokens
        ));
        match ctx_models::read_file_content(&f.path, u64::MAX) {
            ctx_models::FileContentResult::Text(content) => {
                out.push_str(&content);
                if !content.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
            ctx_models::FileContentResult::Skipped(ctx_models::FileSkipReason::NonUtf8) => {
                out.push_str("[File skipped: Binary or non-UTF8]\n\n");
            }
            ctx_models::FileContentResult::Skipped(ctx_models::FileSkipReason::TooLarge) => {
                out.push_str("[File skipped: Too large]\n\n");
            }
            ctx_models::FileContentResult::ReadError(e) => {
                out.push_str(&format!("[File skipped: Read error ({})]\n\n", e));
            }
        }
    }

    let mut ctx_clipboard = arboard::Clipboard::new()?;
    ctx_clipboard.set_text(out)?;

    Ok(format!(
        "Copied {} files ({} tokens) to clipboard!",
        checked_files.len(),
        total_tokens
    ))
}

pub(crate) fn copy_graph_context_to_clipboard(
    app: &TuiApp,
) -> Result<String, crate::error::TuiError> {
    let result = match &app.graph_preview {
        Some(Ok(res)) => res,
        Some(Err(err)) => {
            return Err(crate::error::TuiError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                err.clone(),
            )));
        }
        None => {
            return Err(crate::error::TuiError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "No preview generated to copy",
            )));
        }
    };

    let rendered = render_graph_context_output(
        result,
        &app.path,
        app.graph_mode,
        app.graph_depth,
        app.graph_max_nodes,
    )
    .map_err(|e| {
        crate::error::TuiError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        ))
    })?;

    let mut ctx_clipboard = arboard::Clipboard::new()?;
    ctx_clipboard.set_text(rendered)?;

    Ok("Copied graph context to clipboard!".to_string())
}

pub(crate) fn render_graph_context_output(
    result: &ctx_codegraph::GraphContextResult,
    root_path: &std::path::Path,
    mode: ctx_codegraph::GraphContextMode,
    depth: usize,
    max_nodes: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut out = String::new();

    // Header
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
    out.push_str(&format!("Max nodes: {}\n\n", max_nodes));

    // Graph
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

    // Included Symbols
    out.push_str("## Included Symbols\n\n");
    let mut symbols_list = Vec::new();

    let format_symbol = |obj: &ctx_codegraph::LanguageObject| -> String {
        let kind = kind_to_str(obj.kind);
        let rel_path = obj
            .file_path
            .strip_prefix(root_path)
            .unwrap_or(&obj.file_path);
        format!(
            "- {} {} — {}:{}-{}",
            kind,
            obj.name,
            rel_path.display(),
            obj.range.start_line,
            obj.range.end_line
        )
    };

    symbols_list.push(format_symbol(&result.root));
    for node in &result.nodes {
        symbols_list.push(format_symbol(node));
    }
    symbols_list.sort();

    for sym_line in symbols_list {
        out.push_str(&sym_line);
        out.push('\n');
    }
    out.push('\n');

    // Files
    out.push_str("## Files\n\n");

    let mut sorted_files = result.files.clone();
    sorted_files.sort_by(|a, b| match a.file_path.cmp(&b.file_path) {
        std::cmp::Ordering::Equal => a.range.start_line.cmp(&b.range.start_line),
        other => other,
    });

    for file_span in sorted_files {
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

        let content = match get_file_span_content(
            &file_span.file_path,
            file_span.range.start_line,
            file_span.range.end_line,
        ) {
            Ok(c) => c,
            Err(e) => format!("Error reading file: {}\n", e),
        };

        let lang = get_markdown_lang(&file_span.file_path);
        out.push_str(&format!("```{}\n", lang));
        out.push_str(&content);
        if !content.ends_with('\n') && !content.is_empty() {
            out.push('\n');
        }
        out.push_str("```\n\n");
    }

    Ok(out)
}

fn kind_to_str(kind: ctx_codegraph::LanguageObjectKind) -> &'static str {
    match kind {
        ctx_codegraph::LanguageObjectKind::Function => "fn",
        ctx_codegraph::LanguageObjectKind::Method => "fn",
        ctx_codegraph::LanguageObjectKind::Struct => "struct",
        ctx_codegraph::LanguageObjectKind::Enum => "enum",
        ctx_codegraph::LanguageObjectKind::Trait => "trait",
        ctx_codegraph::LanguageObjectKind::Impl => "impl",
        ctx_codegraph::LanguageObjectKind::Module => "mod",
        _ => "symbol",
    }
}

fn get_file_span_content(
    path: &std::path::Path,
    start_line: usize,
    end_line: usize,
) -> Result<String, std::io::Error> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    if start_line == 0 || start_line > lines.len() {
        return Ok("".to_string());
    }
    let end = std::cmp::min(end_line, lines.len());
    if start_line > end {
        return Ok("".to_string());
    }
    let mut result = String::new();
    for i in (start_line - 1)..end {
        result.push_str(lines[i]);
        result.push('\n');
    }
    Ok(result)
}

fn get_markdown_lang(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("rs") => "rust",
        Some("py") => "python",
        Some("js") => "javascript",
        Some("ts") => "typescript",
        Some("tsx") => "tsx",
        Some("jsx") => "jsx",
        Some("html") => "html",
        Some("css") => "css",
        Some("json") => "json",
        Some("toml") => "toml",
        Some("md") => "markdown",
        Some("sh") => "bash",
        Some("yaml") | Some("yml") => "yaml",
        Some("go") => "go",
        Some("c") => "c",
        Some("cpp") | Some("cc") | Some("h") | Some("hpp") => "cpp",
        Some("java") => "java",
        Some("kt") => "kotlin",
        Some("swift") => "swift",
        Some("txt") => "text",
        _ => "",
    }
}
