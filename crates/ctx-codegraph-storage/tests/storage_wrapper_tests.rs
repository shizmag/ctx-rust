use ctx_codegraph_chunk::ChunkId;
use ctx_codegraph_lang::model::{
    EdgeDirection, EdgeKind, IndexState, RebuildReason, ResolutionConfidence, SymbolId,
    SymbolResolution,
};
use ctx_codegraph_storage::index::BuildIndexOptions;
use ctx_codegraph_storage::storage::{
    check_db_compatibility, clear_index, compute_affected_set, compute_index_diff, ensure_index,
    find_symbols, find_workspace_root, get_index_state, init_schema, load_callees, load_callers,
    load_chunk, load_edges_for_symbol, load_index, load_occurrence, load_symbol, open_codegraph_db,
    open_db, read_metadata, rebuild_index_db, resolve_symbol, run_full_rebuild,
    run_incremental_update, validate_index_db, validate_index_invariants,
    write_metadata,
};
use ctx_codegraph_store::storage::build_search_indexes;
use std::fs;
use std::path::{Path, PathBuf};

fn no_search_options() -> BuildIndexOptions {
    BuildIndexOptions {
        with_lexical: Some(false),
        with_embeddings: Some(false),
        ..BuildIndexOptions::default()
    }
}

/// Match indexer path normalization (macOS `/var` vs `/private/var`).
fn workspace_root(dir: &tempfile::TempDir) -> PathBuf {
    find_workspace_root(dir.path())
}

fn setup_call_graph_project(root: &Path) {
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"wrapper_storage\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"pub fn run_pipeline() {
    helper();
}

pub fn helper() {}
"#,
    )
    .unwrap();
}

fn setup_multi_file_project(root: &Path) {
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"wrapper_diff\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), "pub fn run() {}").unwrap();
    fs::write(src.join("a.rs"), "pub fn a() {}").unwrap();
    fs::write(src.join("b.rs"), "pub fn b() {}").unwrap();
}

#[test]
fn init_schema_creates_core_tables() {
    let dir = tempfile::tempdir().unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("fresh.sqlite")).unwrap();
    init_schema(&conn).unwrap();

    let tables: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    assert!(tables.contains(&"files".to_string()));
    assert!(tables.contains(&"symbols".to_string()));
    assert!(tables.contains(&"metadata".to_string()));
    assert!(tables.contains(&"edges".to_string()));
}

#[test]
fn open_db_and_open_codegraph_db_share_workspace_database() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_call_graph_project(root);

    rebuild_index_db(root, BuildIndexOptions::default()).unwrap();

    let conn = open_db(root).unwrap();
    let cg_conn = open_codegraph_db(root).unwrap();

    let count_db: i64 = conn
        .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
        .unwrap();
    let count_cg: i64 = cg_conn
        .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count_db, count_cg);
    assert!(count_db >= 2);

    assert!(root.join(".ctx-codegraph/codegraph.sqlite").exists());
}

#[test]
fn find_workspace_root_from_nested_source_dir() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_call_graph_project(root);

    let nested = root.join("src").join("deep");
    fs::create_dir_all(&nested).unwrap();

    assert_eq!(
        find_workspace_root(&nested).canonicalize().unwrap(),
        root.canonicalize().unwrap()
    );
}

#[test]
fn metadata_round_trip_and_missing_key() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_call_graph_project(root);
    rebuild_index_db(root, BuildIndexOptions::default()).unwrap();

    write_metadata(root, "custom_key", "custom_value").unwrap();
    assert_eq!(
        read_metadata(root, "custom_key").as_deref(),
        Some("custom_value")
    );
    assert!(read_metadata(root, "absent_key").is_none());
}

#[test]
fn check_db_compatibility_reports_missing_on_empty_schema() {
    let dir = tempfile::tempdir().unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("empty.sqlite")).unwrap();
    init_schema(&conn).unwrap();

    let options = BuildIndexOptions::default();
    let reason = check_db_compatibility(&conn, &options).unwrap();
    assert!(
        reason.is_some(),
        "fresh schema without rebuild metadata should require rebuild, got {reason:?}"
    );
}

#[test]
fn check_db_compatibility_ok_after_full_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_call_graph_project(root);

    let mut conn = open_db(root).unwrap();
    init_schema(&conn).unwrap();
    run_full_rebuild(&mut conn, root, BuildIndexOptions::default(), None).unwrap();

    let reason = check_db_compatibility(&conn, &BuildIndexOptions::default()).unwrap();
    assert!(reason.is_none());
}

#[test]
fn compute_index_diff_tracks_file_changes() {
    let dir = tempfile::tempdir().unwrap();
    setup_multi_file_project(dir.path());
    let root = workspace_root(&dir);

    let options = no_search_options();
    rebuild_index_db(&root, options.clone()).unwrap();

    let conn = open_db(&root).unwrap();
    let diff_unchanged = compute_index_diff(&conn, &root, &options).unwrap();
    assert_eq!(diff_unchanged.added.len(), 0);
    assert_eq!(diff_unchanged.modified.len(), 0);
    assert_eq!(diff_unchanged.deleted.len(), 0);
    assert_eq!(diff_unchanged.unchanged.len(), 3);

    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(root.join("src/a.rs"), "pub fn a_modified() {}").unwrap();

    let diff_modified = compute_index_diff(&conn, &root, &options).unwrap();
    assert_eq!(diff_modified.modified.len(), 1);
    assert_eq!(diff_modified.unchanged.len(), 2);
}

#[test]
fn get_index_state_and_validate_index_db_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_call_graph_project(root);

    let options = BuildIndexOptions::default();
    assert!(matches!(
        get_index_state(root, &options).unwrap(),
        IndexState::Missing | IndexState::NeedsFullRebuild(_)
    ));
    assert!(!validate_index_db(root, &options).unwrap());

    rebuild_index_db(root, options.clone()).unwrap();

    assert!(matches!(get_index_state(root, &options).unwrap(), IndexState::Ready));
    assert!(validate_index_db(root, &options).unwrap());

    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(
        root.join("src/lib.rs"),
        "pub fn run_pipeline() { helper(); extra(); }\npub fn helper() {}\npub fn extra() {}",
    )
    .unwrap();

    assert!(matches!(
        get_index_state(root, &options).unwrap(),
        IndexState::NeedsIncrementalUpdate(_)
    ));
    assert!(!validate_index_db(root, &options).unwrap());
}

#[test]
fn clear_index_removes_symbol_graph() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_call_graph_project(root);

    let mut conn = open_db(root).unwrap();
    init_schema(&mut conn).unwrap();
    run_full_rebuild(&mut conn, root, no_search_options(), None).unwrap();

    let before: i64 = conn
        .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
        .unwrap();
    assert!(before > 0);

    clear_index(&mut conn).unwrap();

    let after: i64 = conn
        .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
        .unwrap();
    assert_eq!(after, 0);
}

#[test]
fn compute_affected_set_includes_deleted_file_symbols() {
    let dir = tempfile::tempdir().unwrap();
    setup_multi_file_project(dir.path());
    let root = workspace_root(&dir);

    let options = no_search_options();
    rebuild_index_db(&root, options.clone()).unwrap();

    let file_b = root.join("src/b.rs");
    fs::remove_file(&file_b).unwrap();

    let conn = open_db(&root).unwrap();
    let diff = compute_index_diff(&conn, &root, &options).unwrap();
    assert_eq!(diff.deleted.len(), 1);

    let affected = compute_affected_set(&conn, &diff, &[]).unwrap();
    assert!(!affected.files.is_empty());
}

#[test]
fn ensure_index_builds_and_reuses_ready_index() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_call_graph_project(root);

    let options = BuildIndexOptions::default();
    let conn1 = ensure_index(root, options.clone()).unwrap();
    let symbols = find_symbols(&conn1, "run_pipeline").unwrap();
    assert_eq!(symbols.len(), 1);

    let conn2 = ensure_index(root, options.clone()).unwrap();
    let symbols2 = find_symbols(&conn2, "run_pipeline").unwrap();
    assert_eq!(symbols2.len(), 1);
    assert!(matches!(get_index_state(root, &options).unwrap(), IndexState::Ready));
}

#[test]
fn run_full_rebuild_and_incremental_update_via_wrappers() {
    let dir = tempfile::tempdir().unwrap();
    setup_call_graph_project(dir.path());
    let root = workspace_root(&dir);

    let options = no_search_options();
    let mut conn = open_db(&root).unwrap();
    init_schema(&mut conn).unwrap();

    let (index, report) = run_full_rebuild(
        &mut conn,
        &root,
        options.clone(),
        Some(RebuildReason::MissingDatabase),
    )
    .unwrap();
    assert!(report.full_rebuild);
    assert!(index.symbols.iter().any(|s| s.name == "run_pipeline"));

    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(
        root.join("src/lib.rs"),
        "pub fn run_pipeline() { helper(); }\npub fn helper() { let x = 1; }",
    )
    .unwrap();

    let diff = compute_index_diff(&conn, &root, &options).unwrap();
    assert_eq!(diff.modified.len(), 1);

    let (index2, report2) =
        run_incremental_update(&mut conn, &root, options.clone(), diff).unwrap();
    assert!(!report2.full_rebuild);
    assert_eq!(report2.modified_files, 1);
    assert!(index2.symbols.iter().any(|s| s.name == "helper"));
}

#[test]
fn load_query_wrappers_resolve_call_graph() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_call_graph_project(root);

    let (index, _) = rebuild_index_db(root, BuildIndexOptions::default()).unwrap();
    let conn = open_db(root).unwrap();
    validate_index_invariants(&conn).unwrap();

    let run_pipeline = index
        .symbols
        .iter()
        .find(|s| s.name == "run_pipeline")
        .unwrap();
    let helper = index.symbols.iter().find(|s| s.name == "helper").unwrap();
    let run_id = run_pipeline.id.unwrap();
    let helper_id = helper.id.unwrap();

    let found = find_symbols(&conn, "run_pipeline").unwrap();
    assert_eq!(found.len(), 1);

    let resolved = resolve_symbol(&conn, "run_pipeline").unwrap();
    match resolved {
        SymbolResolution::Unique(obj) => assert_eq!(obj.name, "run_pipeline"),
        other => panic!("expected unique resolution, got {other:?}"),
    }

    let loaded = load_symbol(&conn, run_id).unwrap();
    assert_eq!(loaded.name, "run_pipeline");

    let callees = load_callees(&conn, run_id).unwrap();
    assert!(
        callees
            .iter()
            .any(|(edge, _)| edge.to_symbol_id == Some(SymbolId(helper_id.0)))
    );

    let callers = load_callers(&conn, helper_id).unwrap();
    assert!(
        callers
            .iter()
            .any(|(edge, _)| edge.from_symbol_id == Some(SymbolId(run_id.0)))
    );

    let edges =
        load_edges_for_symbol(&conn, run_id, EdgeDirection::Outbound, &[EdgeKind::Call]).unwrap();
    assert!(!edges.is_empty());
    assert_eq!(edges[0].0.confidence, ResolutionConfidence::Syntax);

    if let Some(edge) = index.edges.first() {
        if let Some(occ_id) = edge.occurrence_id {
            let occ = load_occurrence(&conn, occ_id).unwrap();
            assert!(!occ.raw_text.is_empty());
        }
    }

    let loaded_index = load_index(&conn, root).unwrap();
    assert_eq!(loaded_index.symbols.len(), index.symbols.len());
}

#[test]
fn load_chunk_after_search_index_build() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_call_graph_project(root);

    let base_options = no_search_options();
    rebuild_index_db(root, base_options).unwrap();

    let options = BuildIndexOptions {
        with_lexical: Some(true),
        with_embeddings: Some(false),
        ..BuildIndexOptions::default()
    };
    let conn = open_db(root).unwrap();
    let report =
        build_search_indexes(&conn, root, &options, &ctx_config::Config::default()).unwrap();
    assert!(report.chunks_written > 0);

    let chunk_id = ChunkId(0);
    let chunk = load_chunk(&conn, chunk_id).unwrap().expect("chunk 0 exists");
    assert!(!chunk.qualified_name.is_empty());

    let missing = load_chunk(&conn, ChunkId(999_999)).unwrap();
    assert!(missing.is_none());
}

#[test]
fn save_and_reload_index_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_call_graph_project(root);

    let mut conn = open_db(root).unwrap();
    init_schema(&mut conn).unwrap();

    let (index, _) =
        run_full_rebuild(&mut conn, root, no_search_options(), None).unwrap();

    let reloaded = load_index(&conn, root).unwrap();
    assert_eq!(reloaded.symbols.len(), index.symbols.len());
    assert_eq!(reloaded.files.len(), index.files.len());
}