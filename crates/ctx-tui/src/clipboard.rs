use ctx_models::get_relative_path;
use crate::app::{TuiApp, collect_checked_files};

pub(crate) fn copy_selection_to_clipboard(app: &TuiApp) -> Result<String, Box<dyn std::error::Error>> {
    let mut out = String::new();
    let root_name = app.path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    let mut checked_files = Vec::new();
    collect_checked_files(&app.scan_result.root, &app.checked_paths, &mut checked_files);
    let total_tokens: usize = checked_files.iter().map(|f| f.stats.tokens).sum();

    out.push_str(&format!("Project Context: {}\n", root_name));
    out.push_str(&format!("Selected files: {} | Total tokens: {}\n\n", checked_files.len(), total_tokens));

    out.push_str("=== DIRECTORY STRUCTURE (SELECTED FILES) ===\n");
    for f in &checked_files {
        let rel_path = get_relative_path(&f.path, &app.path);
        out.push_str(&format!("├── {} ({} tokens)\n", rel_path, f.stats.tokens));
    }
    out.push_str("\n=== FILE CONTENTS ===\n\n");

    for f in &checked_files {
        let rel_path = get_relative_path(&f.path, &app.path);
        out.push_str(&format!("--- FILE: {} ({} tokens) ---\n", rel_path, f.stats.tokens));
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
