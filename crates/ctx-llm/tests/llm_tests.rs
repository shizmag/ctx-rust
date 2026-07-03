use ctx_llm::build_context;
use ctx_models::{NodeKind, NodeStats, ProjectSummary, ScanResult, TreeNode};
use std::fs;

#[test]
fn test_build_context() {
    let temp_dir = tempfile::tempdir().unwrap();

    let file_path = temp_dir.path().join("hello.txt");
    fs::write(&file_path, "hello world").unwrap();

    let root = TreeNode {
        name: "test".to_string(),
        path: temp_dir.path().to_path_buf(),
        kind: NodeKind::Directory,
        stats: NodeStats {
            files: 1,
            dirs: 1,
            lines: 1,
            bytes: 11,
            tokens: 3,
        },
        children: vec![TreeNode {
            name: "hello.txt".to_string(),
            path: file_path,
            kind: NodeKind::File,
            stats: NodeStats {
                files: 1,
                dirs: 0,
                lines: 1,
                bytes: 11,
                tokens: 3,
            },
            children: vec![],
        }],
    };

    let result = ScanResult {
        root,
        summary: ProjectSummary {
            files: 1,
            dirs: 1,
            lines: 1,
            bytes: 11,
            tokens: 3,
            hidden_files: 0,
            hidden_dirs: 0,
        },
        hidden: vec![],
    };

    let context = build_context(&result, 1024);
    assert!(context.contains("hello world"));
    assert!(context.contains("hello.txt"));
    assert!(context.contains("Total files: 1"));
}
