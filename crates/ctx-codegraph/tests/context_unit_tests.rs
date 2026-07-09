use ctx_codegraph::{
    ApproxTokenEstimator, ContextBudget, ContextCandidate, ContextQuery,
    ContextRanker, EdgeDirection, GraphContextEdge, GraphRanker, HybridRanker, LanguageObject,
    LanguageObjectKind, LexicalRanker, SourceRange, Symbol, SymbolId, SymbolKind, TextRange,
    TokenEstimator, extract_snippet, is_subsequence, resolve_roots, tokenize,
};
use ctx_codegraph::storage::{init_schema, open_db, save_index};
use ctx_codegraph::model::{
    CodeIndex, FileId, FileParseStatus, FileSnapshot, Language,
};
use std::path::PathBuf;

fn make_lang_object(
    id: i64,
    name: &str,
    qualified_name: &str,
    file_path: PathBuf,
) -> LanguageObject {
    LanguageObject {
        id: SymbolId(id),
        name: name.to_string(),
        qualified_name: qualified_name.to_string(),
        kind: LanguageObjectKind::Function,
        file_path,
        range: SourceRange {
            start_line: 1,
            start_col: 1,
            end_line: 10,
            end_col: 1,
        },
        signature: None,
        language: Some("rust".to_string()),
    }
}

fn make_candidate(
    id: i64,
    name: &str,
    qualified_name: &str,
    file_path: PathBuf,
    distance: usize,
    via_edge: Option<GraphContextEdge>,
) -> ContextCandidate {
    ContextCandidate {
        node: make_lang_object(id, name, qualified_name, file_path.clone()),
        distance,
        direction: EdgeDirection::Outbound,
        via_edge,
        file_path,
        range: SourceRange {
            start_line: 1,
            start_col: 1,
            end_line: 10,
            end_col: 1,
        },
        graph_score: 0.0,
        lexical_score: 0.0,
        combined_score: 0.0,
        estimated_tokens: 0,
        reason: "test".to_string(),
    }
}

fn make_query(roots: Vec<LanguageObject>, query_string: &str, include_tests: bool) -> ContextQuery {
    ContextQuery {
        query_string: query_string.to_string(),
        roots,
        include_tests,
    }
}

// --- text.rs: tokenize ---

#[test]
fn tokenize_camel_case_auth_service() {
    let tokens = tokenize("AuthService");
    assert!(tokens.contains(&"auth".to_string()));
    assert!(tokens.contains(&"service".to_string()));
    assert!(tokens.contains(&"authservice".to_string()));
}

#[test]
fn tokenize_path_separators() {
    let tokens = tokenize("src/auth_service.rs");
    assert!(tokens.contains(&"src".to_string()));
    assert!(tokens.contains(&"auth".to_string()));
    assert!(tokens.contains(&"service".to_string()));
    assert!(tokens.contains(&"rs".to_string()));
}

#[test]
fn tokenize_underscore_and_dash_separators() {
    let tokens = tokenize("foo_bar-baz");
    assert!(tokens.contains(&"foo".to_string()));
    assert!(tokens.contains(&"bar".to_string()));
    assert!(tokens.contains(&"baz".to_string()));
}

#[test]
fn tokenize_colon_qualified_name() {
    let tokens = tokenize("auth::AuthService");
    assert!(tokens.contains(&"auth".to_string()));
    assert!(tokens.contains(&"service".to_string()));
}

// --- text.rs: is_subsequence ---

#[test]
fn is_subsequence_empty_sub_is_true() {
    assert!(is_subsequence("", "anything"));
}

#[test]
fn is_subsequence_full_match() {
    assert!(is_subsequence("auth", "auth"));
    assert!(is_subsequence("aut", "authenticate"));
}

#[test]
fn is_subsequence_no_match() {
    assert!(!is_subsequence("xyz", "authenticate"));
    assert!(!is_subsequence("tba", "auth"));
}

// --- text.rs: extract_snippet ---

#[test]
fn extract_snippet_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.rs");
    std::fs::write(&path, "").unwrap();

    let snippet = extract_snippet(
        &path,
        SourceRange {
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 1,
        },
        None,
        false,
        2,
    )
    .unwrap();
    assert_eq!(snippet, "");
}

#[test]
fn extract_snippet_small_body_with_context_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.rs");
    let content = (1..=10)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, &content).unwrap();

    let snippet = extract_snippet(
        &path,
        SourceRange {
            start_line: 5,
            start_col: 1,
            end_line: 5,
            end_col: 10,
        },
        Some(SourceRange {
            start_line: 5,
            start_col: 1,
            end_line: 7,
            end_col: 10,
        }),
        false,
        1,
    )
    .unwrap();

    assert!(snippet.contains("line 4"));
    assert!(snippet.contains("line 5"));
    assert!(snippet.contains("line 7"));
    assert!(snippet.contains("line 8"));
}

#[test]
fn extract_snippet_large_body_truncates_with_omitted_marker() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.rs");
    let content = (1..=100)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, &content).unwrap();

    let snippet = extract_snippet(
        &path,
        SourceRange {
            start_line: 10,
            start_col: 1,
            end_line: 90,
            end_col: 1,
        },
        Some(SourceRange {
            start_line: 10,
            start_col: 1,
            end_line: 90,
            end_col: 1,
        }),
        false,
        0,
    )
    .unwrap();

    assert!(snippet.contains("// ..."));
    assert!(snippet.contains("lines omitted"));
    assert!(snippet.contains("line 10"));
    assert!(snippet.contains("line 90"));
    assert!(!snippet.contains("line 50"));
}

#[test]
fn extract_snippet_start_line_beyond_file_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tiny.rs");
    std::fs::write(&path, "only line\n").unwrap();

    let snippet = extract_snippet(
        &path,
        SourceRange {
            start_line: 100,
            start_col: 1,
            end_line: 100,
            end_col: 1,
        },
        None,
        false,
        0,
    )
    .unwrap();
    assert_eq!(snippet, "");
}

// --- types.rs: ContextBudget::effective_budget ---

#[test]
fn effective_budget_no_window_limit_uses_token_budget() {
    let budget = ContextBudget {
        token_budget: 5000,
        model_context_window: None,
        reserve_output_tokens: 1000,
        reserve_instruction_tokens: 500,
    };
    assert_eq!(budget.effective_budget(), 5000);
}

#[test]
fn effective_budget_window_smaller_than_budget() {
    let budget = ContextBudget {
        token_budget: 10000,
        model_context_window: Some(8000),
        reserve_output_tokens: 2000,
        reserve_instruction_tokens: 1000,
    };
    // 8000 - 3000 reserved = 5000
    assert_eq!(budget.effective_budget(), 5000);
}

#[test]
fn effective_budget_reserved_exceeds_window_returns_zero() {
    let budget = ContextBudget {
        token_budget: 10000,
        model_context_window: Some(1000),
        reserve_output_tokens: 800,
        reserve_instruction_tokens: 500,
    };
    assert_eq!(budget.effective_budget(), 0);
}

// --- ranking.rs: ApproxTokenEstimator ---

#[test]
fn approx_token_estimator_empty_and_nonempty() {
    let est = ApproxTokenEstimator;
    assert_eq!(est.estimate_tokens(""), 0);
    assert_eq!(est.estimate_tokens("abcd"), 1);
    assert_eq!(est.estimate_tokens("abcdefghi"), 3); // (9 + 3) / 4
}

// --- ranking.rs: GraphRanker ---

#[test]
fn graph_ranker_distance_weights() {
    let root_path = PathBuf::from("/proj/src/root.rs");
    // Different parent directory so locality bonus does not apply.
    let other_path = PathBuf::from("/proj/lib/other.rs");
    let roots = vec![make_lang_object(0, "root", "root", root_path.clone())];
    let query = make_query(roots, "foo", true);

    let candidates = vec![
        make_candidate(1, "d0", "d0", other_path.clone(), 0, None),
        make_candidate(2, "d1", "d1", other_path.clone(), 1, None),
        make_candidate(3, "d2", "d2", other_path.clone(), 2, None),
        make_candidate(4, "d3", "d3", other_path.clone(), 3, None),
        make_candidate(5, "d4", "d4", other_path.clone(), 4, None),
    ];

    let ranked = GraphRanker.rank(&query, candidates);
    assert_eq!(ranked[0].graph_score, 10.0);
    assert_eq!(ranked[1].graph_score, 6.0);
    assert_eq!(ranked[2].graph_score, 3.0);
    assert_eq!(ranked[3].graph_score, 1.0);
    assert_eq!(ranked[4].graph_score, 0.0);
}

#[test]
fn graph_ranker_same_file_and_same_dir_bonus() {
    let root_path = PathBuf::from("/proj/src/auth_service.rs");
    let same_dir_path = PathBuf::from("/proj/src/login_handler.rs");
    let other_dir_path = PathBuf::from("/proj/lib/util.rs");
    let roots = vec![make_lang_object(0, "AuthService", "auth::AuthService", root_path.clone())];
    let query = make_query(roots, "auth", true);

    let same_file = make_candidate(1, "same_file", "same_file", root_path, 1, None);
    let same_dir = make_candidate(2, "same_dir", "same_dir", same_dir_path, 1, None);
    let other = make_candidate(3, "other", "other", other_dir_path, 1, None);

    let ranked = GraphRanker.rank(&query, vec![other, same_dir, same_file]);
    assert_eq!(ranked[0].node.name, "same_file");
    assert_eq!(ranked[0].graph_score, 8.0); // 6 distance + 2 same file
    assert_eq!(ranked[1].node.name, "same_dir");
    assert_eq!(ranked[1].graph_score, 7.0); // 6 distance + 1 same dir
    assert_eq!(ranked[2].graph_score, 6.0);
}

#[test]
fn graph_ranker_edge_confidence_weights() {
    let path = PathBuf::from("/proj/src/foo.rs");
    let roots = vec![make_lang_object(0, "root", "root", path.clone())];
    let query = make_query(roots, "foo", true);

    let exact = make_candidate(
        1,
        "exact",
        "exact",
        path.clone(),
        1,
        Some(GraphContextEdge {
            from: SymbolId(0),
            to: SymbolId(1),
            label: None,
            confidence: Some("LspExact".to_string()),
        }),
    );
    let syntax = make_candidate(
        2,
        "syntax",
        "syntax",
        path.clone(),
        1,
        Some(GraphContextEdge {
            from: SymbolId(0),
            to: SymbolId(2),
            label: None,
            confidence: Some("Syntax".to_string()),
        }),
    );
    let heuristic = make_candidate(
        3,
        "heuristic",
        "heuristic",
        path.clone(),
        1,
        Some(GraphContextEdge {
            from: SymbolId(0),
            to: SymbolId(3),
            label: None,
            confidence: Some("Heuristic".to_string()),
        }),
    );
    let unresolved = make_candidate(
        4,
        "unresolved",
        "unresolved",
        path.clone(),
        1,
        Some(GraphContextEdge {
            from: SymbolId(0),
            to: SymbolId(4),
            label: None,
            confidence: Some("Unresolved".to_string()),
        }),
    );

    let ranked = GraphRanker.rank(&query, vec![unresolved, heuristic, syntax, exact]);
    assert_eq!(ranked[0].node.name, "exact");
    assert!((ranked[0].graph_score - 10.0).abs() < 0.01); // 6 + 2 same file + 2 exact
    assert!((ranked[1].graph_score - 9.2).abs() < 0.01); // 6 + 2 + 1.2
    assert!((ranked[2].graph_score - 8.5).abs() < 0.01); // 6 + 2 + 0.5
    assert!((ranked[3].graph_score - 7.0).abs() < 0.01); // 6 + 2 - 1 unresolved
}

#[test]
fn graph_ranker_test_penalty_when_excluded() {
    let path = PathBuf::from("/proj/tests/my_test.rs");
    let roots = vec![make_lang_object(0, "root", "root", PathBuf::from("/proj/src/root.rs"))];
    let query = make_query(roots, "test", false);

    let normal = make_candidate(1, "handler", "handler", PathBuf::from("/proj/src/handler.rs"), 1, None);
    let test_fn = make_candidate(2, "run_test", "tests::run_test", path, 1, None);

    let ranked = GraphRanker.rank(&query, vec![test_fn, normal]);
    assert_eq!(ranked[0].node.name, "handler");
    assert_eq!(ranked[0].graph_score, 7.0); // 6 distance + 1 same dir
    assert_eq!(ranked[1].graph_score, 4.0); // 6 distance - 2 test penalty
}

#[test]
fn graph_ranker_vendor_penalty() {
    let roots = vec![make_lang_object(0, "root", "root", PathBuf::from("/proj/src/root.rs"))];
    let query = make_query(roots, "foo", true);

    let normal = make_candidate(1, "handler", "handler", PathBuf::from("/proj/src/handler.rs"), 1, None);
    let vendor = make_candidate(
        2,
        "vendor_fn",
        "vendor_fn",
        PathBuf::from("/proj/vendor/lib/generated/foo.rs"),
        1,
        None,
    );

    let ranked = GraphRanker.rank(&query, vec![vendor, normal]);
    assert_eq!(ranked[0].node.name, "handler");
    assert_eq!(ranked[0].graph_score, 7.0); // 6 distance + 1 same dir
    assert_eq!(ranked[1].graph_score, 2.0); // 6 - 4 vendor penalty
}

// --- ranking.rs: LexicalRanker ---

#[test]
fn lexical_ranker_matches_camel_case_query() {
    let path = PathBuf::from("/proj/src/auth_service.rs");
    let roots = vec![make_lang_object(0, "AuthService", "auth::AuthService", path.clone())];
    let query = make_query(roots, "AuthService", true);

    let auth = make_candidate(1, "AuthService", "auth::AuthService", path.clone(), 0, None);
    let unrelated = make_candidate(2, "DbPool", "db::DbPool", PathBuf::from("/proj/src/db_pool.rs"), 1, None);

    let ranked = LexicalRanker.rank(&query, vec![unrelated, auth]);
    assert_eq!(ranked[0].node.name, "AuthService");
    assert!(ranked[0].lexical_score > ranked[1].lexical_score);
}

#[test]
fn lexical_ranker_subsequence_matching() {
    let path = PathBuf::from("/proj/src/authenticate.rs");
    let roots = vec![make_lang_object(0, "root", "root", path.clone())];
    let query = make_query(roots, "auth", true);

    let subseq_match = make_candidate(1, "authenticate", "auth::authenticate", path.clone(), 1, None);
    let no_match = make_candidate(2, "router", "router::router", PathBuf::from("/proj/src/router.rs"), 1, None);

    let ranked = LexicalRanker.rank(&query, vec![no_match, subseq_match]);
    assert_eq!(ranked[0].node.name, "authenticate");
    assert!(ranked[0].lexical_score > 0.0);
    assert_eq!(ranked[1].lexical_score, 0.0);
}

#[test]
fn lexical_ranker_empty_string_query_prefix_matches_all_terms() {
    // tokenize("") yields [""], and every term starts_with("") so lexical scoring applies.
    let path = PathBuf::from("/proj/src/foo.rs");
    let roots = vec![make_lang_object(0, "foo", "foo", path.clone())];
    let query = make_query(roots, "", true);
    let cand = make_candidate(1, "foo", "foo", path, 0, None);

    let ranked = LexicalRanker.rank(&query, vec![cand]);
    assert!(ranked[0].lexical_score > 0.0);
    assert_eq!(ranked[0].combined_score, ranked[0].lexical_score);
}

// --- ranking.rs: HybridRanker ---

#[test]
fn hybrid_ranker_combines_graph_and_lexical_scores() {
    let auth_path = PathBuf::from("/proj/src/auth_service.rs");
    let other_path = PathBuf::from("/proj/src/db_pool.rs");
    let roots = vec![make_lang_object(0, "AuthService", "auth::AuthService", auth_path.clone())];
    let query = make_query(roots, "AuthService", true);

    let auth_neighbor = make_candidate(1, "login", "login_handler::login", auth_path.clone(), 1, None);
    let distant = make_candidate(2, "db_pool", "db_pool::db_pool", other_path, 2, None);

    let hybrid = HybridRanker {
        graph_weight: 0.6,
        lexical_weight: 0.4,
    };
    let ranked = hybrid.rank(&query, vec![distant, auth_neighbor]);

    assert!(ranked[0].combined_score > ranked[1].combined_score);
    assert!(ranked[0].graph_score > 0.0);
    assert!(ranked[0].lexical_score >= 0.0);
    let expected = 0.6 * ranked[0].graph_score + 0.4 * ranked[0].lexical_score;
    assert!((ranked[0].combined_score - expected).abs() < 0.01);
}

#[test]
fn hybrid_ranker_combined_score_formula() {
    let path = PathBuf::from("/proj/src/foo.rs");
    let roots = vec![make_lang_object(0, "foo", "foo", path.clone())];
    let query = make_query(roots, "foo", true);

    let near = make_candidate(1, "near", "near", path.clone(), 0, None);
    let far = make_candidate(2, "far", "far", path, 3, None);

    let hybrid = HybridRanker {
        graph_weight: 0.7,
        lexical_weight: 0.3,
    };
    let ranked = hybrid.rank(&query, vec![far, near]);

    let expected = 0.7 * ranked[0].graph_score + 0.3 * ranked[0].lexical_score;
    assert!((ranked[0].combined_score - expected).abs() < 0.01);
    assert!(ranked[0].graph_score > ranked[1].graph_score);
}

// --- roots.rs: resolve_roots ---

fn setup_roots_db(dir: &std::path::Path) -> rusqlite::Connection {
    let mut conn = open_db(dir).unwrap();
    init_schema(&conn).unwrap();

    let mut index = CodeIndex {
        root: dir.to_path_buf(),
        files: vec![
            FileSnapshot {
                file_id: Some(FileId(1)),
                rel_path: PathBuf::from("auth_service.rs"),
                abs_path: dir.join("auth_service.rs"),
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
                rel_path: PathBuf::from("vendor/generated.rs"),
                abs_path: dir.join("vendor/generated.rs"),
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
                rel_path: PathBuf::from("tests/noisy_test.rs"),
                abs_path: dir.join("tests/noisy_test.rs"),
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
                id: Some(SymbolId(1)),
                file_id: Some(FileId(1)),
                name: "AuthService".to_string(),
                qualified_name: "auth::AuthService".to_string(),
                kind: SymbolKind::Struct,
                language: Language::rust(),
                file: dir.join("auth_service.rs"),
                range: TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 5,
                    end_col: 1,
                },
                body_range: None,
            },
            Symbol {
                id: Some(SymbolId(2)),
                file_id: Some(FileId(1)),
                name: "authenticate".to_string(),
                qualified_name: "auth::AuthService::authenticate".to_string(),
                kind: SymbolKind::Method,
                language: Language::rust(),
                file: dir.join("auth_service.rs"),
                range: TextRange {
                    start_line: 6,
                    start_col: 1,
                    end_line: 10,
                    end_col: 1,
                },
                body_range: None,
            },
            Symbol {
                id: Some(SymbolId(3)),
                file_id: Some(FileId(2)),
                name: "VendorHelper".to_string(),
                qualified_name: "vendor::VendorHelper".to_string(),
                kind: SymbolKind::Function,
                language: Language::rust(),
                file: dir.join("vendor/generated.rs"),
                range: TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 5,
                    end_col: 1,
                },
                body_range: None,
            },
            Symbol {
                id: Some(SymbolId(4)),
                file_id: Some(FileId(3)),
                name: "noisy_test".to_string(),
                qualified_name: "tests::noisy_test".to_string(),
                kind: SymbolKind::Test,
                language: Language::rust(),
                file: dir.join("tests/noisy_test.rs"),
                range: TextRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 5,
                    end_col: 1,
                },
                body_range: None,
            },
        ],
        occurrences: vec![],
        call_sites: vec![],
        edges: vec![],
    };

    save_index(&mut conn, &mut index).unwrap();
    conn
}

#[test]
fn resolve_roots_exact_qualified_name_match() {
    let dir = tempfile::tempdir().unwrap();
    let conn = setup_roots_db(dir.path());

    let roots = resolve_roots(&conn, "auth::AuthService", 3).unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].qualified_name, "auth::AuthService");
}

#[test]
fn resolve_roots_file_stem_match() {
    let dir = tempfile::tempdir().unwrap();
    let conn = setup_roots_db(dir.path());

    let roots = resolve_roots(&conn, "auth_service", 3).unwrap();
    assert!(!roots.is_empty());
    assert!(
        roots
            .iter()
            .any(|r| r.file_path.file_stem().unwrap() == "auth_service")
    );
}

#[test]
fn resolve_roots_prefers_non_vendor_over_vendor() {
    let dir = tempfile::tempdir().unwrap();
    let conn = setup_roots_db(dir.path());

    // Both auth::AuthService and vendor::VendorHelper token-match "auth" via terms;
    // exact/partial matches should rank AuthService above vendor path.
    let roots = resolve_roots(&conn, "auth", 3).unwrap();
    assert!(!roots.is_empty());
    assert_eq!(roots[0].name, "AuthService");
}

#[test]
fn resolve_roots_token_term_matching_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let conn = setup_roots_db(dir.path());

    let roots = resolve_roots(&conn, "authenticate", 3).unwrap();
    assert!(!roots.is_empty());
    assert!(
        roots
            .iter()
            .any(|r| r.qualified_name.contains("authenticate"))
    );
}

#[test]
fn resolve_roots_no_match_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let conn = setup_roots_db(dir.path());

    let roots = resolve_roots(&conn, "totally_unknown_symbol_xyz", 3).unwrap();
    assert!(roots.is_empty());
}