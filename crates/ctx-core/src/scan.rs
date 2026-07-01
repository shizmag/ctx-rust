use std::path::{Path, PathBuf};

use ctx_filter::FilterEntry;
use ctx_models::{
    HiddenItem, NodeKind, NodeStats, ProjectSummary, ScanOptions, ScanResult, Visibility,
};
use ignore::WalkBuilder;

use crate::error::ScanError;
use crate::kind::node_kind;
use crate::tree_builder::TreeBuilder;

pub fn scan(path: &Path, options: ScanOptions) -> Result<ScanResult, ScanError> {
    let root_path = path.canonicalize()?;

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

        match ctx_filter::classify(&filter_entry, &options) {
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
                };

                summary.files += 1;
                summary.lines += file_stats.lines;
                summary.bytes += file_stats.bytes;

                tree.add_node(entry_path, kind, stats);
            }

            NodeKind::Symlink => {
                let stats = NodeStats {
                    files: 0,
                    dirs: 0,
                    lines: 0,
                    bytes: bytes.unwrap_or(0),
                };

                tree.add_node(entry_path, kind, stats);
            }

            NodeKind::Other => {
                let stats = NodeStats {
                    files: 0,
                    dirs: 0,
                    lines: 0,
                    bytes: bytes.unwrap_or(0),
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
