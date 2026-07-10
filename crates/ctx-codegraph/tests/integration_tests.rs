use ctx_codegraph::*;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

static ENV_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_slices() {
    let index = CodeIndex {
        root: PathBuf::from("."),
        files: vec![],
        symbols: vec![
            Symbol {
                id: Some(SymbolId(0)),
                file_id: None,
                name: "a".to_string(),
                qualified_name: "a".to_string(),
                kind: SymbolKind::Function,
                language: Language::rust(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 2,
                    end_col: 1,
                },
                body_range: None,
            },
            Symbol {
                id: Some(SymbolId(1)),
                file_id: None,
                name: "b".to_string(),
                qualified_name: "b".to_string(),
                kind: SymbolKind::Function,
                language: Language::rust(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 3,
                    start_col: 1,
                    end_line: 4,
                    end_col: 1,
                },
                body_range: None,
            },
            Symbol {
                id: Some(SymbolId(2)),
                file_id: None,
                name: "c".to_string(),
                qualified_name: "c".to_string(),
                kind: SymbolKind::Function,
                language: Language::rust(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 5,
                    start_col: 1,
                    end_line: 6,
                    end_col: 1,
                },
                body_range: None,
            },
        ],
        occurrences: vec![],
        call_sites: vec![],
        edges: vec![
            CallEdge {
                id: None,
                kind: EdgeKind::Call,
                from_file_id: None,
                from_symbol_id: Some(SymbolId(0)),
                to_symbol_id: Some(SymbolId(1)),
                to_external: None,
                occurrence_id: None,
                raw_text: Some("b".to_string()),
                range: Some(TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 1,
                }),
                confidence: ResolutionConfidence::Heuristic,
                produced_by: None,
            },
            CallEdge {
                id: None,
                kind: EdgeKind::Call,
                from_file_id: None,
                from_symbol_id: Some(SymbolId(1)),
                to_symbol_id: Some(SymbolId(2)),
                to_external: None,
                occurrence_id: None,
                raw_text: Some("c".to_string()),
                range: Some(TextRange {
                    start_line: 3,
                    start_col: 1,
                    end_line: 3,
                    end_col: 1,
                }),
                confidence: ResolutionConfidence::Heuristic,
                produced_by: None,
            },
        ],
    };

    let f_slice = forward_slice(
        &index,
        SymbolId(0),
        SliceOptions {
            max_depth: 5,
            max_nodes: None,
            include_tests: true,
        },
    );
    assert_eq!(f_slice, vec![SymbolId(0), SymbolId(1), SymbolId(2)]);

    let r_slice = reverse_slice(
        &index,
        SymbolId(2),
        SliceOptions {
            max_depth: 5,
            max_nodes: None,
            include_tests: true,
        },
    );
    assert_eq!(r_slice, vec![SymbolId(2), SymbolId(1), SymbolId(0)]);
}
#[test]
fn test_integration_with_rust_analyzer() {
    let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    if std::process::Command::new("rust-analyzer")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_err()
    {
        return;
    }

    let dir = tempfile::tempdir().unwrap();

    let cargo_toml = r#"
        [package]
        name = "test-project"
        version = "0.1.0"
        edition = "2021"
    "#;
    fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();

    if std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
    {
        return;
    }

    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_path = src_dir.join("lib.rs");
    let code = r#"
        pub fn run_pipeline() {
            load();
        }

        pub fn load() {}
    "#;
    fs::write(&file_path, code).unwrap();

    let (index, _) = rebuild_index_db(
        dir.path(),
        BuildIndexOptions { use_lsp: true, ..Default::default() },
    )
    .unwrap();

    let load_edge = index
        .edges
        .iter()
        .find(|e| e.raw_text.as_deref() == Some("load"))
        .unwrap();
    assert_eq!(load_edge.confidence, ResolutionConfidence::LspExact);
}
#[test]
fn test_service_context_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = open_db(dir.path()).unwrap();
    storage::init_schema(&conn).unwrap();

    let mut index = CodeIndex {
        root: dir.path().to_path_buf(),
        files: vec![FileSnapshot {
            file_id: None,
            rel_path: PathBuf::from("src/lib.rs"),
            abs_path: dir.path().join("src/lib.rs"),
            language: Language::rust(),
            backend_id: BackendId::new("rust-backend"),
            size_bytes: 200,
            mtime_ms: 100,
            mtime_ns: None,
            content_hash: Some("hash1".to_string()),
            parser_id: ParserId::new("tree-sitter-rust"),
            parser_version: "0.20.0".to_string(),
            parser_config_hash: "".to_string(),
            indexed_at_ms: None,
            parse_status: FileParseStatus::Success,
        }],
        symbols: vec![
            Symbol {
                id: Some(SymbolId(1)),
                file_id: None,
                name: "a".to_string(),
                qualified_name: "a".to_string(),
                kind: SymbolKind::Function,
                language: Language::rust(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 5,
                    end_col: 1,
                },
                body_range: None,
            },
            Symbol {
                id: Some(SymbolId(2)),
                file_id: None,
                name: "b".to_string(),
                qualified_name: "b".to_string(),
                kind: SymbolKind::Function,
                language: Language::rust(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 6,
                    start_col: 1,
                    end_line: 10,
                    end_col: 1,
                },
                body_range: None,
            },
            Symbol {
                id: Some(SymbolId(3)),
                file_id: None,
                name: "c".to_string(),
                qualified_name: "c".to_string(),
                kind: SymbolKind::Function,
                language: Language::rust(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 11,
                    start_col: 1,
                    end_line: 15,
                    end_col: 1,
                },
                body_range: None,
            },
            Symbol {
                id: Some(SymbolId(4)),
                file_id: None,
                name: "d".to_string(),
                qualified_name: "d".to_string(),
                kind: SymbolKind::Function,
                language: Language::rust(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 16,
                    start_col: 1,
                    end_line: 20,
                    end_col: 1,
                },
                body_range: None,
            },
        ],
        occurrences: vec![
            Occurrence {
                id: Some(OccurrenceId(0)),
                file_id: None,
                enclosing_symbol: Some(SymbolId(0)),
                enclosing_temp_index: None,
                kind: OccurrenceKind::Call,
                raw_text: "b".to_string(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 2,
                    start_col: 1,
                    end_line: 2,
                    end_col: 5,
                },
                language: LanguageId::rust(),
                backend_id: BackendId::new("rust-backend"),
            },
            Occurrence {
                id: Some(OccurrenceId(1)),
                file_id: None,
                enclosing_symbol: Some(SymbolId(1)),
                enclosing_temp_index: None,
                kind: OccurrenceKind::Call,
                raw_text: "c".to_string(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 7,
                    start_col: 1,
                    end_line: 7,
                    end_col: 5,
                },
                language: LanguageId::rust(),
                backend_id: BackendId::new("rust-backend"),
            },
            Occurrence {
                id: Some(OccurrenceId(2)),
                file_id: None,
                enclosing_symbol: Some(SymbolId(2)),
                enclosing_temp_index: None,
                kind: OccurrenceKind::Call,
                raw_text: "d".to_string(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 12,
                    start_col: 1,
                    end_line: 12,
                    end_col: 5,
                },
                language: LanguageId::rust(),
                backend_id: BackendId::new("rust-backend"),
            },
        ],
        call_sites: vec![
            Occurrence {
                id: Some(OccurrenceId(0)),
                file_id: None,
                enclosing_symbol: Some(SymbolId(0)),
                enclosing_temp_index: None,
                kind: OccurrenceKind::Call,
                raw_text: "b".to_string(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 2,
                    start_col: 1,
                    end_line: 2,
                    end_col: 5,
                },
                language: LanguageId::rust(),
                backend_id: BackendId::new("rust-backend"),
            },
            Occurrence {
                id: Some(OccurrenceId(1)),
                file_id: None,
                enclosing_symbol: Some(SymbolId(1)),
                enclosing_temp_index: None,
                kind: OccurrenceKind::Call,
                raw_text: "c".to_string(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 7,
                    start_col: 1,
                    end_line: 7,
                    end_col: 5,
                },
                language: LanguageId::rust(),
                backend_id: BackendId::new("rust-backend"),
            },
            Occurrence {
                id: Some(OccurrenceId(2)),
                file_id: None,
                enclosing_symbol: Some(SymbolId(2)),
                enclosing_temp_index: None,
                kind: OccurrenceKind::Call,
                raw_text: "d".to_string(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 12,
                    start_col: 1,
                    end_line: 12,
                    end_col: 5,
                },
                language: LanguageId::rust(),
                backend_id: BackendId::new("rust-backend"),
            },
        ],
        edges: vec![
            CallEdge {
                id: None,
                kind: EdgeKind::Call,
                from_file_id: None,
                from_symbol_id: Some(SymbolId(0)),
                to_symbol_id: Some(SymbolId(1)),
                to_external: None,
                occurrence_id: Some(OccurrenceId(0)),
                raw_text: Some("b".to_string()),
                range: Some(TextRange {
                    start_line: 2,
                    start_col: 1,
                    end_line: 2,
                    end_col: 5,
                }),
                confidence: ResolutionConfidence::LspExact,
                produced_by: None,
            },
            CallEdge {
                id: None,
                kind: EdgeKind::Call,
                from_file_id: None,
                from_symbol_id: Some(SymbolId(1)),
                to_symbol_id: Some(SymbolId(2)),
                to_external: None,
                occurrence_id: Some(OccurrenceId(1)),
                raw_text: Some("c".to_string()),
                range: Some(TextRange {
                    start_line: 7,
                    start_col: 1,
                    end_line: 7,
                    end_col: 5,
                }),
                confidence: ResolutionConfidence::LspExact,
                produced_by: None,
            },
            CallEdge {
                id: None,
                kind: EdgeKind::Call,
                from_file_id: None,
                from_symbol_id: Some(SymbolId(2)),
                to_symbol_id: Some(SymbolId(3)),
                to_external: None,
                occurrence_id: Some(OccurrenceId(2)),
                raw_text: Some("d".to_string()),
                range: Some(TextRange {
                    start_line: 12,
                    start_col: 1,
                    end_line: 12,
                    end_col: 5,
                }),
                confidence: ResolutionConfidence::LspExact,
                produced_by: None,
            },
        ],
    };
    storage::save_index(&mut conn, &mut index).unwrap();

    let service = GraphContextService::new(dir.path(), conn);

    // 1. service на fixture-графе строит context для a в режиме Callees
    let res_callees = service
        .build_context_for_symbol(
            SymbolId(1),
            GraphContextOptions {
                mode: GraphContextMode::Callees,
                max_depth: 2,
                max_nodes: 10,
                include_root: true,
            },
        )
        .unwrap();
    assert_eq!(res_callees.root.name, "a");
    assert_eq!(res_callees.nodes.len(), 3); // a, b, c
    assert!(res_callees.nodes.iter().any(|n| n.name == "a"));
    assert!(res_callees.nodes.iter().any(|n| n.name == "b"));
    assert!(res_callees.nodes.iter().any(|n| n.name == "c"));
    assert_eq!(res_callees.edges.len(), 2); // a -> b, b -> c

    // 2. service на fixture-графе строит context для b в режиме Callers
    let res_callers = service
        .build_context_for_symbol(
            SymbolId(2),
            GraphContextOptions {
                mode: GraphContextMode::Callers,
                max_depth: 2,
                max_nodes: 10,
                include_root: true,
            },
        )
        .unwrap();
    assert_eq!(res_callers.root.name, "b");
    assert_eq!(res_callers.nodes.len(), 2); // b, a (since a calls b)
    assert!(res_callers.nodes.iter().any(|n| n.name == "b"));
    assert!(res_callers.nodes.iter().any(|n| n.name == "a"));

    // 3. include_root = false исключает root symbol из nodes, но root остаётся в metadata (res_callees.root)
    let res_no_root = service
        .build_context_for_symbol(
            SymbolId(1),
            GraphContextOptions {
                mode: GraphContextMode::Callees,
                max_depth: 2,
                max_nodes: 10,
                include_root: false,
            },
        )
        .unwrap();
    assert_eq!(res_no_root.root.name, "a");
    assert_eq!(res_no_root.nodes.len(), 2); // b, c (no a)
    assert!(!res_no_root.nodes.iter().any(|n| n.name == "a"));
    assert!(res_no_root.nodes.iter().any(|n| n.name == "b"));
    assert!(res_no_root.nodes.iter().any(|n| n.name == "c"));

    // 4. max_depth работает
    let res_depth_1 = service
        .build_context_for_symbol(
            SymbolId(1),
            GraphContextOptions {
                mode: GraphContextMode::Callees,
                max_depth: 1,
                max_nodes: 10,
                include_root: true,
            },
        )
        .unwrap();
    assert_eq!(res_depth_1.nodes.len(), 2); // a, b (c is at depth 2)
    assert!(res_depth_1.nodes.iter().any(|n| n.name == "a"));
    assert!(res_depth_1.nodes.iter().any(|n| n.name == "b"));

    // 5. max_nodes работает
    let res_nodes_2 = service
        .build_context_for_symbol(
            SymbolId(1),
            GraphContextOptions {
                mode: GraphContextMode::Callees,
                max_depth: 2,
                max_nodes: 2,
                include_root: true,
            },
        )
        .unwrap();
    assert_eq!(res_nodes_2.nodes.len(), 2); // truncated to 2 nodes
    assert!(!res_nodes_2.diagnostics.is_empty());
    assert!(
        res_nodes_2
            .diagnostics
            .iter()
            .any(|d| d.severity == "warning" && d.message.contains("max_nodes limit"))
    );
}
#[test]
fn test_mock_lsp_exact_enrichment() {
    let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    use std::fs::{self, File};
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = tempfile::tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    // Write python mock rust-analyzer
    let script_path = bin_dir.join("rust-analyzer");
    let script_content = r#"#!/usr/bin/env python3
import sys
import json

def log(msg):
    sys.stderr.write(f"MOCK LOG: {msg}\n")
    sys.stderr.flush()

def write_response(id, result):
    response = {
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    }
    msg = json.dumps(response)
    sys.stdout.write(f"Content-Length: {len(msg)}\r\n\r\n{msg}")
    sys.stdout.flush()

content_buf = ""
while True:
    line = sys.stdin.readline()
    if not line:
        break
    if line.startswith("Content-Length:"):
        length = int(line.split(":")[1].strip())
        # read the empty line \r\n
        sys.stdin.readline()
        # read content of length bytes
        content = sys.stdin.read(length)
        req = json.loads(content)
        method = req.get("method")
        req_id = req.get("id")
        
        if method == "initialize":
            write_response(req_id, {"capabilities": {"textDocumentSync": 1}})
        elif method == "textDocument/definition":
            uri = req.get("params", {}).get("textDocument", {}).get("uri")
            if uri:
                write_response(req_id, [
                    {
                        "uri": uri,
                        "range": {
                            "start": {"line": 4, "character": 3},
                            "end": {"line": 4, "character": 4}
                        }
                    }
                ])
            else:
                write_response(req_id, [])
"#;

    {
        let mut file = File::create(&script_path).unwrap();
        file.write_all(script_content.as_bytes()).unwrap();
    }

    // Make script executable
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    // Set PATH to include bin_dir
    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", bin_dir.display(), old_path);
    unsafe {
        std::env::set_var("PATH", &new_path);
    }

    // Create a mock cargo workspace
    let proj_dir = temp_dir.path().join("mock_project");
    fs::create_dir_all(&proj_dir).unwrap();
    fs::write(
        proj_dir.join("Cargo.toml"),
        "[package]\nname=\"mock_project\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();

    let src_dir = proj_dir.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    // Write lib.rs where:
    // fn a() { b(); } is at line 2 (start line 2)
    // fn b() {} is at line 5 (start line 5, col 9)
    let lib_code = "fn a() {\n    b();\n}\n\nfn b() {}\n";
    fs::write(src_dir.join("lib.rs"), lib_code).unwrap();

    // Run build index with lsp
    let options = BuildIndexOptions {
        use_lsp: true,
        ..Default::default()
    };

    let (index, report) = rebuild_index_db(&proj_dir, options).unwrap();

    // Restore PATH
    unsafe {
        std::env::set_var("PATH", old_path);
    }

    // Verify that edge resolved to LspExact!
    assert!(report.full_rebuild);
    assert_eq!(report.lsp_edges_exact, 1);

    let edge = index
        .edges
        .iter()
        .find(|e| e.raw_text.as_deref() == Some("b"))
        .unwrap();
    assert_eq!(edge.confidence, ResolutionConfidence::LspExact);
    assert!(edge.to_symbol_id.is_some());
}
#[test]
fn test_lsp_failure_fallback() {
    let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    use std::fs::{self, File};
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = tempfile::tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    // Write a mock rust-analyzer that returns a JSON-RPC Error instead of a definition
    let script_path = bin_dir.join("rust-analyzer");
    let script_content = r#"#!/usr/bin/env python3
import sys
import json

content_buf = ""
while True:
    line = sys.stdin.readline()
    if not line:
        break
    if line.startswith("Content-Length:"):
        length = int(line.split(":")[1].strip())
        sys.stdin.readline()
        content = sys.stdin.read(length)
        req = json.loads(content)
        method = req.get("method")
        req_id = req.get("id")
        
        if method == "initialize":
            sys.stdout.write(f"Content-Length: {len(json.dumps({'jsonrpc': '2.0', 'id': req_id, 'result': {'capabilities': {}}}))}\r\n\r\n{json.dumps({'jsonrpc': '2.0', 'id': req_id, 'result': {'capabilities': {}}})}")
            sys.stdout.flush()
        elif method == "textDocument/definition":
            # Use an error code that does *not* match the warmup retry condition
            # in the resolver (which looks for "-32603"). This ensures we take
            # the fast path (no long retry loop) while still exercising the
            # "LSP responded with error => fallback" behavior.
            err_response = {
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {
                    "code": -32000,
                    "message": "Simulated LSP failure for fallback test"
                }
            }
            msg = json.dumps(err_response)
            sys.stdout.write(f"Content-Length: {len(msg)}\r\n\r\n{msg}")
            sys.stdout.flush()
"#;

    {
        let mut file = File::create(&script_path).unwrap();
        file.write_all(script_content.as_bytes()).unwrap();
    }

    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", bin_dir.display(), old_path);
    unsafe {
        std::env::set_var("PATH", &new_path);
    }

    let proj_dir = temp_dir.path().join("mock_project");
    fs::create_dir_all(&proj_dir).unwrap();
    fs::write(
        proj_dir.join("Cargo.toml"),
        "[package]\nname=\"mock_project\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();

    let src_dir = proj_dir.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("lib.rs"), "fn a() { b(); }\nfn b() {}\n").unwrap();

    let options = BuildIndexOptions {
        use_lsp: true,
        ..Default::default()
    };

    let (index, report) = rebuild_index_db(&proj_dir, options).unwrap();

    unsafe {
        std::env::set_var("PATH", &old_path);
    }

    assert!(report.full_rebuild);
    assert_eq!(report.lsp_edges_exact, 0);

    let edge = index
        .edges
        .iter()
        .find(|e| e.raw_text.as_deref() == Some("b"))
        .unwrap();
    assert_eq!(edge.confidence, ResolutionConfidence::Syntax);
    assert!(edge.to_symbol_id.is_some());
}
