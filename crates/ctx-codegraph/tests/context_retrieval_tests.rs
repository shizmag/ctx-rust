use ctx_codegraph::context::{
    ContextBudget, ContextPackingMode, ContextSectionKind, DepthLimit, RankingMode,
    retrieve_graph_context,
};
use ctx_codegraph::model::{
    CodeIndex, EdgeKind, FileId, FileParseStatus, FileSnapshot, GraphContextMode, GraphEdge,
    Language, ResolutionConfidence, Symbol, SymbolId, SymbolKind, TextRange,
};
use ctx_codegraph::storage::{init_schema, open_db, save_index};
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
                backend_id: "rust".to_string(),
                size_bytes: 100,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: None,
                parser_id: "rust".to_string(),
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
                backend_id: "rust".to_string(),
                size_bytes: 100,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: None,
                parser_id: "rust".to_string(),
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
                backend_id: "rust".to_string(),
                size_bytes: 100,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: None,
                parser_id: "rust".to_string(),
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
                backend_id: "rust".to_string(),
                size_bytes: 100,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: None,
                parser_id: "rust".to_string(),
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
                backend_id: "rust".to_string(),
                size_bytes: 100,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: None,
                parser_id: "rust".to_string(),
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
                backend_id: "rust".to_string(),
                size_bytes: 100,
                mtime_ms: 100,
                mtime_ns: None,
                content_hash: None,
                parser_id: "rust".to_string(),
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
