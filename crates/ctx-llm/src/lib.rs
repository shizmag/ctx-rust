use ctx_models::{ScanResult, get_relative_path};
use std::path::Path;

/// Forms the context string containing directory structure and all file contents,
/// annotated with token counts.
pub fn build_context(result: &ScanResult, max_file_size: u64) -> String {
    let mut out = String::new();
    out.push_str(&format!("Project Context: {}\n", result.root.name));
    out.push_str(&format!(
        "Total files: {} | Total tokens: {}\n\n",
        result.summary.files, result.summary.tokens
    ));

    out.push_str("=== DIRECTORY STRUCTURE ===\n");
    out.push_str(&render_tree(&result.root));
    out.push('\n');

    out.push_str("=== FILE CONTENTS ===\n\n");
    append_files_content(&result.root, max_file_size, &result.root.path, &mut out);

    out
}

fn render_tree(node: &ctx_models::TreeNode) -> String {
    let mut out = String::new();
    ctx_models::walk_tree_lines(node, |line| {
        let tokens_str = if line.node.stats.tokens > 0 {
            format!(" ({} tokens)", line.node.stats.tokens)
        } else {
            "".to_string()
        };

        if line.is_root {
            out.push_str(&format!("{}{}\n", line.node.name, tokens_str));
        } else {
            let connector = if line.is_last {
                "└── "
            } else {
                "├── "
            };
            out.push_str(&line.prefix);
            out.push_str(connector);
            out.push_str(&format!("{}{}\n", line.node.name, tokens_str));
        }
        true
    });
    out
}

fn append_files_content(
    node: &ctx_models::TreeNode,
    max_file_size: u64,
    root_path: &Path,
    out: &mut String,
) {
    if node.kind == ctx_models::NodeKind::File {
        let rel_path = get_relative_path(&node.path, root_path);

        out.push_str(&format!(
            "--- FILE: {} ({} tokens) ---\n",
            rel_path, node.stats.tokens
        ));

        match ctx_models::read_file_content(&node.path, max_file_size) {
            ctx_models::FileContentResult::Text(content) => {
                out.push_str(&content);
                if !content.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
            ctx_models::FileContentResult::Skipped(ctx_models::FileSkipReason::TooLarge) => {
                out.push_str(&format!(
                    "[File skipped: Too large ({} KB)]\n\n",
                    node.stats.bytes / 1024
                ));
            }
            ctx_models::FileContentResult::Skipped(ctx_models::FileSkipReason::NonUtf8)
            | ctx_models::FileContentResult::ReadError(_) => {
                out.push_str("[File skipped: Binary or read error]\n\n");
            }
        }
        return;
    }

    for child in &node.children {
        append_files_content(child, max_file_size, root_path, out);
    }
}
