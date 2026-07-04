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

    let (res_idx, res_conf) = resolver::noop::resolve_name_only("foo", &symbols);
    assert_eq!(res_idx, Some(0));
    assert_eq!(res_conf, ResolutionConfidence::NameOnly);

    let (res_idx_ambig, res_conf_ambig) = resolver::noop::resolve_name_only("bar", &symbols);
    assert_eq!(res_idx_ambig, None);
    assert_eq!(res_conf_ambig, ResolutionConfidence::Ambiguous);

    let (res_idx_unres, res_conf_unres) = resolver::noop::resolve_name_only("baz", &symbols);
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
            confidence: ResolutionConfidence::NameOnly,
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
                confidence: ResolutionConfidence::NameOnly,
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
                confidence: ResolutionConfidence::NameOnly,
            },
        ],
    };

    let f_slice = forward_slice(
        &index,
        SymbolId(0),
        SliceOptions {
            max_depth: 5,
            include_tests: true,
        },
    );
    assert_eq!(f_slice, vec![SymbolId(0), SymbolId(1), SymbolId(2)]);

    let r_slice = reverse_slice(
        &index,
        SymbolId(2),
        SliceOptions {
            max_depth: 5,
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

    let index = rebuild_index_db(
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

    let index = rebuild_index_db(
        dir.path(),
        BuildIndexOptions {
            use_rust_analyzer: true,
            max_depth: None,
            include_tests: true,
        },
    )
    .unwrap();

    let load_edge = index.edges.iter().find(|e| e.raw_name == "load").unwrap();
    assert_eq!(load_edge.confidence, ResolutionConfidence::Exact);
}
