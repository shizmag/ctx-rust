use std::{collections::btree_set, fs::OpenOptions, path::PathBuf};

use ctx_filter::{FilterEntry, classify};
use ctx_models::{HiddenReason, Mode, NodeKind, ScanOptions, Visibility};

fn options() -> ScanOptions {
    ScanOptions {
        max_depth: None,
        max_file_size: 512 * 1024,
        mode: Mode::Smart,
    }
}

fn entry(path: &str, kind: NodeKind) -> FilterEntry {
    FilterEntry::new(PathBuf::from(path), kind, 0, None)
}

#[test]
fn hides_vcs_directories() {
    let item = entry(".git", NodeKind::Directory);

    let result = classify(&item, &options());

    assert_eq!(result, Visibility::Hidden(HiddenReason::VcsInternals));
}

#[test]
fn hides_dependency_directories() {
    let item = entry("node_modules", NodeKind::Directory);

    let result = classify(&item, &options());

    assert_eq!(result, Visibility::Hidden(HiddenReason::Dependencies));
}

#[test]
fn hides_rust_target_directory() {
    let item = entry("target", NodeKind::Directory);

    let result = classify(&item, &options());

    assert_eq!(result, Visibility::Hidden(HiddenReason::BuildArtifacts));
}

#[test]
fn hides_lockfiles() {
    let item = entry("package-lock.json", NodeKind::File);

    let result = classify(&item, &options());

    assert_eq!(result, Visibility::Hidden(HiddenReason::Lockfile));
}

#[test]
fn all_mode_disables_builtin_hiding() {
    let options = ScanOptions {
        mode: Mode::All,
        max_depth: None,
        max_file_size: 512 * 1024,
    };

    let item = entry("node_modules", NodeKind::Directory);

    let result = classify(&item, &options);

    assert_eq!(result, Visibility::Visible);
}

#[test]
fn symlink_is_not_hidden_by_directory_rule() {
    let item = entry("node_modules", NodeKind::Symlink);

    let result = classify(&item, &options());

    assert_eq!(result, Visibility::Visible);
}
