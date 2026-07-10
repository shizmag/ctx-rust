use ctx_codegraph_storage::index::{BuildIndexOptions, build_index, create_file_snapshot};
use ctx_codegraph_storage::model::{FileChangeDetection, IndexState};
use ctx_codegraph_storage::storage::{
    find_workspace_root, get_index_state, read_metadata, rebuild_index_db, write_metadata,
};
use std::fs;
use std::path::Path;

#[test]
fn create_file_snapshot_captures_rust_file_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_path = src_dir.join("lib.rs");
    fs::write(&file_path, "pub fn alpha() {}\n").unwrap();

    let snapshot = create_file_snapshot(
        root,
        &file_path,
        FileChangeDetection::MtimeAndSize,
        true,
    );

    assert_eq!(snapshot.rel_path, Path::new("src/lib.rs"));
    assert_eq!(snapshot.abs_path, file_path);
    assert_eq!(snapshot.language.as_str(), "rust");
    assert_eq!(snapshot.backend_id.0, "rust-backend");
    assert!(snapshot.size_bytes > 0);
    assert!(snapshot.mtime_ms > 0);
    assert!(snapshot.content_hash.is_none());
    assert_eq!(snapshot.parse_status, ctx_codegraph_lang::model::FileParseStatus::Success);
}

#[test]
fn create_file_snapshot_includes_content_hash_when_requested() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file_path = root.join("main.rs");
    fs::write(&file_path, "fn main() {}").unwrap();

    let snapshot = create_file_snapshot(
        root,
        &file_path,
        FileChangeDetection::ContentHash,
        false,
    );

    assert!(snapshot.content_hash.is_some());
    assert!(!snapshot.content_hash.as_ref().unwrap().is_empty());
}

#[test]
fn build_index_indexes_rust_sources() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("lib.rs"), "pub fn beta() {}\n").unwrap();

    let index = build_index(root, BuildIndexOptions::default()).unwrap();

    assert_eq!(index.files.len(), 1);
    assert_eq!(index.files[0].rel_path, Path::new("src/lib.rs"));
    let names: std::collections::HashSet<_> = index.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains("beta"));
}

#[test]
fn storage_wrappers_find_workspace_root_and_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"wrapper_test\"\nversion = \"0.1.0\"\nedition = \"2021\"",
    )
    .unwrap();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("lib.rs"), "pub fn gamma() {}").unwrap();

    let nested = src_dir.join("nested");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(
        find_workspace_root(&nested).canonicalize().unwrap(),
        root.canonicalize().unwrap()
    );

    let options = BuildIndexOptions::default();
    let state_before = get_index_state(root, &options).unwrap();
    assert!(matches!(
        state_before,
        IndexState::Missing | IndexState::NeedsFullRebuild(_)
    ));

    rebuild_index_db(root, options.clone()).unwrap();

    write_metadata(root, "test_key", "test_value").unwrap();
    assert_eq!(read_metadata(root, "test_key").as_deref(), Some("test_value"));
    assert!(read_metadata(root, "missing_key").is_none());

    let state_after = get_index_state(root, &options).unwrap();
    assert!(matches!(state_after, IndexState::Ready));
}