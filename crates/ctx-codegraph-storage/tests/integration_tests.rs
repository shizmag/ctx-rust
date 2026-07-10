use ctx_codegraph_storage::index::BuildIndexOptions;
use ctx_codegraph_storage::model::FileChangeDetection;
use ctx_codegraph_storage::storage::{load_index, open_db, rebuild_index_db};
use std::fs;
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
        BuildIndexOptions::default(),
    )
    .unwrap();

    let symbol_names: std::collections::HashSet<String> =
        index.symbols.iter().map(|s| s.name.clone()).collect();
    assert!(symbol_names.contains("run_pipeline"));
    assert!(symbol_names.contains("load"));
    assert!(symbol_names.contains("process"));
    assert!(symbol_names.contains("save"));

    let conn = open_db(dir.path()).unwrap();
    let loaded_index = load_index(&conn, dir.path()).unwrap();

    let loaded_names: std::collections::HashSet<String> = loaded_index
        .symbols
        .iter()
        .map(|s| s.name.clone())
        .collect();
    assert!(loaded_names.contains("run_pipeline"));
    assert!(loaded_names.contains("load"));
    assert!(loaded_names.contains("process"));
    assert!(loaded_names.contains("save"));
}

#[test]
fn test_other_languages_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"test_proj\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("lib.rs"), "pub fn a() {}").unwrap();

    let options = BuildIndexOptions::default();

    // 1. Build initial Rust index
    rebuild_index_db(root, options.clone()).unwrap();

    // 2. Insert a fake Python file into the database
    let db_path = root.join(".ctx-codegraph").join("codegraph.sqlite");
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO files (
                path, rel_path, language, backend_id, mtime_ms, size_bytes,
                content_hash, parser_id, parser_version, parser_config_hash,
                indexed_at_ms, parse_status
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                "/abs/path/main.py",
                "main.py",
                "python",
                "jedi",
                1234,
                567,
                "pyhash",
                "jedi-parser",
                "0.1.0",
                "pyconfig",
                1000,
                "Success",
            ],
        )
        .unwrap();
    }

    // 3. Trigger a full rebuild of the Rust index (e.g. by changing parser config/options)
    let options_diff = BuildIndexOptions {
        include_tests: false,
        ..options.clone()
    };
    rebuild_index_db(root, options_diff).unwrap();

    // 4. Verify that the Python file is still in the database!
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let py_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM files WHERE language = 'python')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(py_exists, "Python file was deleted during full rebuild!");
    }
}
