use ctx_codegraph::context::{
    ContextBudget, ContextPackingMode, ContextSectionKind, DepthLimit, RankingMode,
    retrieve_graph_context,
};
use ctx_codegraph::backend::{BackendId, ParserId};
use ctx_codegraph::model::{
    CodeIndex, EdgeKind, FileId, FileParseStatus, FileSnapshot, GraphContextMode, GraphEdge,
    Language, ResolutionConfidence, Symbol, SymbolId, SymbolKind, TextRange,
};
use ctx_codegraph::storage::{init_schema, open_db, rebuild_index_db, save_index};
use ctx_codegraph::{
    BuildIndexOptions, GraphContextService, HybridRetrievalOptions, RetrievalStrategy,
    WorkspaceHybridBackend, retrieve_context_for_service, retrieve_context_with_options,
};
use ctx_config::Config;
use std::fs;
use std::path::PathBuf;

fn setup_test_index(dir_path: &std::path::Path) -> rusqlite::Connection {
    let mut conn = open_db(dir_path).unwrap();
    init_schema(&conn).unwrap();

    // Create some actual files to read snippets from
    std::fs::write(dir_path.join("auth_service.rs"), "// AuthService code\npub struct AuthService {}\nimpl AuthService {\n    pub fn authenticate() {}\n}\n").unwrap();
    std::fs::write(dir_path.join("login_handler.rs"), "// LoginHandler code. Rust is designed for performance and safety, especially safe concurrency. Rust is syntactically similar to C++, but can guarantee memory safety by using a borrow checker to validate references. Rust also achieves memory safety without garbage collection, and reference counting is optional.\nfn login() {\n    AuthService::authenticate();\n}\n").unwrap();
    std::fs::write(dir_path.join("token_store.rs"), "// TokenStore code. Rust is designed for performance and safety, especially safe concurrency. Rust is syntactically similar to C++, but can guarantee memory safety by using a borrow checker to validate references. Rust also achieves memory safety without garbage collection, and reference counting is optional. TokenStore code. Rust is designed for performance and safety, especially safe concurrency. Rust is syntactically similar to C++, but can guarantee memory safety by using a borrow checker to validate references. Rust also achieves memory safety without garbage collection, and reference counting is optional.\nfn issue() {}\n").unwrap();
    std::fs::write(dir_path.join("router.rs"), "// Router code. Rust is designed for performance and safety, especially safe concurrency. Rust is syntactically similar to C++, but can guarantee memory safety by using a borrow checker to validate references. Rust also achieves memory safety without garbage collection, and reference counting is optional.\npub fn router() {}\n").unwrap();
    std::fs::write(
        dir_path.join("db_pool.rs"),
        "// DbPool code\npub fn db_pool() {}\n",
    )
    .unwrap();
    std::fs::write(dir_path.join("crypto.rs"), "// Crypto code\n").unwrap();
    std::fs::write(
        dir_path.join("noisy.rs"),
        "// Noisy code\npub fn noisy_test() {}\n",
    )
    .unwrap();

    let mut index = CodeIndex {
        root: dir_path.to_path_buf(),
        files: vec![
            FileSnapshot {
                file_id: Some(FileId(1)),
                rel_path: PathBuf::from("auth_service.rs"),
                abs_path: dir_path.join("auth_service.rs"),
                language: Language::rust(),
                backend_id: BackendId::new("rust"),
                size_bytes: 100,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: None,
                parser_id: ParserId::new("rust"),
                parser_version: "1.0.0".to_string(),
                parser_config_hash: "".to_string(),
                indexed_at_ms: None,
                parse_status: FileParseStatus::Success,
            },
            FileSnapshot {
                file_id: Some(FileId(2)),
                rel_path: PathBuf::from("login_handler.rs"),
                abs_path: dir_path.join("login_handler.rs"),
                language: Language::rust(),
                backend_id: BackendId::new("rust"),
                size_bytes: 100,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: None,
                parser_id: ParserId::new("rust"),
                parser_version: "1.0.0".to_string(),
                parser_config_hash: "".to_string(),
                indexed_at_ms: None,
                parse_status: FileParseStatus::Success,
            },
            FileSnapshot {
                file_id: Some(FileId(3)),
                rel_path: PathBuf::from("token_store.rs"),
                abs_path: dir_path.join("token_store.rs"),
                language: Language::rust(),
                backend_id: BackendId::new("rust"),
                size_bytes: 100,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: None,
                parser_id: ParserId::new("rust"),
                parser_version: "1.0.0".to_string(),
                parser_config_hash: "".to_string(),
                indexed_at_ms: None,
                parse_status: FileParseStatus::Success,
            },
            FileSnapshot {
                file_id: Some(FileId(4)),
                rel_path: PathBuf::from("router.rs"),
                abs_path: dir_path.join("router.rs"),
                language: Language::rust(),
                backend_id: BackendId::new("rust"),
                size_bytes: 100,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: None,
                parser_id: ParserId::new("rust"),
                parser_version: "1.0.0".to_string(),
                parser_config_hash: "".to_string(),
                indexed_at_ms: None,
                parse_status: FileParseStatus::Success,
            },
            FileSnapshot {
                file_id: Some(FileId(5)),
                rel_path: PathBuf::from("db_pool.rs"),
                abs_path: dir_path.join("db_pool.rs"),
                language: Language::rust(),
                backend_id: BackendId::new("rust"),
                size_bytes: 100,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: None,
                parser_id: ParserId::new("rust"),
                parser_version: "1.0.0".to_string(),
                parser_config_hash: "".to_string(),
                indexed_at_ms: None,
                parse_status: FileParseStatus::Success,
            },
            FileSnapshot {
                file_id: Some(FileId(6)),
                rel_path: PathBuf::from("noisy.rs"),
                abs_path: dir_path.join("noisy.rs"),
                language: Language::rust(),
                backend_id: BackendId::new("rust"),
                size_bytes: 100,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: None,
                parser_id: ParserId::new("rust"),
                parser_version: "1.0.0".to_string(),
                parser_config_hash: "".to_string(),
                indexed_at_ms: None,
                parse_status: FileParseStatus::Success,
            },
        ],
        symbols: vec![
            Symbol {
                id: Some(SymbolId(0)),
                file_id: Some(FileId(1)),
                name: "AuthService".to_string(),
                qualified_name: "auth::AuthService".to_string(),
                kind: SymbolKind::Struct,
                language: Language::rust(),
                file: dir_path.join("auth_service.rs"),
                range: TextRange {
                    start_line: 2,
                    start_col: 1,
                    end_line: 2,
                    end_col: 20,
                },
                body_range: Some(TextRange {
                    start_line: 3,
                    start_col: 1,
                    end_line: 5,
                    end_col: 1,
                }),
            },
            Symbol {
                id: Some(SymbolId(1)),
                file_id: Some(FileId(2)),
                name: "login".to_string(),
                qualified_name: "login_handler::login".to_string(),
                kind: SymbolKind::Function,
                language: Language::rust(),
                file: dir_path.join("login_handler.rs"),
                range: TextRange {
                    start_line: 2,
                    start_col: 1,
                    end_line: 2,
                    end_col: 10,
                },
                body_range: Some(TextRange {
                    start_line: 2,
                    start_col: 1,
                    end_line: 4,
                    end_col: 1,
                }),
            },
            Symbol {
                id: Some(SymbolId(2)),
                file_id: Some(FileId(3)),
                name: "issue".to_string(),
                qualified_name: "token_store::issue".to_string(),
                kind: SymbolKind::Function,
                language: Language::rust(),
                file: dir_path.join("token_store.rs"),
                range: TextRange {
                    start_line: 2,
                    start_col: 1,
                    end_line: 2,
                    end_col: 10,
                },
                body_range: Some(TextRange {
                    start_line: 2,
                    start_col: 1,
                    end_line: 2,
                    end_col: 15,
                }),
            },
            Symbol {
                id: Some(SymbolId(3)),
                file_id: Some(FileId(4)),
                name: "router".to_string(),
                qualified_name: "router::router".to_string(),
                kind: SymbolKind::Function,
                language: Language::rust(),
                file: dir_path.join("router.rs"),
                range: TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 10,
                },
                body_range: Some(TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 20,
                }),
            },
            Symbol {
                id: Some(SymbolId(4)),
                file_id: Some(FileId(5)),
                name: "db_pool".to_string(),
                qualified_name: "db_pool::db_pool".to_string(),
                kind: SymbolKind::Function,
                language: Language::rust(),
                file: dir_path.join("db_pool.rs"),
                range: TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 10,
                },
                body_range: Some(TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 20,
                }),
            },
            Symbol {
                id: Some(SymbolId(5)),
                file_id: Some(FileId(6)),
                name: "noisy_test".to_string(),
                qualified_name: "noisy::noisy_test".to_string(),
                kind: SymbolKind::Test,
                language: Language::rust(),
                file: dir_path.join("noisy.rs"),
                range: TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 10,
                },
                body_range: Some(TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 20,
                }),
            },
        ],
        occurrences: vec![],
        call_sites: vec![],
        edges: vec![
            GraphEdge {
                id: None,
                kind: EdgeKind::Call,
                from_file_id: Some(FileId(2)),
                from_symbol_id: Some(SymbolId(1)),
                to_symbol_id: Some(SymbolId(0)),
                to_external: None,
                occurrence_id: None,
                raw_text: Some("authenticate".to_string()),
                range: None,
                confidence: ResolutionConfidence::LspExact,
                produced_by: None,
            },
            GraphEdge {
                id: None,
                kind: EdgeKind::Call,
                from_file_id: Some(FileId(1)),
                from_symbol_id: Some(SymbolId(0)),
                to_symbol_id: Some(SymbolId(2)),
                to_external: None,
                occurrence_id: None,
                raw_text: Some("issue".to_string()),
                range: None,
                confidence: ResolutionConfidence::LspExact,
                produced_by: None,
            },
            GraphEdge {
                id: None,
                kind: EdgeKind::Call,
                from_file_id: Some(FileId(3)),
                from_symbol_id: Some(SymbolId(2)),
                to_symbol_id: Some(SymbolId(3)),
                to_external: None,
                occurrence_id: None,
                raw_text: Some("router".to_string()),
                range: None,
                confidence: ResolutionConfidence::Heuristic,
                produced_by: None,
            },
            GraphEdge {
                id: None,
                kind: EdgeKind::Call,
                from_file_id: Some(FileId(4)),
                from_symbol_id: Some(SymbolId(3)),
                to_symbol_id: Some(SymbolId(4)),
                to_external: None,
                occurrence_id: None,
                raw_text: Some("db_pool".to_string()),
                range: None,
                confidence: ResolutionConfidence::Unresolved,
                produced_by: None,
            },
            GraphEdge {
                id: None,
                kind: EdgeKind::Call,
                from_file_id: Some(FileId(5)),
                from_symbol_id: Some(SymbolId(4)),
                to_symbol_id: Some(SymbolId(5)),
                to_external: None,
                occurrence_id: None,
                raw_text: Some("noisy".to_string()),
                range: None,
                confidence: ResolutionConfidence::Heuristic,
                produced_by: None,
            },
        ],
    };

    save_index(&mut conn, &mut index).unwrap();
    conn
}

#[test]
fn test_token_budget_context_packing() {
    let dir = tempfile::tempdir().unwrap();
    let conn = setup_test_index(dir.path());

    // 1. Small budget: includes root + summary only
    let small_budget = ContextBudget {
        token_budget: 100,
        model_context_window: None,
        reserve_output_tokens: 0,
        reserve_instruction_tokens: 0,
    };
    let res_small = retrieve_graph_context(
        &conn,
        "AuthService",
        GraphContextMode::Neighborhood,
        DepthLimit::Fixed(2),
        10,
        5,
        RankingMode::Hybrid,
        ContextPackingMode::Sandwich,
        true,
        3,
        &small_budget,
        false,
        &[],
        false,
        false,
    )
    .unwrap();

    assert!(res_small.snippets.len() <= 1);
    assert!(res_small.estimated_tokens <= 200);

    // 2. Large budget: includes depth-2 snippets
    let large_budget = ContextBudget {
        token_budget: 12000,
        model_context_window: None,
        reserve_output_tokens: 0,
        reserve_instruction_tokens: 0,
    };
    let res_large = retrieve_graph_context(
        &conn,
        "AuthService",
        GraphContextMode::Neighborhood,
        DepthLimit::Fixed(2),
        10,
        5,
        RankingMode::Hybrid,
        ContextPackingMode::Sandwich,
        true,
        3,
        &large_budget,
        false,
        &[],
        false,
        false,
    )
    .unwrap();

    // AuthService (root), login (inbound), issue (outbound), router (depth 2 outbound)
    assert!(res_large.snippets.len() >= 3);
    assert!(res_large.estimated_tokens > 100);
    assert!(res_large.estimated_tokens <= 12000);
}

#[test]
fn test_lost_in_middle_sandwich_packing() {
    let dir = tempfile::tempdir().unwrap();
    let conn = setup_test_index(dir.path());

    let budget = ContextBudget {
        token_budget: 12000,
        model_context_window: None,
        reserve_output_tokens: 0,
        reserve_instruction_tokens: 0,
    };
    let res = retrieve_graph_context(
        &conn,
        "AuthService",
        GraphContextMode::Neighborhood,
        DepthLimit::Fixed(2),
        10,
        5,
        RankingMode::Hybrid,
        ContextPackingMode::Sandwich,
        true,
        3,
        &budget,
        false,
        &[],
        false,
        false,
    )
    .unwrap();

    // Check sections exist
    assert_eq!(res.sections[0].kind, ContextSectionKind::Summary);
    assert_eq!(res.sections[1].kind, ContextSectionKind::Root);

    // In Sandwich mode, final recap is at the end (OmittedSummary section contains recap)
    let last_sec = &res.sections[res.sections.len() - 1];
    assert_eq!(last_sec.kind, ContextSectionKind::OmittedSummary);
    assert!(last_sec.text.contains("Most important context recap"));
}

#[test]
fn test_adaptive_depth_expansion() {
    let dir = tempfile::tempdir().unwrap();
    let conn = setup_test_index(dir.path());

    // Auto depth stops before noisy test node or too deep budget exhaustion
    let budget = ContextBudget {
        token_budget: 1000,
        model_context_window: None,
        reserve_output_tokens: 0,
        reserve_instruction_tokens: 0,
    };
    let res = retrieve_graph_context(
        &conn,
        "AuthService",
        GraphContextMode::Neighborhood,
        DepthLimit::Auto,
        10,
        5,
        RankingMode::Hybrid,
        ContextPackingMode::Sandwich,
        true,
        3,
        &budget,
        false, // include_tests = false, so noisy_test (SymbolId 5) should be penalized
        &[],
        false,
        false,
    )
    .unwrap();

    // Auto depth should not include noisy_test
    assert!(!res.nodes.iter().any(|n| n.name == "noisy_test"));
}

#[test]
fn test_ranking_and_filters() {
    let dir = tempfile::tempdir().unwrap();
    let conn = setup_test_index(dir.path());

    let budget = ContextBudget {
        token_budget: 12000,
        model_context_window: None,
        reserve_output_tokens: 0,
        reserve_instruction_tokens: 0,
    };

    // Test: direct caller (login) outranks distance-2 node (router)
    let res = retrieve_graph_context(
        &conn,
        "AuthService",
        GraphContextMode::Neighborhood,
        DepthLimit::Fixed(2),
        10,
        5,
        RankingMode::Hybrid,
        ContextPackingMode::Sandwich,
        true,
        3,
        &budget,
        false,
        &[],
        false,
        false,
    )
    .unwrap();

    let login_idx = res
        .snippets
        .iter()
        .position(|s| s.text.contains("login"))
        .unwrap();
    let router_idx = res.snippets.iter().position(|s| s.text.contains("router"));
    if let Some(r_idx) = router_idx {
        assert!(login_idx < r_idx);
    }
}

#[test]
fn test_token_budget_clamping_and_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let conn = setup_test_index(dir.path());

    // 1. Clamping test (budget 50 < 100)
    let tiny_budget = ContextBudget {
        token_budget: 50,
        model_context_window: None,
        reserve_output_tokens: 0,
        reserve_instruction_tokens: 0,
    };
    let res = retrieve_graph_context(
        &conn,
        "AuthService",
        GraphContextMode::Neighborhood,
        DepthLimit::Fixed(2),
        10,
        5,
        RankingMode::Hybrid,
        ContextPackingMode::Sandwich,
        true,
        3,
        &tiny_budget,
        false,
        &[],
        false,
        false,
    )
    .unwrap();

    assert_eq!(res.token_budget, 100);
    assert_eq!(res.requested_token_budget, Some(50));
    assert_eq!(res.effective_token_budget, Some(100));
    assert!(res.diagnostics.iter().any(|d| {
        d.message
            .contains("Requested token budget 50 is below minimum 100; using 100.")
    }));

    // 2. Unresolved edges filtering test
    let large_budget = ContextBudget {
        token_budget: 1000,
        model_context_window: None,
        reserve_output_tokens: 0,
        reserve_instruction_tokens: 0,
    };
    let res_unresolved = retrieve_graph_context(
        &conn,
        "AuthService",
        GraphContextMode::Neighborhood,
        DepthLimit::Fixed(3),
        10,
        5,
        RankingMode::Hybrid,
        ContextPackingMode::Sandwich,
        true,
        3,
        &large_budget,
        false,
        &[],
        false, // include_unresolved = false
        false,
    )
    .unwrap();

    assert!(res_unresolved.diagnostics.iter().any(|d| {
        d.message
            .contains("Filtered 1 unresolved edges because include_unresolved=false.")
    }));
}

fn setup_hybrid_mini_project() -> (tempfile::TempDir, rusqlite::Connection, WorkspaceHybridBackend) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"hybrid_mini\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
        pub fn authenticate_user() -> bool {
            verify_credentials()
        }

        fn verify_credentials() -> bool {
            true
        }

        pub fn login_handler() {
            authenticate_user();
        }
        "#,
    )
    .unwrap();

    let (_, report) = rebuild_index_db(
        root,
        BuildIndexOptions {
            with_embeddings: Some(false),
            with_lexical: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(report.chunks_written > 0, "expected chunks to be written");
    assert!(
        report.lexical_docs_written > 0,
        "expected lexical docs to be written"
    );

    let conn = open_db(root).unwrap();
    let backend = WorkspaceHybridBackend::open(root).unwrap();
    (dir, conn, backend)
}

fn default_hybrid_budget() -> ContextBudget {
    ContextBudget {
        token_budget: 8000,
        model_context_window: None,
        reserve_output_tokens: 0,
        reserve_instruction_tokens: 0,
    }
}

#[test]
fn test_retrieve_context_graph_strategy() {
    let dir = tempfile::tempdir().unwrap();
    let conn = setup_test_index(dir.path());
    let backend = WorkspaceHybridBackend::open(dir.path()).unwrap();
    let budget = default_hybrid_budget();

    let mut options = HybridRetrievalOptions::default();
    options.strategy = RetrievalStrategy::Graph;
    options.graph_options.max_nodes = 10;
    options.graph_options.max_files = 5;

    let pack = retrieve_context_with_options(
        &conn,
        dir.path(),
        &backend,
        "AuthService",
        &budget,
        &options,
    )
    .unwrap();

    assert_eq!(pack.query, "AuthService");
    assert_eq!(pack.mode, GraphContextMode::Neighborhood);
    assert!(!pack.nodes.is_empty());
    assert!(pack.nodes.iter().any(|n| n.name == "AuthService"));
}

#[test]
fn test_retrieve_context_lexical_strategy() {
    let (_dir, conn, backend) = setup_hybrid_mini_project();
    let budget = default_hybrid_budget();

    let mut options = HybridRetrievalOptions::default();
    options.strategy = RetrievalStrategy::Lexical;
    options.hybrid_top_k = 10;
    options.graph_options.max_nodes = 10;
    options.graph_options.with_snippets = true;

    let pack = retrieve_context_with_options(
        &conn,
        _dir.path(),
        &backend,
        "authenticate",
        &budget,
        &options,
    )
    .unwrap();

    assert_eq!(pack.query, "authenticate");
    assert!(
        !pack.nodes.is_empty(),
        "lexical search should return symbol hits"
    );
    assert!(
        pack.nodes
            .iter()
            .any(|n| n.name.contains("authenticate") || n.qualified_name.contains("authenticate")),
        "expected authenticate-related symbol in results: {:?}",
        pack.nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
    );
}

#[test]
fn test_retrieve_context_dense_strategy_without_embedding_model() {
    let (_dir, conn, backend) = setup_hybrid_mini_project();
    let budget = default_hybrid_budget();

    let mut options = HybridRetrievalOptions::default();
    options.strategy = RetrievalStrategy::Dense;
    options.hybrid_top_k = 5;

    let pack = retrieve_context_with_options(
        &conn,
        _dir.path(),
        &backend,
        "authenticate",
        &budget,
        &options,
    )
    .unwrap();

    assert_eq!(pack.query, "authenticate");
    assert!(pack.nodes.is_empty());
    assert!(pack.diagnostics.iter().any(|d| {
        d.severity == "warning"
            && d.message.contains("No hybrid search hits")
    }));
}

#[test]
fn test_retrieve_context_hybrid_strategy_lexical_only() {
    let (_dir, conn, backend) = setup_hybrid_mini_project();
    let budget = default_hybrid_budget();

    let mut options = HybridRetrievalOptions::default();
    options.strategy = RetrievalStrategy::Hybrid;
    options.hybrid_top_k = 10;
    options.rrf_k = 60;
    options.graph_options.max_nodes = 10;

    let pack = retrieve_context_with_options(
        &conn,
        _dir.path(),
        &backend,
        "login_handler",
        &budget,
        &options,
    )
    .unwrap();

    assert_eq!(pack.query, "login_handler");
    assert!(!pack.nodes.is_empty());
    assert!(
        pack.nodes.iter().any(|n| n.name.contains("login")),
        "hybrid RRF should surface lexical hits: {:?}",
        pack.nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
    );
}

#[test]
fn test_retrieve_context_for_service_graph_strategy() {
    let dir = tempfile::tempdir().unwrap();
    let conn = setup_test_index(dir.path());
    let service = GraphContextService::new(dir.path(), conn);
    let budget = default_hybrid_budget();
    let config = Config::default();

    let mut options = HybridRetrievalOptions::default();
    options.strategy = RetrievalStrategy::Graph;
    options.graph_options.max_nodes = 10;

    let pack = retrieve_context_for_service(&service, "AuthService", &budget, &options, &config)
        .unwrap();

    assert!(!pack.nodes.is_empty());
    assert!(pack.nodes.iter().any(|n| n.name == "AuthService"));
}

#[test]
fn test_retrieve_context_for_service_hybrid_not_configured() {
    let (_dir, conn, _backend) = setup_hybrid_mini_project();
    let service = GraphContextService::new(_dir.path(), conn);
    let budget = default_hybrid_budget();
    let config = Config::default();

    let mut options = HybridRetrievalOptions::default();
    options.strategy = RetrievalStrategy::Lexical;

    let err = retrieve_context_for_service(&service, "authenticate", &budget, &options, &config)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("hybrid search not configured"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_retrieve_context_for_service_lexical_with_search_config() {
    let (_dir, conn, _backend) = setup_hybrid_mini_project();
    let service = GraphContextService::new(_dir.path(), conn);
    let budget = default_hybrid_budget();
    let config = Config {
        embedding_model: Some("/tmp/ctx-test-nonexistent-embedding.onnx".to_string()),
        ..Default::default()
    };

    let mut options = HybridRetrievalOptions::default();
    options.strategy = RetrievalStrategy::Lexical;
    options.hybrid_top_k = 10;
    options.graph_options.max_nodes = 10;

    let pack =
        retrieve_context_for_service(&service, "verify_credentials", &budget, &options, &config)
            .unwrap();

    assert!(!pack.nodes.is_empty());
    assert!(
        pack.nodes
            .iter()
            .any(|n| n.name.contains("verify") || n.qualified_name.contains("verify"))
    );
}

#[test]
fn test_retrieve_context_enable_rerank_without_model_is_noop() {
    let (_dir, conn, backend) = setup_hybrid_mini_project();
    let budget = default_hybrid_budget();

    let mut options = HybridRetrievalOptions::default();
    options.strategy = RetrievalStrategy::Lexical;
    options.enable_rerank = true;
    options.rerank_top_k = 5;
    options.hybrid_top_k = 10;
    options.graph_options.max_nodes = 10;

    let pack = retrieve_context_with_options(
        &conn,
        _dir.path(),
        &backend,
        "authenticate",
        &budget,
        &options,
    )
    .unwrap();

    assert!(!pack.nodes.is_empty());
    assert!(
        !backend.has_reranker(),
        "test backend should not load a reranker model"
    );
}

#[test]
fn test_retrieve_context_expansion_max_children_zero_skips_child_expansion() {
    let (_dir, conn, backend) = setup_hybrid_mini_project();
    let budget = default_hybrid_budget();

    let mut options = HybridRetrievalOptions::default();
    options.strategy = RetrievalStrategy::Lexical;
    options.expansion_max_children = 0;
    options.hybrid_top_k = 10;
    options.graph_options.max_nodes = 20;

    let pack = retrieve_context_with_options(
        &conn,
        _dir.path(),
        &backend,
        "authenticate",
        &budget,
        &options,
    )
    .unwrap();

    assert!(!pack.nodes.is_empty());
}

#[test]
fn test_retrieve_context_child_chunk_expansion() {
    use ctx_codegraph_chunk::ChunkId;
    use ctx_codegraph_lexical::{IndexDoc, LexicalIndex};
    use ctx_codegraph_lang::model::SymbolId;

    let (_dir, conn, backend) = setup_hybrid_mini_project();
    let root = _dir.path();

    let parent_symbol_id: i64 = conn
        .query_row(
            "SELECT id FROM symbols WHERE name = 'authenticate_user'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let child_symbol_id: i64 = conn
        .query_row(
            "SELECT id FROM symbols WHERE name = 'verify_credentials'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let file_id: i64 = conn
        .query_row(
            "SELECT file_id FROM symbols WHERE id = ?1",
            [parent_symbol_id],
            |row| row.get(0),
        )
        .unwrap();

    conn.execute(
        "INSERT INTO chunks (
            id, symbol_id, parent_chunk_id, file_id, kind, text_hash,
            token_count, start_line, end_line, qualified_name
         ) VALUES (9001, ?1, NULL, ?2, 'ParentSummary', 'parent-expand', 8, 1, 3, 'auth_expand_parent_query')",
        rusqlite::params![parent_symbol_id, file_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chunks (
            id, symbol_id, parent_chunk_id, file_id, kind, text_hash,
            token_count, start_line, end_line, qualified_name
         ) VALUES (9002, ?1, 9001, ?2, 'SymbolBody', 'child-expand', 10, 4, 8, 'verify_credentials_child')",
        rusqlite::params![child_symbol_id, file_id],
    )
    .unwrap();

    let docs = vec![IndexDoc {
        chunk_id: ChunkId(9001),
        symbol_id: Some(SymbolId(parent_symbol_id)),
        path: "src/lib.rs".to_string(),
        qualified_name: "auth_expand_parent_query".to_string(),
        text: "auth_expand_parent_query parent summary".to_string(),
    }];
    let mut lexical = LexicalIndex::open(root).unwrap();
    lexical.build(&docs).unwrap();

    let budget = default_hybrid_budget();
    let mut options = HybridRetrievalOptions::default();
    options.strategy = RetrievalStrategy::Lexical;
    options.expansion_max_children = 2;
    options.hybrid_top_k = 5;
    options.graph_options.max_nodes = 20;

    let pack = retrieve_context_with_options(
        &conn,
        root,
        &backend,
        "auth_expand_parent_query",
        &budget,
        &options,
    )
    .unwrap();

    assert!(
        pack.nodes.iter().any(|n| n.name == "verify_credentials"),
        "child chunk expansion should include verify_credentials: {:?}",
        pack.nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
    );
}

#[test]
fn test_retrieve_context_dense_with_embedding_when_model_available() {
    use ctx_codegraph_models::{DEFAULT_EMBEDDING_ONNX, EmbeddingModel};
    use std::path::PathBuf;

    let embedding_path = PathBuf::from(DEFAULT_EMBEDDING_ONNX);
    if !embedding_path.is_file() {
        eprintln!(
            "skipping dense embedding test: model not found at {}",
            embedding_path.display()
        );
        return;
    }
    let tokenizer_dir = embedding_path.parent().unwrap();
    let embedding_model = match EmbeddingModel::load(&embedding_path, tokenizer_dir) {
        Ok(model) => model,
        Err(err) => {
            eprintln!("skipping dense embedding test: model load failed: {err}");
            return;
        }
    };

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"dense_embed\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
        pub fn search_target_alpha() -> i32 { 42 }
        pub fn unrelated_beta() -> i32 { 0 }
        "#,
    )
    .unwrap();

    let config = Config {
        embedding_model: Some(embedding_path.display().to_string()),
        ..Default::default()
    };

    let (_, report) = rebuild_index_db(
        root,
        BuildIndexOptions {
            with_embeddings: Some(true),
            with_lexical: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    if report.embeddings_written == 0 {
        eprintln!("skipping dense embedding test: index build wrote no embeddings");
        return;
    }

    let conn = open_db(root).unwrap();
    let backend = WorkspaceHybridBackend::open(root)
        .unwrap()
        .with_embedding(embedding_model);

    let budget = default_hybrid_budget();
    let mut options = HybridRetrievalOptions::default();
    options.strategy = RetrievalStrategy::Dense;
    options.hybrid_top_k = 5;
    options.graph_options.max_nodes = 10;

    let pack = retrieve_context_with_options(
        &conn,
        root,
        &backend,
        "search_target_alpha",
        &budget,
        &options,
    )
    .unwrap();

    assert!(
        !pack.nodes.is_empty(),
        "dense search with embedding model should return symbol hits"
    );
    assert!(
        pack.nodes
            .iter()
            .any(|n| n.name.contains("search_target") || n.qualified_name.contains("search_target")),
        "expected search_target_alpha in dense results: {:?}",
        pack.nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
    );
    let _ = config;
}

#[test]
fn test_retrieve_context_rerank_path_when_model_available() {
    use ctx_codegraph_models::{DEFAULT_RERANKER_ONNX, RerankerModel};
    use std::path::PathBuf;

    let reranker_path = PathBuf::from(DEFAULT_RERANKER_ONNX);
    if !reranker_path.is_file() {
        eprintln!("skipping rerank integration test: reranker model not found");
        return;
    }
    let rerank_tokenizer = reranker_path.parent().unwrap();
    let reranker_model = match RerankerModel::load(&reranker_path, rerank_tokenizer) {
        Ok(model) => model,
        Err(err) => {
            eprintln!("skipping rerank integration test: model load failed: {err}");
            return;
        }
    };

    let (_dir, conn, backend) = setup_hybrid_mini_project();
    let root = _dir.path();
    let backend = backend.with_reranker(reranker_model);
    assert!(backend.has_reranker());

    let budget = default_hybrid_budget();
    let mut options = HybridRetrievalOptions::default();
    options.strategy = RetrievalStrategy::Lexical;
    options.enable_rerank = true;
    options.rerank_top_k = 5;
    options.hybrid_top_k = 10;
    options.graph_options.max_nodes = 10;

    let pack = retrieve_context_with_options(
        &conn,
        root,
        &backend,
        "authenticate",
        &budget,
        &options,
    )
    .unwrap();

    assert!(!pack.nodes.is_empty());
    assert!(
        pack.nodes.iter().any(|n| n.name.contains("authenticate")),
        "rerank path should still return lexical hits: {:?}",
        pack.nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
    );
}
