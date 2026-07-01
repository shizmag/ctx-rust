use std::fs;
use ctx_models::{NodeKind, NodeStats, ProjectSummary, ScanResult, TreeNode};
use ctx_render::{render, Format, RenderOptions};

#[test]
fn test_render_formats() {
    let temp_root = std::env::temp_dir().join("ctx_render_test_project");
    let _ = fs::remove_dir_all(&temp_root);
    fs::create_dir_all(&temp_root).unwrap();

    let file1_path = temp_root.join("hello.py");
    let file2_path = temp_root.join("main.rs");
    
    fs::write(&file1_path, "print('hello')\n").unwrap();
    fs::write(&file2_path, "fn main() {}\n").unwrap();

    let root = TreeNode {
        name: "ctx_render_test_project".to_string(),
        path: temp_root.clone(),
        kind: NodeKind::Directory,
        stats: NodeStats {
            files: 2,
            dirs: 1,
            lines: 2,
            bytes: 28,
        },
        children: vec![
            TreeNode {
                name: "hello.py".to_string(),
                path: file1_path.clone(),
                kind: NodeKind::File,
                stats: NodeStats {
                    files: 1,
                    dirs: 0,
                    lines: 1,
                    bytes: 15,
                },
                children: Vec::new(),
            },
            TreeNode {
                name: "main.rs".to_string(),
                path: file2_path.clone(),
                kind: NodeKind::File,
                stats: NodeStats {
                    files: 1,
                    dirs: 0,
                    lines: 1,
                    bytes: 13,
                },
                children: Vec::new(),
            },
        ],
    };

    let result = ScanResult {
        root,
        summary: ProjectSummary {
            files: 2,
            dirs: 1,
            lines: 2,
            bytes: 28,
            hidden_files: 0,
            hidden_dirs: 0,
        },
        hidden: Vec::new(),
    };

    // Test Markdown
    let md_options = RenderOptions {
        format: Format::Markdown,
        include_stats: true,
        max_file_size: 1024,
    };
    let md_out = render(&result, &md_options).unwrap();
    assert!(md_out.contains("# Project: ctx_render_test_project"));
    assert!(md_out.contains("├── hello.py"));
    assert!(md_out.contains("└── main.rs"));
    assert!(md_out.contains("### `hello.py`"));
    assert!(md_out.contains("```python\nprint('hello')"));
    assert!(md_out.contains("### `main.rs`"));
    assert!(md_out.contains("```rust\nfn main() {}"));

    // Test XML
    let xml_options = RenderOptions {
        format: Format::Xml,
        include_stats: true,
        max_file_size: 1024,
    };
    let xml_out = render(&result, &xml_options).unwrap();
    assert!(xml_out.contains("<project name=\"ctx_render_test_project\">"));
    assert!(xml_out.contains("<file path=\"hello.py\">"));
    assert!(xml_out.contains("print(&apos;hello&apos;)"));
    assert!(xml_out.contains("<file path=\"main.rs\">"));
    assert!(xml_out.contains("fn main() {}"));

    // Test Plain Text
    let plain_options = RenderOptions {
        format: Format::Plain,
        include_stats: true,
        max_file_size: 1024,
    };
    let plain_out = render(&result, &plain_options).unwrap();
    assert!(plain_out.contains("Project: ctx_render_test_project"));
    assert!(plain_out.contains("File: hello.py"));
    assert!(plain_out.contains("print('hello')"));
    assert!(plain_out.contains("File: main.rs"));
    assert!(plain_out.contains("fn main() {}"));

    let _ = fs::remove_dir_all(&temp_root);
}
