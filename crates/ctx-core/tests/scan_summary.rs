use std::fs;

use ctx_core::scan;
use ctx_models::{Mode, NodeKind, ScanOptions};

#[test]
fn scan_builds_tree_and_skips_hidden_directories() {
    let root = std::env::temp_dir().join("ctx_core_scan_builds_tree_and_skips_hidden_directories");

    let _ = fs::remove_dir_all(&root);

    fs::create_dir_all(root.join("src/bin")).unwrap();
    fs::create_dir_all(root.join(".git/objects")).unwrap();
    fs::create_dir_all(root.join("target/debug")).unwrap();

    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("src/bin/tool.rs"), "fn tool() {}\n").unwrap();
    fs::write(root.join("README.md"), "# Hello\n").unwrap();

    fs::write(root.join(".git/config"), "git stuff\n").unwrap();
    fs::write(root.join("target/debug/app"), "compiled\n").unwrap();

    let result = scan(
        &root,
        ScanOptions {
            mode: Mode::Smart,
            max_depth: None,
            max_file_size: 1024,
        },
    )
    .unwrap();

    assert_eq!(result.summary.files, 3);
    assert_eq!(result.summary.hidden_dirs, 2);
    assert_eq!(result.summary.lines, 3);

    let root_children: Vec<_> = result
        .root
        .children
        .iter()
        .map(|node| node.name.as_str())
        .collect();

    assert!(root_children.contains(&"src"));
    assert!(root_children.contains(&"README.md"));
    assert!(!root_children.contains(&".git"));
    assert!(!root_children.contains(&"target"));

    let src = result
        .root
        .children
        .iter()
        .find(|node| node.name == "src")
        .unwrap();

    assert_eq!(src.kind, NodeKind::Directory);
    assert_eq!(src.stats.files, 2);
    assert_eq!(src.stats.lines, 2);

    fs::remove_dir_all(root).unwrap();
}
