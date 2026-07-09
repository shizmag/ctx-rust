use std::fs;

use ctx_core::{scan, ScanError};
use ctx_models::{HiddenReason, Mode, ScanOptions};

#[test]
fn scan_empty_directory_returns_zero_counts() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    let result = scan(
        root,
        ScanOptions {
            mode: Mode::Smart,
            max_depth: None,
            max_file_size: 1024,
            exclude: Vec::new(),
        },
    )
    .unwrap();

    assert_eq!(result.summary.files, 0);
    assert_eq!(result.summary.dirs, 0);
    assert_eq!(result.summary.lines, 0);
    assert_eq!(result.summary.bytes, 0);
    assert!(result.root.children.is_empty());
    assert!(result.hidden.is_empty());
}

#[test]
fn scan_empty_subdirectory_is_counted_as_directory() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    fs::create_dir_all(root.join("empty_dir")).unwrap();

    let result = scan(
        root,
        ScanOptions {
            mode: Mode::Smart,
            max_depth: None,
            max_file_size: 1024,
            exclude: Vec::new(),
        },
    )
    .unwrap();

    assert_eq!(result.summary.files, 0);
    assert_eq!(result.summary.dirs, 1);

    let root_children: Vec<_> = result
        .root
        .children
        .iter()
        .map(|node| node.name.as_str())
        .collect();
    assert_eq!(root_children, vec!["empty_dir"]);
}

#[test]
fn scan_respects_exclude_patterns() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    fs::create_dir_all(root.join("vendor/lib")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("vendor/lib/util.rs"), "fn util() {}\n").unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

    let result = scan(
        root,
        ScanOptions {
            mode: Mode::Smart,
            max_depth: None,
            max_file_size: 1024,
            exclude: vec!["vendor/".to_string()],
        },
    )
    .unwrap();

    assert_eq!(result.summary.files, 1);
    assert_eq!(result.summary.hidden_dirs, 1);
    assert_eq!(result.summary.hidden_files, 0);

    let root_children: Vec<_> = result
        .root
        .children
        .iter()
        .map(|node| node.name.as_str())
        .collect();
    assert!(root_children.contains(&"src"));
    assert!(!root_children.contains(&"vendor"));

    assert!(
        result
            .hidden
            .iter()
            .any(|item| item.path.ends_with("vendor") && item.reason == HiddenReason::Gitignored)
    );
}

#[test]
fn scan_skips_oversized_file_content_but_keeps_file_entry() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    let small_content = "small\n";
    let large_content = "x".repeat(2048);
    fs::write(root.join("small.txt"), small_content).unwrap();
    fs::write(root.join("large.txt"), large_content).unwrap();

    let result = scan(
        root,
        ScanOptions {
            mode: Mode::All,
            max_depth: None,
            max_file_size: 1024,
            exclude: Vec::new(),
        },
    )
    .unwrap();

    assert_eq!(result.summary.files, 2);
    assert_eq!(result.summary.lines, 1);
    assert_eq!(result.summary.bytes, small_content.len() as u64 + 2048);

    let large = result
        .root
        .children
        .iter()
        .find(|node| node.name == "large.txt")
        .unwrap();
    assert_eq!(large.stats.lines, 0);
    assert_eq!(large.stats.bytes, 2048);

    let small = result
        .root
        .children
        .iter()
        .find(|node| node.name == "small.txt")
        .unwrap();
    assert_eq!(small.stats.lines, 1);
}

#[test]
fn scan_returns_error_for_nonexistent_path() {
    let missing = std::env::temp_dir().join("ctx-core-missing-path-test");

    let result = scan(
        &missing,
        ScanOptions {
            mode: Mode::Smart,
            max_depth: None,
            max_file_size: 1024,
            exclude: Vec::new(),
        },
    );

    assert!(matches!(result, Err(ScanError::Io(_))));
}