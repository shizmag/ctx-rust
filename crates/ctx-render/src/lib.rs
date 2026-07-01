use std::path::Path;
use ctx_models::{NodeKind, ScanResult, TreeNode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Xml,
    Plain,
}

#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub format: Format,
    pub include_stats: bool,
    pub max_file_size: u64,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            format: Format::Markdown,
            include_stats: true,
            max_file_size: 512 * 1024, // 512 KB
        }
    }
}

pub fn render(result: &ScanResult, options: &RenderOptions) -> Result<String, std::io::Error> {
    match options.format {
        Format::Markdown => render_markdown(result, options),
        Format::Xml => render_xml(result, options),
        Format::Plain => render_plain(result, options),
    }
}

fn get_relative_path(path: &Path, root_path: &Path) -> String {
    match path.strip_prefix(root_path) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => path.to_string_lossy().to_string(),
    }
}

fn render_tree(root: &TreeNode) -> String {
    let mut out = String::new();
    render_tree_node(root, "", true, true, &mut out);
    out
}

fn render_tree_node(node: &TreeNode, prefix: &str, is_last: bool, is_root: bool, out: &mut String) {
    if is_root {
        out.push_str(&node.name);
        out.push('\n');
    } else {
        let connector = if is_last { "└── " } else { "├── " };
        out.push_str(prefix);
        out.push_str(connector);
        out.push_str(&node.name);
        out.push('\n');
    }

    let next_prefix = if is_root {
        "".to_string()
    } else {
        format!("{}{}", prefix, if is_last { "    " } else { "│   " })
    };

    let count = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        let child_is_last = i == count - 1;
        render_tree_node(child, &next_prefix, child_is_last, false, out);
    }
}

fn collect_file_nodes<'a>(node: &'a TreeNode, files: &mut Vec<&'a TreeNode>) {
    if node.kind == NodeKind::File {
        files.push(node);
    }
    for child in &node.children {
        collect_file_nodes(child, files);
    }
}

enum FileContentResult {
    Success(String),
    Skipped(String),
}

fn read_file_content(node: &TreeNode, max_file_size: u64) -> FileContentResult {
    // Check size limit
    if node.stats.bytes > max_file_size {
        return FileContentResult::Skipped(format!(
            "Too large ({} KB, limit is {} KB)",
            node.stats.bytes / 1024,
            max_file_size / 1024
        ));
    }

    match std::fs::read_to_string(&node.path) {
        Ok(content) => FileContentResult::Success(content),
        Err(err) => {
            if err.kind() == std::io::ErrorKind::InvalidData {
                FileContentResult::Skipped("Binary / Non-UTF8 file".to_string())
            } else {
                FileContentResult::Skipped(format!("Read error: {}", err))
            }
        }
    }
}

fn get_markdown_lang(path: &Path) -> &'static str {
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

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn escape_xml(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

fn render_markdown(result: &ScanResult, options: &RenderOptions) -> Result<String, std::io::Error> {
    let mut out = String::new();
    
    out.push_str(&format!("# Project: {}\n\n", result.root.name));

    if options.include_stats {
        out.push_str("## Project Summary\n");
        out.push_str(&format!("- **Files**: {}\n", result.summary.files));
        out.push_str(&format!("- **Directories**: {}\n", result.summary.dirs));
        out.push_str(&format!("- **Total Lines**: {}\n", result.summary.lines));
        out.push_str(&format!("- **Total Size**: {}\n", format_bytes(result.summary.bytes)));
        if result.summary.hidden_files > 0 || result.summary.hidden_dirs > 0 {
            out.push_str(&format!(
                "- **Hidden**: {} files, {} directories\n",
                result.summary.hidden_files, result.summary.hidden_dirs
            ));
        }
        out.push('\n');
    }

    out.push_str("## Directory Structure\n```text\n");
    out.push_str(&render_tree(&result.root));
    out.push_str("```\n\n");

    out.push_str("## Repository Files\n\n");

    let mut files = Vec::new();
    collect_file_nodes(&result.root, &mut files);

    for file_node in files {
        let rel_path = get_relative_path(&file_node.path, &result.root.path);
        out.push_str(&format!("### `{}`\n", rel_path));
        
        match read_file_content(file_node, options.max_file_size) {
            FileContentResult::Success(content) => {
                let lang = get_markdown_lang(&file_node.path);
                out.push_str(&format!("```{}\n", lang));
                out.push_str(&content);
                if !content.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("```\n\n");
            }
            FileContentResult::Skipped(reason) => {
                out.push_str(&format!("*Skipped: {}*\n\n", reason));
            }
        }
    }

    Ok(out)
}

fn render_xml(result: &ScanResult, options: &RenderOptions) -> Result<String, std::io::Error> {
    let mut out = String::new();
    
    out.push_str(&format!("<project name=\"{}\">\n", escape_xml(&result.root.name)));

    if options.include_stats {
        out.push_str("  <summary>\n");
        out.push_str(&format!("    <files>{}</files>\n", result.summary.files));
        out.push_str(&format!("    <directories>{}</directories>\n", result.summary.dirs));
        out.push_str(&format!("    <lines>{}</lines>\n", result.summary.lines));
        out.push_str(&format!("    <bytes>{}</bytes>\n", result.summary.bytes));
        if result.summary.hidden_files > 0 || result.summary.hidden_dirs > 0 {
            out.push_str(&format!("    <hidden_files>{}</hidden_files>\n", result.summary.hidden_files));
            out.push_str(&format!("    <hidden_directories>{}</hidden_directories>\n", result.summary.hidden_dirs));
        }
        out.push_str("  </summary>\n");
    }

    out.push_str("  <structure>\n");
    let tree_str = render_tree(&result.root);
    // Indent the tree lines inside <structure>
    for line in tree_str.lines() {
        out.push_str("    ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("  </structure>\n");

    out.push_str("  <files>\n");

    let mut files = Vec::new();
    collect_file_nodes(&result.root, &mut files);

    for file_node in files {
        let rel_path = get_relative_path(&file_node.path, &result.root.path);
        
        match read_file_content(file_node, options.max_file_size) {
            FileContentResult::Success(content) => {
                out.push_str(&format!("    <file path=\"{}\">\n", escape_xml(&rel_path)));
                let escaped_content = escape_xml(&content);
                // Indent content lines for readability
                for line in escaped_content.lines() {
                    out.push_str("      ");
                    out.push_str(line);
                    out.push('\n');
                }
                out.push_str("    </file>\n");
            }
            FileContentResult::Skipped(reason) => {
                out.push_str(&format!(
                    "    <file path=\"{}\" skipped=\"{}\" />\n",
                    escape_xml(&rel_path),
                    escape_xml(&reason)
                ));
            }
        }
    }

    out.push_str("  </files>\n");
    out.push_str("</project>\n");

    Ok(out)
}

fn render_plain(result: &ScanResult, options: &RenderOptions) -> Result<String, std::io::Error> {
    let mut out = String::new();
    
    out.push_str(&format!("Project: {}\n", result.root.name));

    if options.include_stats {
        out.push_str(&format!("Files: {}\n", result.summary.files));
        out.push_str(&format!("Directories: {}\n", result.summary.dirs));
        out.push_str(&format!("Total Lines: {}\n", result.summary.lines));
        out.push_str(&format!("Total Size: {}\n", format_bytes(result.summary.bytes)));
        out.push('\n');
    }

    out.push_str("Structure:\n");
    out.push_str(&render_tree(&result.root));
    out.push('\n');

    let mut files = Vec::new();
    collect_file_nodes(&result.root, &mut files);

    for file_node in files {
        let rel_path = get_relative_path(&file_node.path, &result.root.path);
        
        match read_file_content(file_node, options.max_file_size) {
            FileContentResult::Success(content) => {
                out.push_str("================================================================================\n");
                out.push_str(&format!("File: {}\n", rel_path));
                out.push_str("================================================================================\n");
                out.push_str(&content);
                if !content.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
            FileContentResult::Skipped(reason) => {
                out.push_str("================================================================================\n");
                out.push_str(&format!("File: {} [Skipped: {}]\n", rel_path, reason));
                out.push_str("================================================================================\n\n");
            }
        }
    }

    Ok(out)
}
