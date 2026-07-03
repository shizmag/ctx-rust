use std::path::Path;
use ctx_models::ScanResult;

/// Estimates the number of tokens in a string.
/// A standard approximation is ~4 characters per token.
pub fn estimate_tokens(content: &str) -> usize {
    if content.is_empty() {
        0
    } else {
        (content.chars().count() + 3) / 4
    }
}

/// Forms the context string containing directory structure and all file contents,
/// annotated with token counts.
pub fn build_context(result: &ScanResult, max_file_size: u64) -> String {
    let mut out = String::new();
    out.push_str(&format!("Project Context: {}\n", result.root.name));
    out.push_str(&format!("Total files: {} | Total tokens: {}\n\n", result.summary.files, result.summary.tokens));

    out.push_str("=== DIRECTORY STRUCTURE ===\n");
    out.push_str(&render_tree(&result.root));
    out.push('\n');

    out.push_str("=== FILE CONTENTS ===\n\n");
    append_files_content(&result.root, max_file_size, &result.root.path, &mut out);

    out
}

fn render_tree(node: &ctx_models::TreeNode) -> String {
    let mut out = String::new();
    render_tree_node(node, "", true, true, &mut out);
    out
}

fn render_tree_node(node: &ctx_models::TreeNode, prefix: &str, is_last: bool, is_root: bool, out: &mut String) {
    let tokens_str = if node.stats.tokens > 0 {
        format!(" ({} tokens)", node.stats.tokens)
    } else {
        "".to_string()
    };

    if is_root {
        out.push_str(&format!("{}{}\n", node.name, tokens_str));
    } else {
        let connector = if is_last { "└── " } else { "├── " };
        out.push_str(prefix);
        out.push_str(connector);
        out.push_str(&format!("{}{}\n", node.name, tokens_str));
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

fn append_files_content(node: &ctx_models::TreeNode, max_file_size: u64, root_path: &Path, out: &mut String) {
    if node.kind == ctx_models::NodeKind::File {
        let rel_path = match node.path.strip_prefix(root_path) {
            Ok(rel) => rel.to_string_lossy().to_string(),
            Err(_) => node.path.to_string_lossy().to_string(),
        };

        out.push_str(&format!("--- FILE: {} ({} tokens) ---\n", rel_path, node.stats.tokens));
        
        if node.stats.bytes > max_file_size {
            out.push_str(&format!("[File skipped: Too large ({} KB)]\n\n", node.stats.bytes / 1024));
            return;
        }

        match std::fs::read_to_string(&node.path) {
            Ok(content) => {
                out.push_str(&content);
                if !content.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
            Err(_) => {
                out.push_str("[File skipped: Binary or read error]\n\n");
            }
        }
    }

    for child in &node.children {
        append_files_content(child, max_file_size, root_path, out);
    }
}
