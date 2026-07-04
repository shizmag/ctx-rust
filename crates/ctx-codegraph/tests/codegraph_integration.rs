use ctx_codegraph::{
    BuildIndexOptions, SliceOptions, SymbolKind, build_index, find_symbols, forward_slice,
    load_index, open_db, rebuild_index_db,
};
use std::fs;

#[test]
fn test_integration_builds_simple_project_index_and_slice() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    // Create Cargo.toml
    let cargo_content = r#"
        [package]
        name = "temp_project"
        version = "0.1.0"
        edition = "2024"
    "#;
    fs::write(root.join("Cargo.toml"), cargo_content).unwrap();

    // Create src directory and src/lib.rs
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let lib_code = r#"
        pub fn run_pipeline() {
            let value = load();
            process(value);
        }

        fn load() -> i32 {
            1
        }

        fn process(value: i32) {
            save(value);
        }

        fn save(_: i32) {}

        fn unrelated() {}
    "#;
    fs::write(src_dir.join("lib.rs"), lib_code).unwrap();

    // 6.1 Builds simple project index
    let index = build_index(
        root,
        BuildIndexOptions {
            use_rust_analyzer: false,
            max_depth: None,
            include_tests: true,
        },
    )
    .unwrap();

    let run_pipeline = index
        .symbols
        .iter()
        .find(|s| s.name == "run_pipeline")
        .unwrap();
    let load = index.symbols.iter().find(|s| s.name == "load").unwrap();
    let process = index.symbols.iter().find(|s| s.name == "process").unwrap();
    let save = index.symbols.iter().find(|s| s.name == "save").unwrap();

    assert_eq!(run_pipeline.kind, SymbolKind::Function);
    assert_eq!(load.kind, SymbolKind::Function);
    assert_eq!(process.kind, SymbolKind::Function);
    assert_eq!(save.kind, SymbolKind::Function);

    // Assert edges
    let e_run_load = index
        .edges
        .iter()
        .find(|e| e.from == run_pipeline.id.unwrap() && e.to == Some(load.id.unwrap()));
    let e_run_proc = index
        .edges
        .iter()
        .find(|e| e.from == run_pipeline.id.unwrap() && e.to == Some(process.id.unwrap()));
    let e_proc_save = index
        .edges
        .iter()
        .find(|e| e.from == process.id.unwrap() && e.to == Some(save.id.unwrap()));

    assert!(e_run_load.is_some());
    assert!(e_run_proc.is_some());
    assert!(e_proc_save.is_some());

    // 6.2 Forward slice from entrypoint
    let f_slice = forward_slice(
        &index,
        run_pipeline.id.unwrap(),
        SliceOptions {
            max_depth: 10,
            include_tests: true,
        },
    );
    assert!(f_slice.contains(&load.id.unwrap()));
    assert!(f_slice.contains(&process.id.unwrap()));
    assert!(f_slice.contains(&save.id.unwrap()));

    let unrelated = index
        .symbols
        .iter()
        .find(|s| s.name == "unrelated")
        .unwrap();
    assert!(!f_slice.contains(&unrelated.id.unwrap()));
}

#[test]
fn test_integration_rebuild_sqlite_database() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    let cargo_content = r#"
        [package]
        name = "temp_project"
        version = "0.1.0"
        edition = "2024"
    "#;
    fs::write(root.join("Cargo.toml"), cargo_content).unwrap();

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let lib_code = r#"
        pub fn run_pipeline() {
            load();
        }
        fn load() {}
    "#;
    fs::write(src_dir.join("lib.rs"), lib_code).unwrap();

    // 6.3 Rebuild SQLite database
    let _index = rebuild_index_db(
        root,
        BuildIndexOptions {
            use_rust_analyzer: false,
            max_depth: None,
            include_tests: true,
        },
    )
    .unwrap();

    let db_path = root.join(".ctx-codegraph/codegraph.sqlite");
    assert!(db_path.exists());

    let conn = open_db(root).unwrap();
    let loaded = load_index(&conn, root).unwrap();

    let rp_loaded = loaded
        .symbols
        .iter()
        .find(|s| s.name == "run_pipeline")
        .unwrap();
    assert_eq!(rp_loaded.qualified_name, "lib::run_pipeline");

    let found = find_symbols(&conn, "run_pipeline").unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "run_pipeline");
}

#[test]
fn test_integration_ignores_target_and_ctx_codegraph() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    let cargo_content = r#"
        [package]
        name = "temp_project"
        version = "0.1.0"
        edition = "2024"
    "#;
    fs::write(root.join("Cargo.toml"), cargo_content).unwrap();

    // Create src/lib.rs
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("lib.rs"), "pub fn valid_function() {}").unwrap();

    // Create target/debug/generated.rs
    let target_dir = root.join("target/debug");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(
        target_dir.join("generated.rs"),
        "pub fn target_function() {}",
    )
    .unwrap();

    // Create .ctx-codegraph/generated.rs
    let cg_dir = root.join(".ctx-codegraph");
    fs::create_dir_all(&cg_dir).unwrap();
    fs::write(
        cg_dir.join("generated.rs"),
        "pub fn codegraph_function() {}",
    )
    .unwrap();

    // Rebuild index
    let index = rebuild_index_db(
        root,
        BuildIndexOptions {
            use_rust_analyzer: false,
            max_depth: None,
            include_tests: true,
        },
    )
    .unwrap();

    let valid_exists = index.symbols.iter().any(|s| s.name == "valid_function");
    let target_exists = index.symbols.iter().any(|s| s.name == "target_function");
    let cg_exists = index.symbols.iter().any(|s| s.name == "codegraph_function");

    assert!(valid_exists);
    assert!(!target_exists);
    assert!(!cg_exists);
}
