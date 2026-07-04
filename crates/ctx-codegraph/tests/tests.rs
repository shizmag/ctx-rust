use ctx_codegraph::*;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn test_parse_rust_code() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("lib.rs");
    let code = r#"
        pub fn run_pipeline() {
            let x = load();
            process(x);
        }

        #[test]
        fn test_helper() {
            save(1);
        }

        impl MyStruct {
            pub fn new() -> Self {
                MyStruct
            }
        }
    "#;
    fs::write(&file_path, code).unwrap();

    let (symbols, call_sites) = languages::rust::parse_rust_file(&file_path).unwrap();

    let run_pipeline = symbols.iter().find(|s| s.name == "run_pipeline").unwrap();
    assert_eq!(run_pipeline.kind, SymbolKind::Function);

    let test_helper = symbols.iter().find(|s| s.name == "test_helper").unwrap();
    assert_eq!(test_helper.kind, SymbolKind::Test);

    let new_method = symbols.iter().find(|s| s.name == "new").unwrap();
    assert_eq!(new_method.kind, SymbolKind::Method);
    assert_eq!(new_method.qualified_name, "MyStruct::new");

    let load_call = call_sites.iter().find(|c| c.raw_name == "load").unwrap();
    assert_eq!(
        load_call.from_temp_index,
        Some(
            symbols
                .iter()
                .position(|s| s.name == "run_pipeline")
                .unwrap()
        )
    );
}

#[test]
fn test_name_only_resolution_and_ambiguity() {
    let symbols = vec![
        Symbol {
            id: Some(SymbolId(0)),
            file_id: None,
            name: "foo".to_string(),
            qualified_name: "mod::foo".to_string(),
            kind: SymbolKind::Function,
            language: Language::Rust,
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
            id: Some(SymbolId(1)),
            file_id: None,
            name: "bar".to_string(),
            qualified_name: "mod1::bar".to_string(),
            kind: SymbolKind::Function,
            language: Language::Rust,
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
            id: Some(SymbolId(2)),
            file_id: None,
            name: "bar".to_string(),
            qualified_name: "mod2::bar".to_string(),
            kind: SymbolKind::Function,
            language: Language::Rust,
            file: PathBuf::from("src/lib.rs"),
            range: TextRange {
                start_line: 11,
                start_col: 1,
                end_line: 15,
                end_col: 1,
            },
            body_range: None,
        },
    ];

    use std::path::Path;
    let (res_idx, res_conf) = resolver::noop::resolve_name_only("foo", &symbols, Path::new("src/lib.rs"));
    assert_eq!(res_idx, Some(0));
    assert_eq!(res_conf, ResolutionConfidence::Syntax);

    let (res_idx_ambig, res_conf_ambig) = resolver::noop::resolve_name_only("bar", &symbols, Path::new("src/lib.rs"));
    assert_eq!(res_idx_ambig, None);
    assert_eq!(res_conf_ambig, ResolutionConfidence::Unresolved);

    let (res_idx_unres, res_conf_unres) = resolver::noop::resolve_name_only("baz", &symbols, Path::new("src/lib.rs"));
    assert_eq!(res_idx_unres, None);
    assert_eq!(res_conf_unres, ResolutionConfidence::Unresolved);
}

#[test]
fn test_sqlite_and_find_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.sqlite");

    let mut conn = rusqlite::Connection::open(&db_path).unwrap();
    storage::init_schema(&conn).unwrap();

    let mut index = CodeIndex {
        root: PathBuf::from("."),
        files: vec![SourceFile {
            id: None,
            path: PathBuf::from("src/lib.rs"),
            language: Language::Rust,
            mtime_ms: Some(12345),
            size_bytes: Some(100),
            content_hash: Some("abc".to_string()),
        }],
        symbols: vec![
            Symbol {
                id: None,
                file_id: None,
                name: "foo".to_string(),
                qualified_name: "mod::foo".to_string(),
                kind: SymbolKind::Function,
                language: Language::Rust,
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
                id: None,
                file_id: None,
                name: "bar_func".to_string(),
                qualified_name: "mod::bar_func".to_string(),
                kind: SymbolKind::Function,
                language: Language::Rust,
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 6,
                    start_col: 1,
                    end_line: 10,
                    end_col: 1,
                },
                body_range: None,
            },
        ],
        call_sites: vec![CallSite {
            id: None,
            file_id: None,
            from: Some(SymbolId(0)),
            from_temp_index: Some(0),
            raw_name: "bar_func".to_string(),
            file: PathBuf::from("src/lib.rs"),
            range: TextRange {
                start_line: 3,
                start_col: 5,
                end_line: 3,
                end_col: 15,
            },
        }],
        edges: vec![CallEdge {
            from: SymbolId(0),
            to: Some(SymbolId(1)),
            call_site_id: Some(CallId(0)),
            raw_name: "bar_func".to_string(),
            call_range: TextRange {
                start_line: 3,
                start_col: 5,
                end_line: 3,
                end_col: 15,
            },
            confidence: ResolutionConfidence::Heuristic,
        }],
    };

    storage::save_index(&mut conn, &mut index).unwrap();
    let loaded = storage::load_index(&conn, Path::new(".")).unwrap();

    assert_eq!(loaded.files.len(), 1);
    assert_eq!(loaded.symbols.len(), 2);
    assert_eq!(loaded.call_sites.len(), 1);
    assert_eq!(loaded.edges.len(), 1);

    let exact_qual = storage::find_symbols(&conn, "mod::foo").unwrap();
    assert_eq!(exact_qual.len(), 1);
    assert_eq!(exact_qual[0].name, "foo");

    let exact_name = storage::find_symbols(&conn, "foo").unwrap();
    assert_eq!(exact_name.len(), 1);
    assert_eq!(exact_name[0].qualified_name, "mod::foo");

    let partial_qual = storage::find_symbols(&conn, "mod::").unwrap();
    assert_eq!(partial_qual.len(), 2);

    let partial_name = storage::find_symbols(&conn, "bar_").unwrap();
    assert_eq!(partial_name.len(), 1);
    assert_eq!(partial_name[0].name, "bar_func");
}

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
                language: Language::Rust,
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
                language: Language::Rust,
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
                language: Language::Rust,
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
        call_sites: vec![],
        edges: vec![
            CallEdge {
                from: SymbolId(0),
                to: Some(SymbolId(1)),
                call_site_id: None,
                raw_name: "b".to_string(),
                call_range: TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 1,
                },
                confidence: ResolutionConfidence::Heuristic,
            },
            CallEdge {
                from: SymbolId(1),
                to: Some(SymbolId(2)),
                call_site_id: None,
                raw_name: "c".to_string(),
                call_range: TextRange {
                    start_line: 3,
                    start_col: 1,
                    end_line: 3,
                    end_col: 1,
                },
                confidence: ResolutionConfidence::Heuristic,
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
fn test_integration_mini_project() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_path = src_dir.join("lib.rs");
    let code = r#"
        pub fn run_pipeline() {
            let x = load();
            process(x);
        }

        fn load() -> i32 { 1 }

        fn process(x: i32) {
            save(x);
        }

        fn save(_: i32) {}
    "#;
    fs::write(&file_path, code).unwrap();

    let (index, _) = rebuild_index_db(
        dir.path(),
        BuildIndexOptions {
            use_rust_analyzer: false,
            max_depth: None,
            include_tests: true,
        },
    )
    .unwrap();

    let run_pipeline_sym = index
        .symbols
        .iter()
        .find(|s| s.name == "run_pipeline")
        .unwrap();
    let run_pipeline_id = run_pipeline_sym.id.unwrap();

    let f_slice = forward_slice(
        &index,
        run_pipeline_id,
        SliceOptions {
            max_depth: 10,
            max_nodes: None,
            include_tests: true,
        },
    );

    let names: Vec<String> = f_slice
        .iter()
        .map(|id| {
            index
                .symbols
                .iter()
                .find(|s| s.id == Some(*id))
                .unwrap()
                .name
                .clone()
        })
        .collect();

    assert!(names.contains(&"run_pipeline".to_string()));
    assert!(names.contains(&"load".to_string()));
    assert!(names.contains(&"process".to_string()));
    assert!(names.contains(&"save".to_string()));

    let conn = storage::open_db(dir.path()).unwrap();
    let loaded_index = storage::load_index(&conn, dir.path()).unwrap();

    let run_pipeline_sym_loaded = loaded_index
        .symbols
        .iter()
        .find(|s| s.name == "run_pipeline")
        .unwrap();
    let run_pipeline_id_loaded = run_pipeline_sym_loaded.id.unwrap();

    let f_slice_loaded = forward_slice(
        &loaded_index,
        run_pipeline_id_loaded,
        SliceOptions {
            max_depth: 10,
            max_nodes: None,
            include_tests: true,
        },
    );
    let names_loaded: Vec<String> = f_slice_loaded
        .iter()
        .map(|id| {
            loaded_index
                .symbols
                .iter()
                .find(|s| s.id == Some(*id))
                .unwrap()
                .name
                .clone()
        })
        .collect();

    assert!(names_loaded.contains(&"run_pipeline".to_string()));
    assert!(names_loaded.contains(&"load".to_string()));
    assert!(names_loaded.contains(&"process".to_string()));
    assert!(names_loaded.contains(&"save".to_string()));
}

#[test]
fn test_integration_with_rust_analyzer() {
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
        BuildIndexOptions {
            use_rust_analyzer: true,
            max_depth: None,
            include_tests: true,
        },
    )
    .unwrap();

    let load_edge = index.edges.iter().find(|e| e.raw_name == "load").unwrap();
    assert_eq!(load_edge.confidence, ResolutionConfidence::LspExact);
}

#[test]
fn test_service_context_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = open_db(dir.path()).unwrap();
    storage::init_schema(&conn).unwrap();

    let mut index = CodeIndex {
        root: dir.path().to_path_buf(),
        files: vec![SourceFile {
            id: None,
            path: PathBuf::from("src/lib.rs"),
            language: Language::Rust,
            mtime_ms: Some(100),
            size_bytes: Some(200),
            content_hash: Some("hash1".to_string()),
        }],
        symbols: vec![
            Symbol {
                id: Some(SymbolId(1)),
                file_id: None,
                name: "a".to_string(),
                qualified_name: "a".to_string(),
                kind: SymbolKind::Function,
                language: Language::Rust,
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
                language: Language::Rust,
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
                language: Language::Rust,
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
                language: Language::Rust,
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
        call_sites: vec![
            CallSite {
                id: Some(CallId(0)),
                file_id: None,
                from: Some(SymbolId(0)),
                from_temp_index: None,
                raw_name: "b".to_string(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 2,
                    start_col: 1,
                    end_line: 2,
                    end_col: 5,
                },
            },
            CallSite {
                id: Some(CallId(1)),
                file_id: None,
                from: Some(SymbolId(1)),
                from_temp_index: None,
                raw_name: "c".to_string(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 7,
                    start_col: 1,
                    end_line: 7,
                    end_col: 5,
                },
            },
            CallSite {
                id: Some(CallId(2)),
                file_id: None,
                from: Some(SymbolId(2)),
                from_temp_index: None,
                raw_name: "d".to_string(),
                file: PathBuf::from("src/lib.rs"),
                range: TextRange {
                    start_line: 12,
                    start_col: 1,
                    end_line: 12,
                    end_col: 5,
                },
            },
        ],
        edges: vec![
            CallEdge {
                from: SymbolId(0),
                to: Some(SymbolId(1)),
                call_site_id: Some(CallId(0)),
                raw_name: "b".to_string(),
                call_range: TextRange {
                    start_line: 2,
                    start_col: 1,
                    end_line: 2,
                    end_col: 5,
                },
                confidence: ResolutionConfidence::LspExact,
            },
            CallEdge {
                from: SymbolId(1),
                to: Some(SymbolId(2)),
                call_site_id: Some(CallId(1)),
                raw_name: "c".to_string(),
                call_range: TextRange {
                    start_line: 7,
                    start_col: 1,
                    end_line: 7,
                    end_col: 5,
                },
                confidence: ResolutionConfidence::LspExact,
            },
            CallEdge {
                from: SymbolId(2),
                to: Some(SymbolId(3)),
                call_site_id: Some(CallId(2)),
                raw_name: "d".to_string(),
                call_range: TextRange {
                    start_line: 12,
                    start_col: 1,
                    end_line: 12,
                    end_col: 5,
                },
                confidence: ResolutionConfidence::LspExact,
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
fn test_index_lifecycle_validation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let git_dir = root.join(".git");
    fs::create_dir_all(&git_dir).unwrap();

    let file_path = root.join("lib.rs");
    fs::write(&file_path, "fn test() {}").unwrap();

    // 1. First call to load_or_build builds index
    let _service = GraphContextService::load_or_build(root).unwrap();
    let db_path = root.join(".ctx-codegraph/codegraph.sqlite");
    assert!(db_path.exists());

    let options = BuildIndexOptions {
        use_rust_analyzer: false,
        max_depth: None,
        include_tests: true,
    };
    let is_valid = validate_index_db(root, &options).unwrap();
    assert!(is_valid);

    // 2. Second call to load_or_build reuses the index
    let _service2 = GraphContextService::load_or_build(root).unwrap();
    assert!(db_path.exists());

    // 3. Cache path is stable for a subdirectory inside the same repo root
    let sub_dir = root.join("src");
    fs::create_dir_all(&sub_dir).unwrap();
    let resolved_root = find_workspace_root(&sub_dir);
    assert_eq!(
        resolved_root.canonicalize().unwrap(),
        root.canonicalize().unwrap()
    );

    // 4. Modifying a file invalidates the index
    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&file_path, "fn test_modified_long_body() { let x = 1; }").unwrap();
    let is_valid_after_mod = validate_index_db(root, &options).unwrap();
    assert!(!is_valid_after_mod);

    // 5. Changing options invalidates cache
    let _service3 = GraphContextService::load_or_build(root).unwrap();
    assert!(validate_index_db(root, &options).unwrap());

    let different_options = BuildIndexOptions {
        use_rust_analyzer: true,
        max_depth: None,
        include_tests: true,
    };
    let is_valid_diff_opts = validate_index_db(root, &different_options).unwrap();
    assert!(!is_valid_diff_opts);

    // 6. Changing schema version invalidates cache
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', '3')",
            [],
        )
        .unwrap();
    }
    let is_valid_diff_schema = validate_index_db(root, &options).unwrap();
    assert!(!is_valid_diff_schema);
}

#[test]
fn test_edge_resolution_quality_variants() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Create Cargo.toml
    fs::write(root.join("Cargo.toml"), "[package]\nname=\"test_proj\"\nversion=\"0.1.0\"\nedition=\"2021\"").unwrap();

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    // Syntax call (local call) and Unresolved call
    let lib_code = r#"
        pub fn a() {
            b();
            unresolved_call();
        }
        pub fn b() {}
    "#;
    fs::write(src_dir.join("lib.rs"), lib_code).unwrap();

    // Heuristic call (cross-file call)
    let other_code = r#"
        pub fn d() {
            b();
        }
    "#;
    fs::write(src_dir.join("other.rs"), other_code).unwrap();

    let (index, report) = rebuild_index_db(
        root,
        BuildIndexOptions {
            use_rust_analyzer: false,
            max_depth: None,
            include_tests: true,
        },
    )
    .unwrap();

    assert!(report.full_rebuild);

    let sym_a = index.symbols.iter().find(|s| s.name == "a").unwrap();
    let sym_d = index.symbols.iter().find(|s| s.name == "d").unwrap();

    // Verify b(); inside a() is Syntax (same file)
    let edge_a_b = index.edges.iter().find(|e| e.raw_name == "b" && e.from == sym_a.id.unwrap()).unwrap();
    assert_eq!(edge_a_b.confidence, ResolutionConfidence::Syntax);

    // Verify unresolved_call(); inside a() is Unresolved
    let edge_unres = index.edges.iter().find(|e| e.raw_name == "unresolved_call" && e.from == sym_a.id.unwrap()).unwrap();
    assert_eq!(edge_unres.confidence, ResolutionConfidence::Unresolved);
    assert!(edge_unres.to.is_none());

    // Verify b(); inside d() is Heuristic (different file)
    let edge_d_b = index.edges.iter().find(|e| e.raw_name == "b" && e.from == sym_d.id.unwrap()).unwrap();
    assert_eq!(edge_d_b.confidence, ResolutionConfidence::Heuristic);
}

#[test]
fn test_incremental_diff_report() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(root.join("Cargo.toml"), "[package]\nname=\"test_proj\"\nversion=\"0.1.0\"\nedition=\"2021\"").unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_lib = src_dir.join("lib.rs");
    let file_a = src_dir.join("a.rs");
    let file_b = src_dir.join("b.rs");

    fs::write(&file_lib, "pub fn run() {}").unwrap();
    fs::write(&file_a, "pub fn a() {}").unwrap();
    fs::write(&file_b, "pub fn b() {}").unwrap();

    let options = BuildIndexOptions {
        use_rust_analyzer: false,
        max_depth: None,
        include_tests: true,
    };

    // 1. Initial build: all files added
    let (_, report1) = rebuild_index_db(root, options.clone()).unwrap();
    assert!(report1.full_rebuild);
    assert_eq!(report1.added_files, 3);
    assert_eq!(report1.modified_files, 0);
    assert_eq!(report1.deleted_files, 0);
    assert_eq!(report1.unchanged_files, 0);

    // 2. Second build: no changes, all files unchanged
    let (_, report2) = rebuild_index_db(root, options.clone()).unwrap();
    assert!(!report2.full_rebuild);
    assert_eq!(report2.added_files, 0);
    assert_eq!(report2.modified_files, 0);
    assert_eq!(report2.deleted_files, 0);
    assert_eq!(report2.unchanged_files, 3);

    // 3. Modify a.rs: only a.rs is modified
    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&file_a, "pub fn a() { // modified\n }").unwrap();
    let (_, report3) = rebuild_index_db(root, options.clone()).unwrap();
    assert!(!report3.full_rebuild);
    assert_eq!(report3.added_files, 0);
    assert_eq!(report3.modified_files, 1);
    assert_eq!(report3.deleted_files, 0);
    assert_eq!(report3.unchanged_files, 2);

    // 4. Add c.rs: only c.rs is added
    let file_c = src_dir.join("c.rs");
    fs::write(&file_c, "pub fn c() {}").unwrap();
    let (_, report4) = rebuild_index_db(root, options.clone()).unwrap();
    assert!(!report4.full_rebuild);
    assert_eq!(report4.added_files, 1);
    assert_eq!(report4.modified_files, 0);
    assert_eq!(report4.deleted_files, 0);
    assert_eq!(report4.unchanged_files, 3);

    // 5. Delete b.rs: only b.rs is deleted
    fs::remove_file(&file_b).unwrap();
    let (_, report5) = rebuild_index_db(root, options.clone()).unwrap();
    assert!(!report5.full_rebuild);
    assert_eq!(report5.added_files, 0);
    assert_eq!(report5.modified_files, 0);
    assert_eq!(report5.deleted_files, 1);
    assert_eq!(report5.unchanged_files, 3);
}

#[test]
fn test_db_correctness_after_incremental_update() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(root.join("Cargo.toml"), "[package]\nname=\"test_proj\"\nversion=\"0.1.0\"\nedition=\"2021\"").unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_lib = src_dir.join("lib.rs");
    let file_b = src_dir.join("b.rs");

    // Scenario 1: Modify file
    // Initial: a calls b
    fs::write(&file_lib, "pub fn a() { b(); }").unwrap();
    fs::write(&file_b, "pub fn b() {}").unwrap();

    let options = BuildIndexOptions {
        use_rust_analyzer: false,
        max_depth: None,
        include_tests: true,
    };

    let (index, _) = rebuild_index_db(root, options.clone()).unwrap();
    assert!(index.symbols.iter().any(|s| s.name == "b"));
    assert!(index.edges.iter().any(|e| e.raw_name == "b" && e.to.is_some()));

    // Modify file: change lib.rs so a calls c instead, and define c in lib.rs
    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&file_lib, "pub fn a() { c(); }\npub fn c() {}").unwrap();

    let (index_mod, _) = rebuild_index_db(root, options.clone()).unwrap();
    assert!(index_mod.symbols.iter().any(|s| s.name == "c"));
    assert!(!index_mod.edges.iter().any(|e| e.raw_name == "b")); // old edge b disappeared
    assert!(index_mod.edges.iter().any(|e| e.raw_name == "c")); // new edge c appeared

    // Scenario 2: Add file
    // Add file d.rs: d calls a
    let file_d = src_dir.join("d.rs");
    fs::write(&file_d, "pub fn d() { a(); }").unwrap();

    let (index_add, _) = rebuild_index_db(root, options.clone()).unwrap();
    assert!(index_add.symbols.iter().any(|s| s.name == "d"));
    assert!(index_add.edges.iter().any(|e| e.raw_name == "a" && e.to.is_some()));

    // Scenario 3: Delete file
    // Delete b.rs
    fs::remove_file(&file_b).unwrap();

    let (index_del, _) = rebuild_index_db(root, options.clone()).unwrap();
    assert!(!index_del.symbols.iter().any(|s| s.name == "b")); // symbol b is gone

    // Scenario 4: Rename/change symbol
    // Change lib.rs from defining c to defining new_name, and calling it
    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&file_lib, "pub fn a() { new_name(); }\npub fn new_name() {}").unwrap();

    let (index_rename, _) = rebuild_index_db(root, options.clone()).unwrap();
    assert!(!index_rename.symbols.iter().any(|s| s.name == "c"));
    assert!(index_rename.symbols.iter().any(|s| s.name == "new_name"));
    assert!(!index_rename.edges.iter().any(|e| e.raw_name == "c"));
    assert!(index_rename.edges.iter().any(|e| e.raw_name == "new_name"));
}

