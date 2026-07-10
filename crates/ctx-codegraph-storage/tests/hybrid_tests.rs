use ctx_codegraph_chunk::ChunkId;
use ctx_codegraph_lang::model::SymbolId;
use ctx_codegraph_lexical::{IndexDoc, LexicalIndex};
use ctx_codegraph_models::{EmbeddingModel, ModelPaths};
use ctx_codegraph_search::traits::SearchResult;
use ctx_codegraph_search::{HybridQuery, HybridSearchBackend};
use ctx_codegraph_storage::hybrid::{
    WorkspaceHybridBackend, apply_rerank_scores, chunk_ids_from_results,
};
use ctx_codegraph_storage::index::BuildIndexOptions;
use ctx_codegraph_storage::storage::{open_db, rebuild_index_db};
use ctx_codegraph_store::storage::build_search_indexes;
use ctx_config::Config;
use std::fs;
use std::path::Path;

fn seed_lexical_index(workspace: &Path) {
    let docs = vec![
        IndexDoc {
            chunk_id: ChunkId(1),
            symbol_id: Some(SymbolId(10)),
            path: "src/lib.rs".to_string(),
            qualified_name: "my_crate::run_pipeline".to_string(),
            text: "pub fn run_pipeline() { process(); }".to_string(),
        },
        IndexDoc {
            chunk_id: ChunkId(2),
            symbol_id: None,
            path: "src/util.rs".to_string(),
            qualified_name: "my_crate::helper".to_string(),
            text: "fn helper() {}".to_string(),
        },
    ];
    let mut lexical = LexicalIndex::open(workspace).unwrap();
    lexical.build(&docs).unwrap();
}

#[test]
fn workspace_hybrid_backend_open_creates_index_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let backend = WorkspaceHybridBackend::open(dir.path()).unwrap();

    assert!(dir.path().join(".ctx-codegraph/lexical").exists());
    assert!(dir.path().join(".ctx-codegraph/dense.sqlite").exists());
    drop(backend);
}

#[test]
fn search_lexical_returns_indexed_hits() {
    let dir = tempfile::tempdir().unwrap();
    seed_lexical_index(dir.path());

    let backend = WorkspaceHybridBackend::open(dir.path()).unwrap();
    let query = HybridQuery {
        workspace_root: dir.path(),
        text: "run_pipeline",
        limit: 5,
    };
    let hits = HybridSearchBackend::search_lexical(&&backend, query).unwrap();

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].chunk_id, ChunkId(1));
    assert_eq!(hits[0].symbol_id, SymbolId(10));
    assert!(hits[0].score > 0.0);
}

#[test]
fn search_dense_without_embedding_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let backend = WorkspaceHybridBackend::open(dir.path()).unwrap();

    let query = HybridQuery {
        workspace_root: dir.path(),
        text: "anything",
        limit: 5,
    };
    let hits = HybridSearchBackend::search_dense(&&backend, query).unwrap();
    assert!(hits.is_empty());
}

#[test]
fn chunk_ids_from_results_maps_chunk_ids() {
    let results = vec![
        SearchResult {
            chunk_id: ChunkId(7),
            symbol_id: SymbolId(1),
            score: 1.0,
            snippet: None,
        },
        SearchResult {
            chunk_id: ChunkId(42),
            symbol_id: SymbolId(2),
            score: 0.5,
            snippet: None,
        },
    ];

    assert_eq!(
        chunk_ids_from_results(&results),
        vec![ChunkId(7), ChunkId(42)]
    );
    assert!(chunk_ids_from_results(&[]).is_empty());
}

#[test]
fn try_with_config_returns_none_when_search_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config::default_values();

    let backend = WorkspaceHybridBackend::try_with_config(dir.path(), &config).unwrap();
    assert!(backend.is_none());
}

#[test]
fn try_with_config_opens_backend_when_embedding_path_set() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        embedding_model: Some("/nonexistent/model.onnx".into()),
        ..Config::default_values()
    };

    let backend = WorkspaceHybridBackend::try_with_config(dir.path(), &config)
        .unwrap()
        .expect("backend should open even if model load fails");
    let query = HybridQuery {
        workspace_root: dir.path(),
        text: "query",
        limit: 3,
    };
    let dense_hits = HybridSearchBackend::search_dense(&&backend, query).unwrap();
    assert!(dense_hits.is_empty(), "model load failed so dense search is disabled");
}

#[test]
fn apply_rerank_scores_updates_result_scores() {
    let mut results = vec![
        SearchResult {
            chunk_id: ChunkId(1),
            symbol_id: SymbolId(10),
            score: 0.1,
            snippet: None,
        },
        SearchResult {
            chunk_id: ChunkId(2),
            symbol_id: SymbolId(20),
            score: 0.2,
            snippet: None,
        },
    ];

    apply_rerank_scores(&mut results, &[0.95, 0.05]);

    assert!((results[0].score - 0.95).abs() < f32::EPSILON);
    assert!((results[1].score - 0.05).abs() < f32::EPSILON);
}

#[test]
fn rerank_results_without_reranker_is_noop() {
    let dir = tempfile::tempdir().unwrap();
    seed_lexical_index(dir.path());
    let backend = WorkspaceHybridBackend::open(dir.path()).unwrap();
    let conn = rusqlite::Connection::open_in_memory().unwrap();

    let mut results = vec![SearchResult {
        chunk_id: ChunkId(1),
        symbol_id: SymbolId(10),
        score: 0.5,
        snippet: None,
    }];

    backend
        .rerank_results(&conn, "query", &mut results, 5)
        .unwrap();
    assert!((results[0].score - 0.5).abs() < f32::EPSILON);
}

#[test]
fn search_lexical_maps_missing_symbol_id_to_zero() {
    let dir = tempfile::tempdir().unwrap();
    seed_lexical_index(dir.path());

    let backend = WorkspaceHybridBackend::open(dir.path()).unwrap();
    let query = HybridQuery {
        workspace_root: dir.path(),
        text: "helper",
        limit: 5,
    };
    let hits = HybridSearchBackend::search_lexical(&&backend, query).unwrap();

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].chunk_id, ChunkId(2));
    assert_eq!(hits[0].symbol_id, SymbolId(0));
}

fn no_search_options() -> BuildIndexOptions {
    BuildIndexOptions {
        with_lexical: Some(false),
        with_embeddings: Some(false),
        ..BuildIndexOptions::default()
    }
}

fn setup_dense_search_project(root: &Path) {
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"dense_hybrid\"\nversion=\"0.1.0\"\nedition=\"2021\"",
    )
    .unwrap();
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"pub fn run_pipeline() {
    process_data();
}

fn process_data() {
    println!("processing");
}
"#,
    )
    .unwrap();
}

#[test]
#[ignore = "requires local ONNX models; set CTX_TEST_MODELS=1 to run"]
fn search_dense_with_embedding_model_returns_hits() {
    if std::env::var("CTX_TEST_MODELS").ok().as_deref() != Some("1") {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_dense_search_project(root);

    let paths = ModelPaths::default_paths();
    if !paths.embedding_onnx.is_file() {
        eprintln!(
            "skipping: embedding model file missing at {}",
            paths.embedding_onnx.display()
        );
        return;
    }

    let model = match EmbeddingModel::load(&paths.embedding_onnx, &paths.embedding_tokenizer) {
        Ok(model) => model,
        Err(err) => {
            eprintln!("skipping: could not load embedding model: {err}");
            return;
        }
    };

    rebuild_index_db(root, no_search_options()).unwrap();

    let conn = open_db(root).unwrap();
    let config = Config {
        embedding_model: Some(paths.embedding_onnx.to_string_lossy().into_owned()),
        embedding_tokenizer: Some(paths.embedding_tokenizer.to_string_lossy().into_owned()),
        ..Config::default_values()
    };
    let search_options = BuildIndexOptions {
        with_lexical: Some(true),
        with_embeddings: Some(true),
        ..BuildIndexOptions::default()
    };
    let report = build_search_indexes(&conn, root, &search_options, &config).unwrap();
    assert!(report.embeddings_written > 0, "expected dense embeddings");
    let backend = WorkspaceHybridBackend::open(root).unwrap().with_embedding(model);

    let query = HybridQuery {
        workspace_root: root,
        text: "run pipeline process data",
        limit: 5,
    };
    let hits = HybridSearchBackend::search_dense(&&backend, query).unwrap();

    assert!(!hits.is_empty(), "dense search should return indexed chunks");
    assert!(hits.iter().all(|h| h.score > 0.0));
    assert!(hits.iter().all(|h| h.symbol_id == SymbolId(0)));
}

#[test]
#[ignore = "requires local ONNX models; set CTX_TEST_MODELS=1 to run"]
fn try_with_config_loads_embedding_when_model_present() {
    if std::env::var("CTX_TEST_MODELS").ok().as_deref() != Some("1") {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_dense_search_project(root);

    let paths = ModelPaths::default_paths();
    if !paths.embedding_onnx.is_file() {
        eprintln!(
            "skipping: embedding model file missing at {}",
            paths.embedding_onnx.display()
        );
        return;
    }
    if EmbeddingModel::load(&paths.embedding_onnx, &paths.embedding_tokenizer).is_err() {
        eprintln!("skipping: embedding model could not be loaded");
        return;
    }

    let config = Config {
        embedding_model: Some(paths.embedding_onnx.to_string_lossy().into_owned()),
        embedding_tokenizer: Some(paths.embedding_tokenizer.to_string_lossy().into_owned()),
        ..Config::default_values()
    };

    rebuild_index_db(root, no_search_options()).unwrap();
    let conn = open_db(root).unwrap();
    let search_options = BuildIndexOptions {
        with_lexical: Some(true),
        with_embeddings: Some(true),
        ..BuildIndexOptions::default()
    };
    build_search_indexes(&conn, root, &search_options, &config).unwrap();

    let backend = WorkspaceHybridBackend::try_with_config(root, &config)
        .unwrap()
        .expect("search should be enabled when embedding path is set");

    let query = HybridQuery {
        workspace_root: root,
        text: "process data",
        limit: 3,
    };
    let hits = HybridSearchBackend::search_dense(&&backend, query).unwrap();
    assert!(!hits.is_empty());
}