use ctx_models::{NodeKind, NodeStats, ProjectSummary, ScanResult, TreeNode};
use ctx_render::{Format, RenderOptions, render};
use std::fs;

#[test]
fn test_render_formats() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_root = temp_dir.path().to_path_buf();

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
            tokens: 7,
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
                    tokens: 4,
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
                    tokens: 3,
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
            tokens: 7,
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
}

#[test]
fn test_render_edge_cases() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_root = temp_dir.path().to_path_buf();

    let file_special = temp_root.join("special.xml");
    let file_multiline = temp_root.join("multiline.txt");
    let file_empty = temp_root.join("empty.txt");
    let file_no_newline = temp_root.join("no_newline.txt");
    let file_fences = temp_root.join("code_fences.md");

    fs::write(&file_special, "<hello name=\"world\"> & 'rust'</hello>").unwrap();
    fs::write(&file_multiline, "line 1\n  line 2 with spaces\nline 3\n").unwrap();
    fs::write(&file_empty, "").unwrap();
    fs::write(&file_no_newline, "no trailing newline").unwrap();
    fs::write(
        &file_fences,
        "Some text\n```rust\nfn main() {}\n```\nMore text\n````javascript\nconsole.log(1);\n````\n",
    )
    .unwrap();

    let root = TreeNode {
        name: "ctx_render_edge_cases".to_string(),
        path: temp_root.clone(),
        kind: NodeKind::Directory,
        stats: NodeStats::default(),
        children: vec![
            TreeNode {
                name: "special.xml".to_string(),
                path: file_special.clone(),
                kind: NodeKind::File,
                stats: NodeStats::default(),
                children: Vec::new(),
            },
            TreeNode {
                name: "multiline.txt".to_string(),
                path: file_multiline.clone(),
                kind: NodeKind::File,
                stats: NodeStats::default(),
                children: Vec::new(),
            },
            TreeNode {
                name: "empty.txt".to_string(),
                path: file_empty.clone(),
                kind: NodeKind::File,
                stats: NodeStats::default(),
                children: Vec::new(),
            },
            TreeNode {
                name: "no_newline.txt".to_string(),
                path: file_no_newline.clone(),
                kind: NodeKind::File,
                stats: NodeStats::default(),
                children: Vec::new(),
            },
            TreeNode {
                name: "code_fences.md".to_string(),
                path: file_fences.clone(),
                kind: NodeKind::File,
                stats: NodeStats::default(),
                children: Vec::new(),
            },
        ],
    };

    let result = ScanResult {
        root,
        summary: ProjectSummary::default(),
        hidden: Vec::new(),
    };

    // 1. Test XML Renderer edge cases
    let xml_options = RenderOptions {
        format: Format::Xml,
        include_stats: false,
        max_file_size: 1024,
    };
    let xml_out = render(&result, &xml_options).unwrap();

    // Verify XML Special Chars escaping
    assert!(
        xml_out
            .contains("&lt;hello name=&quot;world&quot;&gt; &amp; &apos;rust&apos;&lt;/hello&gt;")
    );

    // Verify Multiline content preserves exact leading spaces and structure without extra prefixing spaces
    assert!(xml_out.contains("<content>line 1\n  line 2 with spaces\nline 3\n</content>"));

    // Verify Empty file content
    assert!(xml_out.contains("<file path=\"empty.txt\">\n      <content></content>"));

    // Verify File without trailing newline is preserved exactly
    assert!(
        xml_out.contains(
            "<file path=\"no_newline.txt\">\n      <content>no trailing newline</content>"
        )
    );

    // 2. Test Markdown Renderer dynamic fence
    let md_options = RenderOptions {
        format: Format::Markdown,
        include_stats: false,
        max_file_size: 1024,
    };
    let md_out = render(&result, &md_options).unwrap();

    // For file_fences, it contains both ``` and ````.
    // The maximum consecutive backticks count is 4.
    // So the enclosing fence must be 5 backticks: `````
    let expected_fence = "`````";
    assert!(md_out.contains(&format!("### `code_fences.md`\n{}markdown\nSome text\n```rust\nfn main() {{}}\n```\nMore text\n````javascript\nconsole.log(1);\n````\n{}\n", expected_fence, expected_fence)));

    // Verify empty file markdown
    assert!(md_out.contains("### `empty.txt`\n```text\n\n```\n"));

    // Verify no trailing newline markdown has a trailing newline added before closing fence
    assert!(md_out.contains("### `no_newline.txt`\n```text\nno trailing newline\n```\n"));
}

#[test]
fn test_render_golden_snapshot() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_root = temp_dir.path().to_path_buf();

    let a_py = temp_root.join("a.py");
    let b_rs = temp_root.join("b.rs");

    fs::write(&a_py, "print(\"hello\")\n").unwrap();
    fs::write(&b_rs, "fn main() {\n    println!(\"world\");\n}\n").unwrap();

    let root = TreeNode {
        name: "golden_project".to_string(),
        path: temp_root.clone(),
        kind: NodeKind::Directory,
        stats: NodeStats {
            files: 2,
            dirs: 1,
            lines: 5,
            bytes: 52,
            tokens: 15,
        },
        children: vec![
            TreeNode {
                name: "a.py".to_string(),
                path: a_py,
                kind: NodeKind::File,
                stats: NodeStats {
                    files: 1,
                    dirs: 0,
                    lines: 1,
                    bytes: 15,
                    tokens: 5,
                },
                children: Vec::new(),
            },
            TreeNode {
                name: "b.rs".to_string(),
                path: b_rs,
                kind: NodeKind::File,
                stats: NodeStats {
                    files: 1,
                    dirs: 0,
                    lines: 4,
                    bytes: 37,
                    tokens: 10,
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
            lines: 5,
            bytes: 52,
            tokens: 15,
            hidden_files: 0,
            hidden_dirs: 0,
        },
        hidden: Vec::new(),
    };

    let md_options = RenderOptions {
        format: Format::Markdown,
        include_stats: true,
        max_file_size: 1024,
    };
    let md_out = render(&result, &md_options).unwrap();

    // Replace the dynamically-generated temp directory name in the output with a fixed string "golden_project"
    let temp_name = temp_root.file_name().unwrap().to_str().unwrap();
    let normalized_md_out = md_out.replace(temp_name, "golden_project");

    let expected_markdown_golden = r#"# Project: golden_project

## Project Summary
- **Files**: 2
- **Directories**: 1
- **Total Lines**: 5
- **Total Size**: 52 B

## Directory Structure
```text
golden_project
├── a.py
└── b.rs
```

## Repository Files

### `a.py`
```python
print("hello")
```

### `b.rs`
```rust
fn main() {
    println!("world");
}
```

"#;

    assert_eq!(normalized_md_out, expected_markdown_golden);
}
