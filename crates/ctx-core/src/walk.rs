use std::path::{Path, PathBuf};
use ignore::{Walk, WalkBuilder};

pub fn setup_walker(root_path: &Path) -> Walk {
    WalkBuilder::new(root_path)
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .build()
}

pub fn is_inside_pruned_dir(path: &Path, pruned_dirs: &[PathBuf]) -> bool {
    pruned_dirs
        .iter()
        .any(|dir| path != dir && path.starts_with(dir))
}
