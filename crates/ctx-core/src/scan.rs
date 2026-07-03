use std::path::{Path, PathBuf};

use ctx_filter::{FilterContext, FilterEngine, FilterEntry};
use ctx_models::{
    HiddenItem, HiddenReason, NodeKind, NodeStats, ProjectSummary, ScanOptions, ScanResult, Visibility,
};

use crate::error::ScanError;
use crate::ignore::load_gitignore;
use crate::kind::node_kind;
use crate::summary;
use crate::tree_builder::TreeBuilder;
use crate::walk::{is_inside_pruned_dir, setup_walker};

pub fn scan(path: &Path, options: ScanOptions) -> Result<ScanResult, ScanError> {
    let root_path = path.canonicalize()?;
    let gitignore = load_gitignore(&root_path, &options.exclude);

    let engine = FilterEngine::default_smart();
    let context = FilterContext { options: &options };

    let walker = setup_walker(&root_path);

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
                summary::increment_hidden(&mut summary, is_dir);
                if is_dir {
                    pruned_dirs.push(entry_path.to_path_buf());
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
                summary::increment_hidden(&mut summary, is_dir);
                if is_dir {
                    pruned_dirs.push(entry_path.to_path_buf());
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

                summary::add_dir(&mut summary);
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

                summary::add_file(&mut summary, &file_stats);
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
