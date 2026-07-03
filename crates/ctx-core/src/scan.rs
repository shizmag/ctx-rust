use std::path::{Path, PathBuf};

use ctx_filter::{FilterContext, FilterEngine, FilterEntry};
use ctx_models::{
    HiddenItem, HiddenReason, NodeKind, NodeStats, ProjectSummary, ScanOptions, ScanResult, Visibility,
};
use ignore::WalkBuilder;

use crate::error::ScanError;
use crate::kind::node_kind;
use crate::tree_builder::TreeBuilder;

pub fn scan(path: &Path, options: ScanOptions) -> Result<ScanResult, ScanError> {
    let root_path = path.canonicalize()?;
    let gitignore = load_gitignore(&root_path, &options.exclude);

    let engine = FilterEngine::default_smart();
    let context = FilterContext { options: &options };

    let walker = WalkBuilder::new(&root_path)
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .build();

    let mut summary = ProjectSummary::default();
    let mut hidden = Vec::new();
    let mut pruned_dirs: Vec<PathBuf> = Vec::new();
    let mut tree = TreeBuilder::new(root_path.clone());

    for result in walker {
        let entry = result?;
        let entry_path = entry.path();

        if entry_path == root_path {
            continue;
        }

        if is_inside_pruned_dir(entry_path, &pruned_dirs) {
            continue;
        }

        let kind = node_kind(&entry);
        let is_dir = kind == NodeKind::Directory;

        if let Some(ref gi) = gitignore {
            if gi.matched(entry_path, is_dir).is_ignore() {
                if is_dir {
                    summary.hidden_dirs += 1;
                    pruned_dirs.push(entry_path.to_path_buf());
                } else {
                    summary.hidden_files += 1;
                }

                hidden.push(HiddenItem {
                    path: entry_path.to_path_buf(),
                    reason: HiddenReason::Gitignored,
                    is_dir,
                });

                continue;
            }
        }

        let depth = entry.depth().saturating_sub(1);

        if let Some(max_depth) = options.max_depth {
            if depth > max_depth {
                if kind == NodeKind::Directory {
                    pruned_dirs.push(entry_path.to_path_buf());
                }

                continue;
            }
        }

        let metadata = entry_path.symlink_metadata().ok();
        let bytes = metadata.as_ref().map(|metadata| metadata.len());

        let filter_entry = FilterEntry::new(entry_path.to_path_buf(), kind, depth, bytes);

        match engine.check(&filter_entry, &context) {
            Visibility::Visible => {}

            Visibility::Hidden(reason) => {
                let is_dir = kind == NodeKind::Directory;

                if is_dir {
                    summary.hidden_dirs += 1;
                    pruned_dirs.push(entry_path.to_path_buf());
                } else {
                    summary.hidden_files += 1;
                }

                hidden.push(HiddenItem {
                    path: entry_path.to_path_buf(),
                    reason,
                    is_dir,
                });

                continue;
            }
        }

        match kind {
            NodeKind::Directory => {
                let stats = NodeStats {
                    files: 0,
                    dirs: 1,
                    lines: 0,
                    bytes: 0,
                    tokens: 0,
                };

                summary.dirs += 1;
                tree.add_node(entry_path, kind, stats);
            }

            NodeKind::File => {
                let file_stats = ctx_stats::collect_file_stats(entry_path, options.max_file_size)?;

                let stats = NodeStats {
                    files: 1,
                    dirs: 0,
                    lines: file_stats.lines,
                    bytes: file_stats.bytes,
                    tokens: file_stats.tokens,
                };

                summary.files += 1;
                summary.lines += file_stats.lines;
                summary.bytes += file_stats.bytes;
                summary.tokens += file_stats.tokens;

                tree.add_node(entry_path, kind, stats);
            }

            NodeKind::Symlink => {
                let stats = NodeStats {
                    files: 0,
                    dirs: 0,
                    lines: 0,
                    bytes: bytes.unwrap_or(0),
                    tokens: 0,
                };

                tree.add_node(entry_path, kind, stats);
            }

            NodeKind::Other => {
                let stats = NodeStats {
                    files: 0,
                    dirs: 0,
                    lines: 0,
                    bytes: bytes.unwrap_or(0),
                    tokens: 0,
                };

                tree.add_node(entry_path, kind, stats);
            }
        }
    }

    Ok(ScanResult {
        root: tree.finish(),
        summary,
        hidden,
    })
}

fn is_inside_pruned_dir(path: &Path, pruned_dirs: &[PathBuf]) -> bool {
    pruned_dirs
        .iter()
        .any(|dir| path != dir && path.starts_with(dir))
}

fn load_gitignore(root_path: &Path, exclude_patterns: &[String]) -> Option<ignore::gitignore::Gitignore> {
    let gitignore_path = root_path.join(".gitignore");
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root_path);

    for pattern in exclude_patterns {
        let _ = builder.add_line(None, pattern);
    }

    if !gitignore_path.exists() {
        if !exclude_patterns.is_empty() {
            return builder.build().ok();
        }
        return None;
    }

    let content = match std::fs::read_to_string(&gitignore_path) {
        Ok(c) => c,
        Err(_) => {
            if !exclude_patterns.is_empty() {
                return builder.build().ok();
            }
            return None;
        }
    };

    let mut current_block: Vec<String> = Vec::new();
    let mut has_ctx = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !current_block.is_empty() {
                if !has_ctx {
                    for rule in &current_block {
                        let _ = builder.add_line(None, rule);
                    }
                }
                current_block.clear();
                has_ctx = false;
            }
        } else if trimmed == "#[ctx]" {
            has_ctx = true;
        } else {
            current_block.push(line.to_string());
        }
    }

    if !current_block.is_empty() && !has_ctx {
        for rule in &current_block {
            let _ = builder.add_line(None, rule);
        }
    }

    builder.build().ok()
}
