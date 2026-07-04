use std::fs;

use ctx_core::scan;
use ctx_models::{Mode, NodeKind, ScanOptions};

#[test]
fn scan_builds_tree_and_skips_hidden_directories() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path().to_path_buf();

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
            exclude: Vec::new(),
        },
    )
    .unwrap();

    assert_eq!(result.summary.files, 4);
    assert_eq!(result.summary.dirs, 2); // src, src/bin
    assert_eq!(result.summary.hidden_dirs, 2);
    assert_eq!(result.summary.lines, 5);
    assert!(result.summary.bytes > 0);
    assert!(result.summary.tokens > 0);

    // Verify hidden directories are in the hidden list
    assert!(
        result
            .hidden
            .iter()
            .any(|item| item.path.ends_with(".git") && item.is_dir)
    );
    assert!(
        result
            .hidden
            .iter()
            .any(|item| item.path.ends_with("target") && item.is_dir)
    );

    let root_children: Vec<_> = result
        .root
        .children
        .iter()
        .map(|node| node.name.as_str())
        .collect();

    assert!(root_children.contains(&"src"));
    assert!(root_children.contains(&"README.md"));
    assert!(root_children.contains(&".gitignore"));
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
}

#[test]
fn scan_respects_max_depth() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path().to_path_buf();

    fs::create_dir_all(root.join("depth1/depth2/depth3")).unwrap();
    fs::write(root.join("depth1/file1.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("depth1/depth2/file2.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("depth1/depth2/depth3/file3.rs"), "fn main() {}\n").unwrap();

    let result = scan(
        &root,
        ScanOptions {
            mode: Mode::Smart,
            max_depth: Some(1),
            max_file_size: 1024,
            exclude: Vec::new(),
        },
    )
    .unwrap();

    // Should include:
    // - depth1 (directory, depth 0)
    // - depth1/file1.rs (file, depth 1)
    // - depth1/depth2 (directory, depth 1)
    // Should NOT include:
    // - depth1/depth2/file2.rs (depth 2)
    // - depth1/depth2/depth3 (depth 2)
    // - depth1/depth2/depth3/file3.rs (depth 3)

    assert_eq!(result.summary.files, 1);
    assert_eq!(result.summary.dirs, 2);

    let depth1 = result
        .root
        .children
        .iter()
        .find(|node| node.name == "depth1")
        .unwrap();
    let depth1_children: Vec<_> = depth1
        .children
        .iter()
        .map(|node| node.name.as_str())
        .collect();
    assert!(depth1_children.contains(&"file1.rs"));
    assert!(depth1_children.contains(&"depth2"));

    let depth2 = depth1
        .children
        .iter()
        .find(|node| node.name == "depth2")
        .unwrap();
    assert!(depth2.children.is_empty());
}

#[test]
fn scan_respects_custom_gitignore_with_ctx_block() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path().to_path_buf();

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
            exclude: Vec::new(),
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
    assert_eq!(result.summary.hidden_dirs, 1); // ignored_dir/

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
}

#[test]
fn scan_respects_nested_gitignore_and_pruning() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path().to_path_buf();

    fs::create_dir_all(root.join("src/ignored_nested")).unwrap();
    fs::create_dir_all(root.join("logs")).unwrap();

    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("src/ignored_nested/file.rs"), "fn ignored() {}\n").unwrap();
    fs::write(root.join("logs/normal.txt"), "some log\n").unwrap();
    fs::write(root.join("logs/ctx.txt"), "bypassed log\n").unwrap();
    fs::write(root.join("visible.txt"), "visible text\n").unwrap();

    // root .gitignore:
    // normal ignores logs/ and *.txt
    // #[ctx] bypasses visible.txt
    let root_gitignore = "\
# normal ignore
logs/
*.txt

#[ctx]
visible.txt
";
    fs::write(root.join(".gitignore"), root_gitignore).unwrap();

    // nested .gitignore in src:
    // normal ignores ignored_nested/
    let nested_gitignore = "\
ignored_nested/
";
    fs::write(root.join("src/.gitignore"), nested_gitignore).unwrap();

    let result = scan(
        &root,
        ScanOptions {
            mode: Mode::Smart,
            max_depth: None,
            max_file_size: 1024,
            exclude: Vec::new(),
        },
    )
    .unwrap();

    let root_children: Vec<_> = result
        .root
        .children
        .iter()
        .map(|node| node.name.as_str())
        .collect();

    // Visible files:
    // - src/main.rs (visible)
    // - visible.txt (visible because of #[ctx] bypass, despite *.txt ignore rule)
    // - .gitignore
    // - src/.gitignore
    // Total files = 4
    assert_eq!(result.summary.files, 4);

    assert!(root_children.contains(&"src"));
    assert!(root_children.contains(&"visible.txt"));
    assert!(root_children.contains(&".gitignore"));
    assert!(!root_children.contains(&"logs")); // logs/ contains only ignored *.txt files

    let src = result
        .root
        .children
        .iter()
        .find(|node| node.name == "src")
        .unwrap();
    let src_children: Vec<_> = src.children.iter().map(|node| node.name.as_str()).collect();
    assert!(src_children.contains(&"main.rs"));
    assert!(src_children.contains(&".gitignore"));
    assert!(!src_children.contains(&"ignored_nested")); // ignored by nested gitignore

    // Check hidden items (pruned directories are in the hidden list):
    assert!(
        result
            .hidden
            .iter()
            .any(|item| item.path.ends_with("logs") && item.is_dir)
    );
    assert!(
        result
            .hidden
            .iter()
            .any(|item| item.path.ends_with("src/ignored_nested") && item.is_dir)
    );
}

#[test]
fn test_gitignore_auto_add() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path().to_path_buf();

    // 1. If .git exists, .gitignore should be created
    fs::create_dir_all(root.join(".git")).unwrap();
    let _result = scan(
        &root,
        ScanOptions {
            mode: Mode::Smart,
            max_depth: None,
            max_file_size: 1024,
            exclude: Vec::new(),
        },
    ).unwrap();

    let gitignore_path = root.join(".gitignore");
    assert!(gitignore_path.exists());
    let content = fs::read_to_string(&gitignore_path).unwrap();
    assert!(content.contains(".ctx-codegraph/"));
    assert!(content.contains(".ctx_*/"));

    // 2. If .gitignore exists, it should be appended without duplicating/corrupting existing lines
    fs::write(&gitignore_path, "target/\n# Comment\n").unwrap();
    let _result2 = scan(
        &root,
        ScanOptions {
            mode: Mode::Smart,
            max_depth: None,
            max_file_size: 1024,
            exclude: Vec::new(),
        },
    ).unwrap();

    let content2 = fs::read_to_string(&gitignore_path).unwrap();
    assert!(content2.starts_with("target/\n# Comment\n"));
    assert!(content2.contains(".ctx-codegraph/"));
    assert!(content2.contains(".ctx_*/"));
    // Count occurrences
    assert_eq!(content2.matches(".ctx-codegraph/").count(), 1);
    assert_eq!(content2.matches(".ctx_*/").count(), 1);
}

