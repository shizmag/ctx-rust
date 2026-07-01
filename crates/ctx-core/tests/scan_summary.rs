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

#[test]
fn scan_respects_custom_gitignore_with_ctx_block() {
    let root = std::env::temp_dir().join("ctx_core_scan_gitignore_ctx_block");
    let _ = fs::remove_dir_all(&root);

    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("ignored_dir")).unwrap();
    fs::create_dir_all(root.join("ctx_bypass_dir")).unwrap();

    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("ignored_dir/file.txt"), "ignored\n").unwrap();
    fs::write(root.join("ctx_bypass_dir/file.txt"), "bypassed\n").unwrap();
    fs::write(root.join("normal_ignored.txt"), "ignored file\n").unwrap();
    fs::write(root.join("bypass_ignored.txt"), "bypassed file\n").unwrap();

    let gitignore_content = "\
# Normal ignore block
ignored_dir/
normal_ignored.txt

#[ctx]
ctx_bypass_dir/
bypass_ignored.txt
";
    fs::write(root.join(".gitignore"), gitignore_content).unwrap();

    let result = scan(
        &root,
        ScanOptions {
            mode: Mode::Smart,
            max_depth: None,
            max_file_size: 1024,
        },
    )
    .unwrap();

    // Checked files:
    // - src/main.rs (visible)
    // - ctx_bypass_dir/file.txt (visible due to #[ctx])
    // - bypass_ignored.txt (visible due to #[ctx])
    // - .gitignore (visible)
    // Total files = 4
    assert_eq!(result.summary.files, 4);
    assert_eq!(result.summary.hidden_files, 1); // normal_ignored.txt
    assert_eq!(result.summary.hidden_dirs, 1);  // ignored_dir/

    let root_children: Vec<_> = result
        .root
        .children
        .iter()
        .map(|node| node.name.as_str())
        .collect();

    assert!(root_children.contains(&"src"));
    assert!(root_children.contains(&"ctx_bypass_dir"));
    assert!(root_children.contains(&"bypass_ignored.txt"));
    assert!(!root_children.contains(&"ignored_dir"));
    assert!(!root_children.contains(&"normal_ignored.txt"));

    fs::remove_dir_all(root).unwrap();
}
