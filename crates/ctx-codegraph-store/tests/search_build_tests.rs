use ctx_codegraph_lang::index::BuildIndexOptions;
use ctx_codegraph_lang::model::IndexState;
use ctx_codegraph_models::{batch_ranges, DEFAULT_EMBED_BATCH_SIZE};
use ctx_codegraph_store::storage::{
    build_search_indexes, dense_embedding_count, get_index_state_with_registry, load_chunk,
    load_chunks_for_symbol, read_metadata, rebuild_index_db_with_registry,
};
use ctx_config::{Config, DEFAULT_BUILD_BATCH_SIZE};
use std::fs;
use std::path::Path;

mod common;
use common::{
    indexed_db, lexical_index_dir, lexical_search_options, no_search_options, production_registry,
    setup_mini_rust_project,
};

/// Many top-level functions across several source files (for file-batch and chunk-batch tests).
fn setup_many_functions_project(root: &Path) {
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"batch_embed_test\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();

    let file_count = DEFAULT_EMBED_BATCH_SIZE + 5;
    let mut lib_rs = String::from("pub mod generated;\n");
    for file_idx in 0..4 {
        lib_rs.push_str(&format!("pub mod part_{file_idx};\n"));
    }
    for idx in 0..file_count {
        lib_rs.push_str(&format!("pub fn fn_{idx}() {{ generated::helper_{idx}(); }}\n"));
    }
    fs::write(src.join("lib.rs"), lib_rs).unwrap();

    let mut generated_rs = String::new();
    for idx in 0..file_count {
        generated_rs.push_str(&format!(
            "pub fn helper_{idx}() {{ let _ = {idx}; }}\n"
        ));
    }
    fs::write(src.join("generated.rs"), generated_rs).unwrap();

    for file_idx in 0..4 {
        let mut part_rs = String::new();
        for idx in 0..3 {
            part_rs.push_str(&format!("pub fn part_{file_idx}_{idx}() {{}}\n"));
        }
        fs::write(src.join(format!("part_{file_idx}.rs")), part_rs).unwrap();
    }
}

#[test]
fn test_build_search_indexes_respects_custom_file_batch_size() {
    const FILE_BATCH: usize = 2;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_many_functions_project(root);

    let (conn, _index, _registry) = indexed_db(root, no_search_options());
    let file_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
        .unwrap();
    assert!(
        file_count > FILE_BATCH as i64,
        "fixture should index more files than one batch (got {file_count})"
    );

    let ranges: Vec<_> = batch_ranges(file_count as usize, FILE_BATCH).collect();
    assert!(
        ranges.len() > 1,
        "expected multiple file batches for {file_count} files at batch size {FILE_BATCH}"
    );

    let config = Config {
        build_batch_size: Some(FILE_BATCH),
        ..Default::default()
    };
    let report = build_search_indexes(
        &conn,
        root,
        &lexical_search_options(),
        &config,
    )
    .unwrap();

    assert!(report.chunks_written > 0);
    assert_eq!(report.chunks_written, report.lexical_docs_written);
    assert_eq!(config.effective_build_batch_size(), FILE_BATCH);

    let covered: usize = ranges.iter().map(|r| r.end - r.start).sum();
    assert_eq!(covered, file_count as usize);
    assert_eq!(ranges.first().map(|r| r.start), Some(0));
    assert_eq!(ranges.last().map(|r| r.end), Some(file_count as usize));
}

#[test]
fn test_build_search_indexes_many_chunks_use_multiple_embedding_batches() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_many_functions_project(root);

    let (conn, _index, _registry) = indexed_db(root, no_search_options());
    let report = build_search_indexes(
        &conn,
        root,
        &lexical_search_options(),
        &Config::default(),
    )
    .unwrap();

    assert!(
        report.chunks_written > DEFAULT_EMBED_BATCH_SIZE,
        "fixture should produce more chunks than one embedding batch (got {})",
        report.chunks_written
    );

    let ranges: Vec<_> = batch_ranges(
        report.chunks_written,
        DEFAULT_BUILD_BATCH_SIZE,
    )
    .collect();
    assert!(
        ranges.len() > 1,
        "expected multiple build batches for {} chunks at default batch size {}",
        report.chunks_written,
        DEFAULT_BUILD_BATCH_SIZE,
    );
    let covered: usize = ranges.iter().map(|r| r.end - r.start).sum();
    assert_eq!(covered, report.chunks_written);
    assert_eq!(ranges.first().map(|r| r.start), Some(0));
    assert_eq!(ranges.last().map(|r| r.end), Some(report.chunks_written));
}

#[test]
fn test_build_search_indexes_skipped_without_config_options() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_mini_rust_project(root);

    let (conn, _index, _registry) = indexed_db(root, no_search_options());

    let report = build_search_indexes(&conn, root, &no_search_options(), &Config::default())
        .unwrap();

    assert_eq!(report.chunks_written, 0);
    assert_eq!(report.embeddings_written, 0);
    assert_eq!(report.lexical_docs_written, 0);

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn test_build_search_indexes_lexical_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_mini_rust_project(root);

    let (conn, index, registry) = indexed_db(root, no_search_options());
    assert!(index.symbols.iter().any(|s| s.name == "greet"));
    assert!(index.symbols.iter().any(|s| s.name == "helper"));

    let search_options = lexical_search_options();
    let report =
        build_search_indexes(&conn, root, &search_options, &Config::default()).unwrap();

    assert!(report.chunks_written > 0);
    assert_eq!(report.chunks_written, report.lexical_docs_written);
    assert_eq!(report.embeddings_written, 0);
    assert!(lexical_index_dir(root).join("meta.json").exists());

    let lexical_version = read_metadata(root, &registry, "lexical_index_version");
    assert_eq!(lexical_version.as_deref(), Some("0.1.0"));

    let db_chunk_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
        .unwrap();
    assert_eq!(db_chunk_count as usize, report.chunks_written);
}

#[test]
fn test_build_search_indexes_force_rebuild_replaces_chunks() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_mini_rust_project(root);

    let (conn, _index, _registry) = indexed_db(root, no_search_options());

    let search_options = lexical_search_options();
    let first =
        build_search_indexes(&conn, root, &search_options, &Config::default()).unwrap();
    assert!(first.chunks_written > 0);

    let forced = BuildIndexOptions {
        force_search_rebuild: true,
        ..search_options.clone()
    };
    let second = build_search_indexes(&conn, root, &forced, &Config::default()).unwrap();

    assert_eq!(second.chunks_written, first.chunks_written);
    assert_eq!(second.lexical_docs_written, first.lexical_docs_written);

    let db_chunk_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
        .unwrap();
    assert_eq!(db_chunk_count as usize, second.chunks_written);
}

#[test]
fn test_build_search_indexes_embeddings_requires_model_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_mini_rust_project(root);

    let (conn, _index, _registry) = indexed_db(root, no_search_options());

    let search_options = BuildIndexOptions {
        with_lexical: Some(false),
        with_embeddings: Some(true),
        ..Default::default()
    };

    let config = Config {
        embedding_model: Some("/nonexistent/ctx-test/missing.onnx".into()),
        ..Default::default()
    };
    let err = build_search_indexes(&conn, root, &search_options, &config)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("model file not found") || err.contains("embedding model"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_build_search_indexes_chunks_loadable_after_build() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_mini_rust_project(root);

    let (conn, index, _registry) = indexed_db(root, no_search_options());

    let report = build_search_indexes(
        &conn,
        root,
        &lexical_search_options(),
        &Config::default(),
    )
    .unwrap();
    assert!(report.chunks_written > 0);

    let greet = index.symbols.iter().find(|s| s.name == "greet").unwrap();
    let symbol_chunks = load_chunks_for_symbol(&conn, greet.id.unwrap()).unwrap();
    assert!(!symbol_chunks.is_empty());
    assert!(symbol_chunks.iter().all(|c| c.text.is_none()));

    let chunk_id = symbol_chunks[0].id.unwrap();
    let loaded = load_chunk(&conn, chunk_id).unwrap().expect("chunk exists");
    assert_eq!(loaded.qualified_name, greet.qualified_name);
    assert_eq!(loaded.file_id, greet.file_id.unwrap());

    let missing = load_chunk(&conn, ctx_codegraph_chunk::ChunkId(999_999)).unwrap();
    assert!(missing.is_none());
}

#[test]
fn test_build_search_indexes_after_source_change() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_mini_rust_project(root);

    let (conn, _index, _registry) = indexed_db(root, no_search_options());

    let search_options = lexical_search_options();
    let first =
        build_search_indexes(&conn, root, &search_options, &Config::default()).unwrap();

    std::thread::sleep(std::time::Duration::from_millis(10));
    fs::write(
        root.join("src/lib.rs"),
        r#"pub mod util;

pub fn greet() {
    util::helper();
    println!("updated");
}

pub fn farewell() {}
pub fn new_fn() {}
"#,
    )
    .unwrap();

    let (conn2, index2, _registry2) = indexed_db(root, no_search_options());
    assert!(index2.symbols.iter().any(|s| s.name == "new_fn"));

    let forced = BuildIndexOptions {
        force_search_rebuild: true,
        ..search_options
    };
    let second = build_search_indexes(&conn2, root, &forced, &Config::default()).unwrap();

    assert!(second.chunks_written >= first.chunks_written);
    assert!(second.lexical_docs_written > 0);
}

#[test]
fn test_build_search_indexes_auto_skipped_without_embedding_model() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_mini_rust_project(root);

    let (conn, _index, _registry) = indexed_db(root, BuildIndexOptions::default());

    let report = build_search_indexes(
        &conn,
        root,
        &BuildIndexOptions::default(),
        &Config::default(),
    )
    .unwrap();

    assert_eq!(report.chunks_written, 0);
    assert_eq!(report.embeddings_written, 0);
    assert_eq!(report.lexical_docs_written, 0);
}

#[test]
fn test_build_search_indexes_embeddings_only_without_lexical() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_mini_rust_project(root);

    let (conn, _index, _registry) = indexed_db(root, no_search_options());

    let search_options = BuildIndexOptions {
        with_lexical: Some(false),
        with_embeddings: Some(true),
        ..Default::default()
    };
    let config = Config {
        embedding_model: Some("/nonexistent/embedding.onnx".into()),
        ..Default::default()
    };

    let err = build_search_indexes(&conn, root, &search_options, &config)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("model file not found") || err.contains("embedding"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_build_search_indexes_auto_builds_chunks_when_embedding_configured() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_mini_rust_project(root);

    let (conn, _index, _registry) = indexed_db(root, no_search_options());

    let config = Config {
        embedding_model: Some("/nonexistent/embedding.onnx".into()),
        ..Default::default()
    };

    let err = build_search_indexes(&conn, root, &BuildIndexOptions::default(), &config)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("model file not found") || err.contains("embedding"),
        "expected model load failure after chunk build, got: {err}"
    );

    let chunk_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
        .unwrap();
    assert!(chunk_count > 0, "chunks should be built before embedding step fails");
}

#[test]
#[ignore = "requires local ONNX models; set CTX_TEST_MODELS=1 to run"]
fn test_build_search_indexes_embeddings_with_model() {
    if std::env::var("CTX_TEST_MODELS").ok().as_deref() != Some("1") {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_mini_rust_project(root);

    let (conn, _index, registry) = indexed_db(root, no_search_options());
    let paths = ctx_codegraph_models::ModelPaths::default_paths();

    let search_options = BuildIndexOptions {
        with_lexical: Some(false),
        with_embeddings: Some(true),
        ..Default::default()
    };
    let config = Config {
        embedding_model: Some(paths.embedding_onnx.to_string_lossy().into_owned()),
        embedding_tokenizer: Some(paths.embedding_tokenizer.to_string_lossy().into_owned()),
        ..Default::default()
    };

    let report = build_search_indexes(&conn, root, &search_options, &config).unwrap();
    assert!(report.chunks_written > 0);
    assert_eq!(report.embeddings_written, report.chunks_written);
    assert_eq!(report.lexical_docs_written, 0);

    let fp = read_metadata(root, &registry, "embedding_model_fingerprint");
    assert!(fp.is_some());
    let path_meta = read_metadata(root, &registry, "embedding_model_path");
    assert!(path_meta.is_some());
}

#[test]
fn test_ready_rebuild_builds_lexical_when_explicitly_requested() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_mini_rust_project(root);
    let registry = production_registry();

    rebuild_index_db_with_registry(root, no_search_options(), &registry).unwrap();
    assert!(
        matches!(
            get_index_state_with_registry(root, &no_search_options(), &registry).unwrap(),
            IndexState::Ready
        ),
        "graph index should be ready before lexical build"
    );
    assert!(!lexical_index_dir(root).join("meta.json").exists());

    let (_, report) =
        rebuild_index_db_with_registry(root, lexical_search_options(), &registry).unwrap();

    assert!(!report.full_rebuild);
    assert!(report.chunks_written > 0);
    assert!(report.lexical_docs_written > 0);
    assert!(lexical_index_dir(root).join("meta.json").exists());
}

#[test]
#[ignore = "requires local ONNX models; set CTX_TEST_MODELS=1 to run"]
fn test_ready_rebuild_builds_dense_index_when_embeddings_requested() {
    if std::env::var("CTX_TEST_MODELS").ok().as_deref() != Some("1") {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_mini_rust_project(root);
    let registry = production_registry();
    let paths = ctx_codegraph_models::ModelPaths::default_paths();
    if !paths.embedding_onnx.is_file() {
        eprintln!("skipping: embedding model missing");
        return;
    }
    if ctx_codegraph_models::EmbeddingModel::load(&paths.embedding_onnx, &paths.embedding_tokenizer)
        .is_err()
    {
        eprintln!("skipping: embedding model not loadable in this environment");
        return;
    }

    rebuild_index_db_with_registry(root, no_search_options(), &registry).unwrap();
    assert_eq!(dense_embedding_count(root), 0);

    let search_options = BuildIndexOptions {
        with_lexical: Some(true),
        with_embeddings: Some(true),
        ..Default::default()
    };
    let model_dir = paths
        .embedding_onnx
        .parent()
        .expect("embedding model parent dir");
    fs::write(
        root.join(".ctxconfig"),
        format!(
            "embedding_model = {}\nembedding_tokenizer = {}\n",
            model_dir.display(),
            paths.embedding_tokenizer.display()
        ),
    )
    .unwrap();

    let (_, report) =
        rebuild_index_db_with_registry(root, search_options.clone(), &registry).unwrap();

    assert!(!report.full_rebuild);
    assert!(report.chunks_written > 0, "chunks should be written on ready rebuild");
    assert!(
        report.embeddings_written > 0,
        "embeddings should be written on ready rebuild"
    );
    assert!(report.lexical_docs_written > 0);

    let dense_path = root.join(".ctx-codegraph/dense.sqlite");
    assert!(dense_path.exists(), "dense.sqlite should exist after --with-emb style rebuild");
    assert!(
        dense_embedding_count(root) > 0,
        "dense index should contain embeddings"
    );
}