use ctx_codegraph_lang::backend::{BackendId, ParserId};
use ctx_codegraph_lang::error::CodeGraphError;
use ctx_codegraph_lang::index::{BuildIndexOptions, get_mtime_ms};
use ctx_codegraph_lang::model::*;
use ctx_codegraph_store::storage::{
    ensure_index_with_registry, find_symbols, find_workspace_root, get_index_state_with_registry,
    init_schema, load_index, open_db, rebuild_index_db_with_registry, save_index,
    validate_index_db_with_registry,
};
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::production_registry;

#[test]
fn test_sqlite_and_find_symbols() {
    let registry = production_registry();
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.sqlite");

    let mut conn = rusqlite::Connection::open(&db_path).unwrap();
    init_schema(&conn, &registry).unwrap();

    let mut index = CodeIndex {
        root: PathBuf::from("."),
        files: vec![FileSnapshot {
            file_id: None,
            rel_path: PathBuf::from("src/lib.rs"),
            abs_path: PathBuf::from("src/lib.rs"),
            language: Language::rust(),
            backend_id: BackendId::new("rust-backend"),
            size_bytes: 100,
            mtime_ms: 12345,
            mtime_ns: None,
            content_hash: Some("abc".to_string()),
            parser_id: ParserId::new("tree-sitter-rust"),
            parser_version: "0.20.0".to_string(),
            parser_config_hash: "".to_string(),
            indexed_at_ms: None,
            parse_status: FileParseStatus::Success,
        }],
        symbols: vec![
            Symbol {
                id: None,
                file_id: None,
                name: "foo".to_string(),
                qualified_name: "mod::foo".to_string(),
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
                id: None,
                file_id: None,
                name: "bar_func".to_string(),
                qualified_name: "mod::bar_func".to_string(),
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
        ],
        occurrences: vec![Occurrence {
            id: None,
            file_id: None,
            enclosing_symbol: Some(SymbolId(0)),
            enclosing_temp_index: Some(0),
            kind: OccurrenceKind::Call,
            raw_text: "bar_func".to_string(),
            file: PathBuf::from("src/lib.rs"),
            range: TextRange {
                start_line: 3,
                start_col: 5,
                end_line: 3,
                end_col: 15,
            },
            language: LanguageId::rust(),
            backend_id: BackendId::new("rust-backend"),
        }],
        call_sites: vec![Occurrence {
            id: None,
            file_id: None,
            enclosing_symbol: Some(SymbolId(0)),
            enclosing_temp_index: Some(0),
            kind: OccurrenceKind::Call,
            raw_text: "bar_func".to_string(),
            file: PathBuf::from("src/lib.rs"),
            range: TextRange {
                start_line: 3,
                start_col: 5,
                end_line: 3,
                end_col: 15,
            },
            language: LanguageId::rust(),
            backend_id: BackendId::new("rust-backend"),
        }],
        edges: vec![CallEdge {
            id: None,
            kind: EdgeKind::Call,
            from_file_id: None,
            from_symbol_id: Some(SymbolId(0)),
            to_symbol_id: Some(SymbolId(1)),
            to_external: None,
            occurrence_id: Some(OccurrenceId(0)),
            raw_text: Some("bar_func".to_string()),
            range: Some(TextRange {
                start_line: 3,
                start_col: 5,
                end_line: 3,
                end_col: 15,
            }),
            confidence: ResolutionConfidence::Heuristic,
            produced_by: None,
        }],
    };

    save_index(&mut conn, &mut index).unwrap();
    let loaded = load_index(&conn, Path::new(".")).unwrap();

    assert_eq!(loaded.files.len(), 1);
    assert_eq!(loaded.symbols.len(), 2);
    assert_eq!(loaded.call_sites.len(), 1);
    assert_eq!(loaded.edges.len(), 1);

    let exact_qual = find_symbols(&conn, "mod::foo").unwrap();
    assert_eq!(exact_qual.len(), 1);
    assert_eq!(exact_qual[0].name, "foo");

    let exact_name = find_symbols(&conn, "foo").unwrap();
    assert_eq!(exact_name.len(), 1);
    assert_eq!(exact_name[0].qualified_name, "mod::foo");

    let partial_qual = find_symbols(&conn, "mod::").unwrap();
    assert_eq!(partial_qual.len(), 2);

    let partial_name = find_symbols(&conn, "bar_").unwrap();
    assert_eq!(partial_name.len(), 1);
    assert_eq!(partial_name[0].name, "bar_func");
}

#[test]
fn test_affected_set_callee_pulls_callers() {
    let registry = production_registry();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"test_proj\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_lib = src_dir.join("lib.rs");
    let file_b = src_dir.join("b.rs");

    fs::write(&file_lib, "pub fn a() { b(); }").unwrap();
    fs::write(&file_b, "pub fn b() {}").unwrap();

    let options = BuildIndexOptions::default();

    // Build the initial index
    let (index, _) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    let edge = index
        .edges
        .iter()
        .find(|e| e.raw_text.as_deref() == Some("b"))
        .unwrap();
    assert!(edge.to_symbol_id.is_some()); // resolved to b()

    // Now modify b.rs
    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&file_b, "pub fn b() { let modified = 1; }").unwrap();

    // Run incremental build
    let (index2, report) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert_eq!(report.modified_files, 1); // b.rs modified
    assert_eq!(report.unchanged_files, 1); // lib.rs unchanged

    // Check if the edge calling b is still resolved correctly
    let edge2 = index2
        .edges
        .iter()
        .find(|e| e.raw_text.as_deref() == Some("b"))
        .unwrap();
    assert!(edge2.to_symbol_id.is_some());
}
#[test]
fn test_db_transaction_rollback_on_failure() {
    let registry = production_registry();
    let dir = tempfile::tempdir().unwrap();
    let mut conn = open_db(dir.path(), &registry).unwrap();
    init_schema(&conn, &registry).unwrap();

    let mut index = CodeIndex {
        root: dir.path().to_path_buf(),
        files: vec![FileSnapshot {
            file_id: None,
            rel_path: PathBuf::from("lib.rs"),
            abs_path: dir.path().join("lib.rs"),
            language: Language::rust(),
            backend_id: BackendId::new("rust-backend"),
            size_bytes: 100,
            mtime_ms: 100,
            mtime_ns: None,
            content_hash: Some("hash1".to_string()),
            parser_id: ParserId::new("tree-sitter-rust"),
            parser_version: "0.20.0".to_string(),
            parser_config_hash: "".to_string(),
            indexed_at_ms: None,
            parse_status: FileParseStatus::Success,
        }],
        symbols: vec![],
        occurrences: vec![],
        call_sites: vec![],
        edges: vec![],
    };
    save_index(&mut conn, &mut index).unwrap();

    let run_failed_transaction =
        |conn: &mut rusqlite::Connection| -> Result<(), CodeGraphError> {
            let tx = conn.transaction()?;
            tx.execute("INSERT INTO symbols (file_id, name, qualified_name, kind, language, start_line, start_col, end_line, end_col) VALUES (1, 'foo', 'foo', 'function', 'rust', 1, 1, 1, 1)", [])?;
            tx.execute("INSERT INTO files (path, rel_path, language, backend_id, mtime_ms, size_bytes) VALUES (?, ?, ?, ?, ?, ?)", 
                   rusqlite::params![dir.path().join("lib.rs").to_string_lossy().to_string(), "lib.rs", "rust", "rust-backend", 100, 100])?;
            tx.commit()?;
            Ok(())
        };

    let res = run_failed_transaction(&mut conn);
    assert!(res.is_err());

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols WHERE name = 'foo'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "Changes were committed despite error!");
}
#[test]
fn test_empty_and_whitespace_only_files() {
    let registry = production_registry();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"test_proj\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file1 = src_dir.join("lib.rs");
    let file2 = src_dir.join("empty.rs");
    let file3 = src_dir.join("whitespace.rs");

    fs::write(&file1, "pub fn a() {}").unwrap();
    fs::write(&file2, "").unwrap();
    fs::write(&file3, "   \n  \n\t ").unwrap();

    let options = BuildIndexOptions::default();

    let (index, report) = rebuild_index_db_with_registry(root, options, &registry).unwrap();
    assert_eq!(report.parsed_files, 3);
    assert!(index.symbols.iter().any(|s| s.name == "a"));
    assert_eq!(index.symbols.len(), 1);
}
#[test]
fn test_multiple_simultaneous_changes() {
    let registry = production_registry();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"test_proj\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_a = src_dir.join("a.rs");
    let file_b = src_dir.join("b.rs");
    let file_c = src_dir.join("c.rs");

    fs::write(&file_a, "pub fn a() {}").unwrap();
    fs::write(&file_b, "pub fn b() {}").unwrap();

    let options = BuildIndexOptions::default();

    rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();

    fs::remove_file(&file_a).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&file_b, "pub fn b_new() {}").unwrap();
    fs::write(&file_c, "pub fn c() {}").unwrap();

    let (index2, report) = rebuild_index_db_with_registry(root, options, &registry).unwrap();

    assert_eq!(report.deleted_files, 1);
    assert_eq!(report.modified_files, 1);
    assert_eq!(report.added_files, 1);

    let symbol_names: std::collections::HashSet<String> =
        index2.symbols.iter().map(|s| s.name.clone()).collect();
    assert!(!symbol_names.contains("a"));
    assert!(!symbol_names.contains("b"));
    assert!(symbol_names.contains("b_new"));
    assert!(symbol_names.contains("c"));
}
#[test]
fn test_parse_failure_recovery() {
    let registry = production_registry();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"test_proj\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_path = src_dir.join("lib.rs");
    fs::write(&file_path, "pub fn a() {}").unwrap();

    let options = BuildIndexOptions::default();

    let (index1, _) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert!(index1.symbols.iter().any(|s| s.name == "a"));

    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&file_path, "fn a( {").unwrap();
    let (index2, _) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert!(index2.symbols.iter().any(|s| s.name == "a"));

    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&file_path, "pub fn b() {}").unwrap();
    let (index3, report3) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert_eq!(report3.modified_files, 1);

    assert!(index3.symbols.iter().any(|s| s.name == "b"));
    assert!(!index3.symbols.iter().any(|s| s.name == "a"));
}
#[test]
fn test_index_lifecycle_validation() {
    let registry = production_registry();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let git_dir = root.join(".git");
    fs::create_dir_all(&git_dir).unwrap();

    let file_path = root.join("lib.rs");
    fs::write(&file_path, "fn test() {}").unwrap();

    let options = BuildIndexOptions::default();

    // 1. First call builds index
    let _conn = ensure_index_with_registry(root, options.clone(), &registry).unwrap();
    let db_path = root.join(".ctx-codegraph/codegraph.sqlite");
    assert!(db_path.exists());
    let is_valid = validate_index_db_with_registry(root, &options, &registry).unwrap();
    assert!(is_valid);

    // 2. Second call to load_or_build reuses the index
    let _conn2 = ensure_index_with_registry(root, options.clone(), &registry).unwrap();
    assert!(db_path.exists());

    // 3. Cache path is stable for a subdirectory inside the same repo root
    let sub_dir = root.join("src");
    fs::create_dir_all(&sub_dir).unwrap();
    let resolved_root = find_workspace_root(&sub_dir, &registry);
    assert_eq!(
        resolved_root.canonicalize().unwrap(),
        root.canonicalize().unwrap()
    );

    // 4. Modifying a file invalidates the index
    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&file_path, "fn test_modified_long_body() { let x = 1; }").unwrap();
    let is_valid_after_mod = validate_index_db_with_registry(root, &options, &registry).unwrap();
    assert!(!is_valid_after_mod);

    // 5. Changing options invalidates cache
    let _conn3 = ensure_index_with_registry(root, options.clone(), &registry).unwrap();
    assert!(validate_index_db_with_registry(root, &options, &registry).unwrap());

    let different_options = BuildIndexOptions {
        use_lsp: true,
        max_depth: None,
        include_tests: true,
        change_detection: FileChangeDetection::MtimeAndSize,
        ..Default::default()
    };
    let is_valid_diff_opts = validate_index_db_with_registry(root, &different_options, &registry).unwrap();
    assert!(!is_valid_diff_opts);

    // 6. Changing schema version invalidates cache
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', '99')",
            [],
        )
        .unwrap();
    }
    let is_valid_diff_schema = validate_index_db_with_registry(root, &options, &registry).unwrap();
    assert!(!is_valid_diff_schema);
}
#[test]
fn test_ensure_index_unified_behavior() {
    let registry = production_registry();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let git_dir = root.join(".git");
    fs::create_dir_all(&git_dir).unwrap();

    let file_path = root.join("lib.rs");
    fs::write(&file_path, "fn foo() {}").unwrap();

    let options = BuildIndexOptions::default();

    // 1. Missing -> ensure builds
    let conn1 = ensure_index_with_registry(root, options.clone(), &registry).unwrap();
    let db_path = root.join(".ctx-codegraph/codegraph.sqlite");
    assert!(db_path.exists());
    // conn is usable
    let syms = find_symbols(&conn1, "foo").unwrap();
    assert!(!syms.is_empty());

    // 2. Ready -> ensure short-circuits (no rebuild side effects)
    let state1 = get_index_state_with_registry(root, &options, &registry).unwrap();
    assert!(matches!(state1, IndexState::Ready));
    let _conn2 = ensure_index_with_registry(root, options.clone(), &registry).unwrap();
    let state2 = get_index_state_with_registry(root, &options, &registry).unwrap();
    assert!(matches!(state2, IndexState::Ready));

    // 3. Change -> next ensure triggers update to Ready
    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&file_path, "fn foo() { let _ = 42; }").unwrap();
    let state_dirty = get_index_state_with_registry(root, &options, &registry).unwrap();
    assert!(matches!(state_dirty, IndexState::NeedsIncrementalUpdate(_)));
    let _conn3 = ensure_index_with_registry(root, options.clone(), &registry).unwrap();
    let state_clean = get_index_state_with_registry(root, &options, &registry).unwrap();
    assert!(matches!(state_clean, IndexState::Ready));
}
#[test]
fn test_edge_resolution_quality_variants() {
    let registry = production_registry();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Create Cargo.toml
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"test_proj\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();

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

    let (index, report) = rebuild_index_db_with_registry(
        root,
        BuildIndexOptions::default(),
        &registry,
    )
    .unwrap();

    assert!(report.full_rebuild);

    let sym_a = index.symbols.iter().find(|s| s.name == "a").unwrap();
    let sym_d = index.symbols.iter().find(|s| s.name == "d").unwrap();

    // Verify b(); inside a() is Syntax (same file)
    let edge_a_b = index
        .edges
        .iter()
        .find(|e| e.raw_text.as_deref() == Some("b") && e.from_symbol_id == Some(sym_a.id.unwrap()))
        .unwrap();
    assert_eq!(edge_a_b.confidence, ResolutionConfidence::Syntax);

    // Verify unresolved_call(); inside a() is Unresolved
    let edge_unres = index
        .edges
        .iter()
        .find(|e| {
            e.raw_text.as_deref() == Some("unresolved_call")
                && e.from_symbol_id == Some(sym_a.id.unwrap())
        })
        .unwrap();
    assert_eq!(edge_unres.confidence, ResolutionConfidence::Unresolved);
    assert!(edge_unres.to_symbol_id.is_none());

    // Verify b(); inside d() is Heuristic (different file)
    let edge_d_b = index
        .edges
        .iter()
        .find(|e| e.raw_text.as_deref() == Some("b") && e.from_symbol_id == Some(sym_d.id.unwrap()))
        .unwrap();
    assert_eq!(edge_d_b.confidence, ResolutionConfidence::Heuristic);
}
#[test]
fn test_incremental_diff_report() {
    let registry = production_registry();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"test_proj\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_lib = src_dir.join("lib.rs");
    let file_a = src_dir.join("a.rs");
    let file_b = src_dir.join("b.rs");

    fs::write(&file_lib, "pub fn run() {}").unwrap();
    fs::write(&file_a, "pub fn a() {}").unwrap();
    fs::write(&file_b, "pub fn b() {}").unwrap();

    let options = BuildIndexOptions::default();

    // 1. Initial build: all files added
    let (_, report1) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert!(report1.full_rebuild);
    assert_eq!(report1.added_files, 3);
    assert_eq!(report1.modified_files, 0);
    assert_eq!(report1.deleted_files, 0);
    assert_eq!(report1.unchanged_files, 0);

    // 2. Second build: no changes, all files unchanged
    let (_, report2) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert!(!report2.full_rebuild);
    assert_eq!(report2.added_files, 0);
    assert_eq!(report2.modified_files, 0);
    assert_eq!(report2.deleted_files, 0);
    assert_eq!(report2.unchanged_files, 3);

    // 3. Modify a.rs: only a.rs is modified
    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&file_a, "pub fn a() { // modified\n }").unwrap();
    let (_, report3) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert!(!report3.full_rebuild);
    assert_eq!(report3.added_files, 0);
    assert_eq!(report3.modified_files, 1);
    assert_eq!(report3.deleted_files, 0);
    assert_eq!(report3.unchanged_files, 2);

    // 4. Add c.rs: only c.rs is added
    let file_c = src_dir.join("c.rs");
    fs::write(&file_c, "pub fn c() {}").unwrap();
    let (_, report4) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert!(!report4.full_rebuild);
    assert_eq!(report4.added_files, 1);
    assert_eq!(report4.modified_files, 0);
    assert_eq!(report4.deleted_files, 0);
    assert_eq!(report4.unchanged_files, 3);

    // 5. Delete b.rs: only b.rs is deleted
    fs::remove_file(&file_b).unwrap();
    let (_, report5) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert!(!report5.full_rebuild);
    assert_eq!(report5.added_files, 0);
    assert_eq!(report5.modified_files, 0);
    assert_eq!(report5.deleted_files, 1);
    assert_eq!(report5.unchanged_files, 3);
}
#[test]
fn test_db_correctness_after_incremental_update() {
    let registry = production_registry();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"test_proj\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_lib = src_dir.join("lib.rs");
    let file_b = src_dir.join("b.rs");

    // Scenario 1: Modify file
    // Initial: a calls b
    fs::write(&file_lib, "pub fn a() { b(); }").unwrap();
    fs::write(&file_b, "pub fn b() {}").unwrap();

    let options = BuildIndexOptions::default();

    let (index, _) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert!(index.symbols.iter().any(|s| s.name == "b"));
    assert!(
        index
            .edges
            .iter()
            .any(|e| e.raw_text.as_deref() == Some("b") && e.to_symbol_id.is_some())
    );

    // Modify file: change lib.rs so a calls c instead, and define c in lib.rs
    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&file_lib, "pub fn a() { c(); }\npub fn c() {}").unwrap();

    let (index_mod, _) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert!(index_mod.symbols.iter().any(|s| s.name == "c"));
    assert!(
        !index_mod
            .edges
            .iter()
            .any(|e| e.raw_text.as_deref() == Some("b"))
    ); // old edge b disappeared
    assert!(
        index_mod
            .edges
            .iter()
            .any(|e| e.raw_text.as_deref() == Some("c"))
    ); // new edge c appeared

    // Scenario 2: Add file
    // Add file d.rs: d calls a
    let file_d = src_dir.join("d.rs");
    fs::write(&file_d, "pub fn d() { a(); }").unwrap();

    let (index_add, _) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert!(index_add.symbols.iter().any(|s| s.name == "d"));
    assert!(
        index_add
            .edges
            .iter()
            .any(|e| e.raw_text.as_deref() == Some("a") && e.to_symbol_id.is_some())
    );

    // Scenario 3: Delete file
    // Delete b.rs
    fs::remove_file(&file_b).unwrap();

    let (index_del, _) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert!(!index_del.symbols.iter().any(|s| s.name == "b")); // symbol b is gone

    // Scenario 4: Rename/change symbol
    // Change lib.rs from defining c to defining new_name, and calling it
    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(
        &file_lib,
        "pub fn a() { new_name(); }\npub fn new_name() {}",
    )
    .unwrap();

    let (index_rename, _) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert!(!index_rename.symbols.iter().any(|s| s.name == "c"));
    assert!(index_rename.symbols.iter().any(|s| s.name == "new_name"));
    assert!(
        !index_rename
            .edges
            .iter()
            .any(|e| e.raw_text.as_deref() == Some("c"))
    );
    assert!(
        index_rename
            .edges
            .iter()
            .any(|e| e.raw_text.as_deref() == Some("new_name"))
    );
}
#[test]
fn test_parse_failure_preserves_old_graph() {
    let registry = production_registry();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"test_proj\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_path = src_dir.join("lib.rs");
    fs::write(&file_path, "pub fn a() {}").unwrap();

    let options = BuildIndexOptions::default();

    // 1. Initial build: success
    let (index, report) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    assert_eq!(report.parsed_files, 1);
    assert!(index.symbols.iter().any(|s| s.name == "a"));

    // 2. Modify file to have invalid Rust syntax (e.g. "fn a( {")
    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(&file_path, "fn a( {").unwrap();

    // Run incremental update (through rebuild_index_db)
    let (index2, report2) = rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();
    // Parse should have failed, so the old symbol graph is preserved
    assert_eq!(report2.parsed_files, 0);
    assert!(index2.symbols.iter().any(|s| s.name == "a"));

    // Check IndexState in the DB. Since lib.rs has parse_status = 'Failed', the index state should be NeedsIncrementalUpdate, not Ready!
    let state = get_index_state_with_registry(root, &options, &registry).unwrap();
    match state {
        IndexState::NeedsIncrementalUpdate(_) => {}
        other => panic!("Expected NeedsIncrementalUpdate, got {:?}", other),
    }
}
#[test]
fn test_content_hash_detection() {
    let registry = production_registry();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"test_proj\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_path = src_dir.join("lib.rs");
    fs::write(&file_path, "pub fn a() {}").unwrap();

    let options = BuildIndexOptions {
        use_lsp: false,
        max_depth: None,
        include_tests: true,
        change_detection: FileChangeDetection::MtimeAndSize,
        ..Default::default()
    };

    // Build the initial index
    rebuild_index_db_with_registry(root, options.clone(), &registry).unwrap();

    // 1. Change detection strategy changes
    let options_diff_strat = BuildIndexOptions {
        change_detection: FileChangeDetection::ContentHash,
        ..options.clone()
    };
    let state1 = get_index_state_with_registry(root, &options_diff_strat, &registry).unwrap();
    match state1 {
        IndexState::NeedsFullRebuild(RebuildReason::ChangeDetectionStrategyChanged) => {}
        other => panic!("Expected ChangeDetectionStrategyChanged, got {:?}", other),
    }

    // 2. Parser config hash changes (e.g. include_tests changes)
    let options_diff_parser = BuildIndexOptions {
        include_tests: false,
        ..options.clone()
    };
    let state2 = get_index_state_with_registry(root, &options_diff_parser, &registry).unwrap();
    match state2 {
        IndexState::NeedsFullRebuild(RebuildReason::ParserConfigChanged) => {}
        other => panic!("Expected ParserConfigChanged, got {:?}", other),
    }

    // 3. Resolver config changes (e.g. use_lsp changes)
    let options_diff_resolver = BuildIndexOptions {
        use_lsp: true,
        ..options.clone()
    };
    let state3 = get_index_state_with_registry(root, &options_diff_resolver, &registry).unwrap();
    match state3 {
        IndexState::NeedsFullRebuild(RebuildReason::ResolverConfigChanged) => {}
        other => panic!("Expected ResolverConfigChanged, got {:?}", other),
    }
}
