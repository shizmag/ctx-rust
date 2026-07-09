use std::fs;

use ctx_core::scan;
use ctx_models::{HiddenReason, Mode, ScanOptions};

fn scan_with_mode(root: &std::path::Path, mode: Mode) -> ctx_models::ScanResult {
    scan(
        root,
        ScanOptions {
            mode,
            max_depth: None,
            max_file_size: 1024 * 1024,
            exclude: Vec::new(),
        },
    )
    .unwrap()
}

#[test]
fn scan_code_mode_keeps_code_and_hides_docs() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("README.md"), "# Project\n").unwrap();
    fs::write(root.join("notes.txt"), "plain text\n").unwrap();
    fs::write(root.join("image.png"), &[0x89, 0x50, 0x4E, 0x47]).unwrap();

    let result = scan_with_mode(root, Mode::Code);

    assert_eq!(result.summary.files, 2); // main.rs + README.md
    assert_eq!(result.summary.hidden_files, 2); // notes.txt + image.png

    let root_children: Vec<_> = result
        .root
        .children
        .iter()
        .map(|node| node.name.as_str())
        .collect();
    assert!(root_children.contains(&"src"));
    assert!(root_children.contains(&"README.md"));
    assert!(!root_children.contains(&"notes.txt"));
    assert!(!root_children.contains(&"image.png"));

    assert!(
        result
            .hidden
            .iter()
            .any(|item| item.path.ends_with("notes.txt") && item.reason == HiddenReason::NonCode)
    );
}

#[test]
fn scan_docs_mode_keeps_docs_and_hides_code() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    fs::write(root.join("guide.md"), "# Guide\n").unwrap();
    fs::write(root.join("notes.txt"), "notes\n").unwrap();
    fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();

    let result = scan_with_mode(root, Mode::Docs);

    assert_eq!(result.summary.files, 2); // guide.md + notes.txt
    assert_eq!(result.summary.hidden_files, 2); // main.rs + Cargo.toml

    let root_children: Vec<_> = result
        .root
        .children
        .iter()
        .map(|node| node.name.as_str())
        .collect();
    assert!(root_children.contains(&"guide.md"));
    assert!(root_children.contains(&"notes.txt"));
    assert!(!root_children.contains(&"main.rs"));
    assert!(!root_children.contains(&"Cargo.toml"));
}

#[test]
fn scan_llm_mode_keeps_text_and_hides_binary_extensions() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    fs::write(root.join("context.md"), "# Context\n").unwrap();
    fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("logo.png"), &[0x89, 0x50, 0x4E, 0x47]).unwrap();
    fs::write(root.join("archive.zip"), &[0x50, 0x4B, 0x03, 0x04]).unwrap();

    let result = scan_with_mode(root, Mode::Llm);

    assert_eq!(result.summary.files, 2); // context.md + main.rs
    assert_eq!(result.summary.hidden_files, 2); // logo.png + archive.zip

    assert!(
        result
            .hidden
            .iter()
            .any(|item| item.path.ends_with("logo.png") && item.reason == HiddenReason::Binary)
    );
    assert!(
        result
            .hidden
            .iter()
            .any(|item| item.path.ends_with("archive.zip") && item.reason == HiddenReason::Binary)
    );
}

#[test]
fn scan_all_mode_disables_builtin_hiding() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    fs::create_dir_all(root.join(".git/objects")).unwrap();
    fs::create_dir_all(root.join("target/debug")).unwrap();
    fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join(".git/config"), "git\n").unwrap();
    fs::write(root.join("target/debug/app"), "bin\n").unwrap();
    fs::write(root.join("node_modules/pkg/index.js"), "module\n").unwrap();
    fs::write(root.join("package-lock.json"), "{}\n").unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

    let result = scan_with_mode(root, Mode::All);

    assert_eq!(result.summary.files, 6);
    assert_eq!(result.summary.dirs, 7);
    assert_eq!(result.summary.hidden_files, 0);
    assert_eq!(result.summary.hidden_dirs, 0);

    let root_children: Vec<_> = result
        .root
        .children
        .iter()
        .map(|node| node.name.as_str())
        .collect();
    assert!(root_children.contains(&".git"));
    assert!(root_children.contains(&"target"));
    assert!(root_children.contains(&"node_modules"));
    assert!(root_children.contains(&"package-lock.json"));
}