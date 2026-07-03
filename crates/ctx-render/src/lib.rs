use ctx_models::{
    NodeKind, ScanResult, TreeNode, format_bytes, get_relative_path, walk_tree_lines,
};
use std::path::Path;

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

fn render_tree(root: &TreeNode) -> String {
    let mut out = String::new();
    walk_tree_lines(root, |line| {
        if line.is_root {
            out.push_str(&line.node.name);
            out.push('\n');
        } else {
            let connector = if line.is_last {
                "└── "
            } else {
                "├── "
            };
            out.push_str(&line.prefix);
            out.push_str(connector);
            out.push_str(&line.node.name);
            out.push('\n');
        }
        true
    });
    out
}

fn collect_file_nodes<'a>(node: &'a TreeNode, files: &mut Vec<&'a TreeNode>) {
    if node.kind == NodeKind::File {
        files.push(node);
    }
    for child in &node.children {
        collect_file_nodes(child, files);
    }
}

fn get_skip_reason(
    node: &TreeNode,
    res: &ctx_models::FileContentResult,
    max_file_size: u64,
) -> String {
    match res {
        ctx_models::FileContentResult::Text(_) => String::new(),
        ctx_models::FileContentResult::Skipped(ctx_models::FileSkipReason::TooLarge) => {
            format!(
                "Too large ({} KB, limit is {} KB)",
                node.stats.bytes / 1024,
                max_file_size / 1024
            )
        }
        ctx_models::FileContentResult::Skipped(ctx_models::FileSkipReason::NonUtf8) => {
            "Binary / Non-UTF8 file".to_string()
        }
        ctx_models::FileContentResult::ReadError(err) => {
            format!("Read error: {}", err)
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

fn get_markdown_fence(content: &str) -> String {
    let mut max_backticks = 0;
    let mut current_backticks = 0;
    for c in content.chars() {
        if c == '`' {
            current_backticks += 1;
            if current_backticks > max_backticks {
                max_backticks = current_backticks;
            }
        } else {
            current_backticks = 0;
        }
    }
    let fence_len = std::cmp::max(3, max_backticks + 1);
    "`".repeat(fence_len)
}

fn render_markdown(result: &ScanResult, options: &RenderOptions) -> Result<String, std::io::Error> {
    let mut out = String::new();

    out.push_str(&format!("# Project: {}\n\n", result.root.name));

    if options.include_stats {
        out.push_str("## Project Summary\n");
        out.push_str(&format!("- **Files**: {}\n", result.summary.files));
        out.push_str(&format!("- **Directories**: {}\n", result.summary.dirs));
        out.push_str(&format!("- **Total Lines**: {}\n", result.summary.lines));
        out.push_str(&format!(
            "- **Total Size**: {}\n",
            format_bytes(result.summary.bytes)
        ));
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

        match ctx_models::read_file_content(&file_node.path, options.max_file_size) {
            ctx_models::FileContentResult::Text(content) => {
                let lang = get_markdown_lang(&file_node.path);
                let fence = get_markdown_fence(&content);
                out.push_str(&format!("{}{}\n", fence, lang));
                out.push_str(&content);
                if !content.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&format!("{}\n\n", fence));
            }
            other => {
                let reason = get_skip_reason(file_node, &other, options.max_file_size);
                out.push_str(&format!("*Skipped: {}*\n\n", reason));
            }
        }
    }

    Ok(out)
}

fn render_xml(result: &ScanResult, options: &RenderOptions) -> Result<String, std::io::Error> {
    let mut out = String::new();

    out.push_str(&format!(
        "<project name=\"{}\">\n",
        escape_xml(&result.root.name)
    ));

    if options.include_stats {
        out.push_str("  <summary>\n");
        out.push_str(&format!("    <files>{}</files>\n", result.summary.files));
        out.push_str(&format!(
            "    <directories>{}</directories>\n",
            result.summary.dirs
        ));
        out.push_str(&format!("    <lines>{}</lines>\n", result.summary.lines));
        out.push_str(&format!("    <bytes>{}</bytes>\n", result.summary.bytes));
        if result.summary.hidden_files > 0 || result.summary.hidden_dirs > 0 {
            out.push_str(&format!(
                "    <hidden_files>{}</hidden_files>\n",
                result.summary.hidden_files
            ));
            out.push_str(&format!(
                "    <hidden_directories>{}</hidden_directories>\n",
                result.summary.hidden_dirs
            ));
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

        match ctx_models::read_file_content(&file_node.path, options.max_file_size) {
            ctx_models::FileContentResult::Text(content) => {
                out.push_str(&format!("    <file path=\"{}\">\n", escape_xml(&rel_path)));
                out.push_str("      <content>");
                out.push_str(&escape_xml(&content));
                out.push_str("</content>\n");
                out.push_str("    </file>\n");
            }
            other => {
                let reason = get_skip_reason(file_node, &other, options.max_file_size);
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
        out.push_str(&format!(
            "Total Size: {}\n",
            format_bytes(result.summary.bytes)
        ));
        out.push('\n');
    }

    out.push_str("Structure:\n");
    out.push_str(&render_tree(&result.root));
    out.push('\n');

    let mut files = Vec::new();
    collect_file_nodes(&result.root, &mut files);

    for file_node in files {
        let rel_path = get_relative_path(&file_node.path, &result.root.path);

        match ctx_models::read_file_content(&file_node.path, options.max_file_size) {
            ctx_models::FileContentResult::Text(content) => {
                out.push_str("================================================================================\n");
                out.push_str(&format!("File: {}\n", rel_path));
                out.push_str("================================================================================\n");
                out.push_str(&content);
                if !content.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
            other => {
                let reason = get_skip_reason(file_node, &other, options.max_file_size);
                out.push_str("================================================================================\n");
                out.push_str(&format!("File: {} [Skipped: {}]\n", rel_path, reason));
                out.push_str("================================================================================\n\n");
            }
        }
    }

    Ok(out)
}

pub fn render_colored_tree(result: &ScanResult) -> Result<String, std::io::Error> {
    let mut out = String::new();

    let reset = "\x1b[0m";
    let gray = "\x1b[38;2;86;95;137m";
    let bold_blue = "\x1b[1;38;2;122;162;247m";
    let green = "\x1b[38;2;158;206;106m";
    let yellow = "\x1b[38;2;224;175;104m";
    let magenta = "\x1b[38;2;187;154;247m";
    let foreground = "\x1b[38;2;192;202;245m";

    walk_tree_lines(&result.root, |line| {
        let node = line.node;
        let is_dir = node.kind == NodeKind::Directory;
        let icon = get_node_icon(&node.name, is_dir);

        // Calculate prefix, icon, name lengths for alignment
        let prefix_len = if line.is_root {
            0
        } else {
            line.prefix.chars().count() + 4
        };
        let icon_width = 2; // Emoji icon + 1 space
        let name_len = node.name.chars().count() + if is_dir { 1 } else { 0 };
        let total_width = prefix_len + icon_width + name_len;

        let target_col = 55;
        let padding_count = if total_width < target_col {
            target_col - total_width
        } else {
            1
        };
        let leader = if total_width < target_col {
            format!("{}{}{}", gray, ".".repeat(padding_count), reset)
        } else {
            " ".to_string()
        };

        if line.is_root {
            let mut dir_parts = Vec::new();
            if node.stats.files > 0 {
                dir_parts.push(format!("{} files", node.stats.files));
            }
            let subdirs = node.stats.dirs.saturating_sub(1);
            if subdirs > 0 {
                dir_parts.push(format!("{} dirs", subdirs));
            }
            if node.stats.lines > 0 {
                dir_parts.push(format!("{} lines", node.stats.lines));
            }
            if node.stats.tokens > 0 {
                dir_parts.push(format!("{} tokens", node.stats.tokens));
            }

            let stats_str = if !dir_parts.is_empty() {
                format!(" ({})", dir_parts.join(", "))
            } else {
                "".to_string()
            };

            out.push_str(&format!(
                "{}{}{}/{}{}{}{}{}\n",
                bold_blue, icon, node.name, reset, leader, gray, stats_str, reset
            ));
        } else {
            let connector = if line.is_last {
                "└── "
            } else {
                "├── "
            };
            out.push_str(&format!("{}{}{}", gray, line.prefix, connector));

            match node.kind {
                NodeKind::Directory => {
                    let mut dir_parts = Vec::new();
                    if node.stats.files > 0 {
                        dir_parts.push(format!("{} files", node.stats.files));
                    }
                    let subdirs = node.stats.dirs.saturating_sub(1);
                    if subdirs > 0 {
                        dir_parts.push(format!("{} dirs", subdirs));
                    }
                    if node.stats.lines > 0 {
                        dir_parts.push(format!("{} lines", node.stats.lines));
                    }
                    if node.stats.tokens > 0 {
                        dir_parts.push(format!("{} tokens", node.stats.tokens));
                    }

                    let stats_str = if !dir_parts.is_empty() {
                        format!(" ({})", dir_parts.join(", "))
                    } else {
                        "".to_string()
                    };

                    out.push_str(&format!(
                        "{}{}{}/{}{}{}{}{}\n",
                        bold_blue, icon, node.name, reset, leader, gray, stats_str, reset
                    ));
                }
                NodeKind::File => {
                    let extension = std::path::Path::new(&node.name)
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or("")
                        .to_lowercase();

                    let file_color = match extension.as_str() {
                        "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "c" | "cpp" | "go" | "java"
                        | "swift" | "html" | "css" | "sh" | "bash" => green,
                        "md" | "txt" | "pdf" | "adoc" | "license" => yellow,
                        "toml" | "json" | "yaml" | "yml" | "lock" | "ini" | "conf" | "xml" => {
                            magenta
                        }
                        _ => foreground,
                    };

                    let mut stats_parts = Vec::new();
                    if node.stats.lines > 0 {
                        stats_parts.push(format!("{} lines", node.stats.lines));
                    }
                    if node.stats.tokens > 0 {
                        stats_parts.push(format!("{} tokens", node.stats.tokens));
                    }
                    if node.stats.bytes > 0 {
                        stats_parts.push(format_bytes(node.stats.bytes));
                    }

                    let stats_str = if !stats_parts.is_empty() {
                        format!(" ({})", stats_parts.join(", "))
                    } else {
                        "".to_string()
                    };

                    out.push_str(&format!(
                        "{}{}{}{}{}{}{}{}\n",
                        file_color, icon, node.name, reset, leader, gray, stats_str, reset
                    ));
                }
                _ => {
                    out.push_str(&format!(
                        "{}{}{}{}{}\n",
                        foreground, node.name, reset, leader, reset
                    ));
                }
            }
        }
        true
    });

    // Add space and a gorgeous project summary box in Tokyo Night style!
    out.push_str(
        "\n\x1b[38;2;86;95;137m╭────────────────────────────────────────────────╮\x1b[0m\n",
    );
    out.push_str(&format!(
        "│ \x1b[1;38;2;122;162;247mProject Summary:\x1b[0m{:31}│\n",
        ""
    ));

    let files_str = format!("{} files", result.summary.files);
    let dirs_str = format!("{} directories", result.summary.dirs);
    let lines_str = format!("{} lines", result.summary.lines);
    let size_str = format_bytes(result.summary.bytes);

    out.push_str(&format!(
        "│  \x1b[38;2;125;207;255m{:<12}\x1b[0m : {:<31}│\n",
        "Files", files_str
    ));
    out.push_str(&format!(
        "│  \x1b[38;2;125;207;255m{:<12}\x1b[0m : {:<31}│\n",
        "Directories", dirs_str
    ));
    out.push_str(&format!(
        "│  \x1b[38;2;125;207;255m{:<12}\x1b[0m : {:<31}│\n",
        "Total Lines", lines_str
    ));
    out.push_str(&format!(
        "│  \x1b[38;2;125;207;255m{:<12}\x1b[0m : {:<31}│\n",
        "Total Size", size_str
    ));

    if result.summary.hidden_files > 0 || result.summary.hidden_dirs > 0 {
        let hidden_str = format!(
            "{} files, {} dirs",
            result.summary.hidden_files, result.summary.hidden_dirs
        );
        out.push_str(&format!(
            "│  \x1b[38;2;247;118;142m{:<12}\x1b[0m : {:<31}│\n",
            "Hidden", hidden_str
        ));
    }

    out.push_str("\x1b[38;2;86;95;137m╰────────────────────────────────────────────────╯\x1b[0m\n");

    Ok(out)
}

pub fn get_node_icon(name: &str, is_dir: bool) -> &'static str {
    let lower_name = name.to_lowercase();
    if is_dir {
        if lower_name == ".git" {
            "🗃️ "
        } else if lower_name == ".github" {
            "🐙 "
        } else if lower_name == "node_modules" {
            "📦 "
        } else if lower_name == "target"
            || lower_name == "build"
            || lower_name == "dist"
            || lower_name == "out"
        {
            "🏗️ "
        } else if lower_name == "src" {
            "📂 "
        } else if lower_name == "tests" || lower_name == "test" || lower_name == "spec" {
            "🧪 "
        } else if lower_name == "docs" || lower_name == "doc" {
            "📖 "
        } else if lower_name == "assets"
            || lower_name == "static"
            || lower_name == "images"
            || lower_name == "img"
        {
            "🎨 "
        } else if lower_name == "config" || lower_name == "settings" {
            "⚙️ "
        } else {
            "📁 "
        }
    } else if lower_name == "cargo.lock"
        || lower_name == "package-lock.json"
        || lower_name == "yarn.lock"
        || lower_name == "pnpm-lock.yaml"
    {
        "🔒 "
    } else if lower_name == "cargo.toml"
        || lower_name == "package.json"
        || lower_name == "tsconfig.json"
        || lower_name == "webpack.config.js"
        || lower_name == "vite.config.ts"
        || lower_name == "makefile"
        || lower_name == "cmakelists.txt"
    {
        "⚙️ "
    } else if lower_name == ".gitignore"
        || lower_name == ".gitattributes"
        || lower_name == ".env"
        || lower_name == ".env.example"
        || lower_name == ".dockerignore"
    {
        "🛠️ "
    } else if lower_name == "dockerfile"
        || lower_name == "docker-compose.yml"
        || lower_name == "docker-compose.yaml"
    {
        "🐳 "
    } else if lower_name.ends_with(".rs") {
        "🦀 "
    } else if lower_name.ends_with(".py") {
        "🐍 "
    } else if lower_name.ends_with(".js") || lower_name.ends_with(".jsx") {
        "🟨 "
    } else if lower_name.ends_with(".ts") || lower_name.ends_with(".tsx") {
        "🟦 "
    } else if lower_name.ends_with(".md") {
        "📝 "
    } else if lower_name.ends_with(".toml")
        || lower_name.ends_with(".json")
        || lower_name.ends_with(".yaml")
        || lower_name.ends_with(".yml")
        || lower_name.ends_with(".xml")
        || lower_name.ends_with(".ini")
        || lower_name.ends_with(".conf")
    {
        "⚙️ "
    } else if lower_name == "license"
        || lower_name.starts_with("license.")
        || lower_name == "copying"
    {
        "⚖️ "
    } else if lower_name.ends_with(".sh")
        || lower_name.ends_with(".bash")
        || lower_name.ends_with(".zsh")
    {
        "🐚 "
    } else if lower_name.ends_with(".go") {
        "🐹 "
    } else if lower_name.ends_with(".c") || lower_name.ends_with(".h") {
        "🇨 "
    } else if lower_name.ends_with(".cpp")
        || lower_name.ends_with(".hpp")
        || lower_name.ends_with(".cc")
    {
        "➕ "
    } else if lower_name.ends_with(".java") || lower_name.ends_with(".jar") {
        "☕ "
    } else if lower_name.ends_with(".html") || lower_name.ends_with(".htm") {
        "🌐 "
    } else if lower_name.ends_with(".css")
        || lower_name.ends_with(".scss")
        || lower_name.ends_with(".sass")
        || lower_name.ends_with(".less")
    {
        "🎨 "
    } else if lower_name.ends_with(".png")
        || lower_name.ends_with(".jpg")
        || lower_name.ends_with(".jpeg")
        || lower_name.ends_with(".gif")
        || lower_name.ends_with(".svg")
        || lower_name.ends_with(".ico")
    {
        "🖼️ "
    } else if lower_name.ends_with(".zip")
        || lower_name.ends_with(".tar")
        || lower_name.ends_with(".gz")
        || lower_name.ends_with(".rar")
        || lower_name.ends_with(".7z")
    {
        "🗜️ "
    } else {
        "📄 "
    }
}
