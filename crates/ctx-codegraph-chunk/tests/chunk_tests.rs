use ctx_codegraph_chunk::ChunkBuilder;
use ctx_codegraph_chunk::model::ChunkKind;
use ctx_codegraph_chunk::{extract_lines_from_file, truncate_large_body};
use ctx_codegraph_lang::backend::BackendId;
use ctx_codegraph_lang::model::{
    FileId, LanguageId, Occurrence, OccurrenceKind, Symbol, SymbolId, SymbolKind, TextRange,
};
use std::collections::HashMap;
use std::io::Write;
#[test]
fn parent_child_chunks_for_nested_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested.rs");
    let mut file = std::fs::File::create(&path).unwrap();
    writeln!(
        file,
        "impl Foo {{
    fn bar() {{
        1
    }}
    fn baz() {{
        2
    }}
}}"
    )
    .unwrap();

    let symbols = vec![
        Symbol { nesting_depth: 0, lines_of_code: 0, complexity_proxy: 0, param_count: 0, parent_symbol_id: None, fan_in: 0, fan_out: 0, coupling: 0.0, cohesion: 0.0,
            id: Some(SymbolId(0)),
            file_id: Some(FileId(1)),
            name: "Foo".to_string(),
            qualified_name: "nested::Foo".to_string(),
            kind: SymbolKind::Impl,
            language: LanguageId::rust(),
            file: path.clone(),
            range: TextRange {
                start_line: 1,
                start_col: 1,
                end_line: 8,
                end_col: 2,
            },
            body_range: Some(TextRange {
                start_line: 1,
                start_col: 12,
                end_line: 8,
                end_col: 2,
            }),
        },
        Symbol { nesting_depth: 0, lines_of_code: 0, complexity_proxy: 0, param_count: 0, parent_symbol_id: None, fan_in: 0, fan_out: 0, coupling: 0.0, cohesion: 0.0,
            id: Some(SymbolId(1)),
            file_id: Some(FileId(1)),
            name: "bar".to_string(),
            qualified_name: "nested::Foo::bar".to_string(),
            kind: SymbolKind::Method,
            language: LanguageId::rust(),
            file: path.clone(),
            range: TextRange {
                start_line: 2,
                start_col: 5,
                end_line: 4,
                end_col: 6,
            },
            body_range: Some(TextRange {
                start_line: 2,
                start_col: 12,
                end_line: 4,
                end_col: 6,
            }),
        },
        Symbol { nesting_depth: 0, lines_of_code: 0, complexity_proxy: 0, param_count: 0, parent_symbol_id: None, fan_in: 0, fan_out: 0, coupling: 0.0, cohesion: 0.0,
            id: Some(SymbolId(2)),
            file_id: Some(FileId(1)),
            name: "baz".to_string(),
            qualified_name: "nested::Foo::baz".to_string(),
            kind: SymbolKind::Method,
            language: LanguageId::rust(),
            file: path.clone(),
            range: TextRange {
                start_line: 5,
                start_col: 5,
                end_line: 7,
                end_col: 6,
            },
            body_range: Some(TextRange {
                start_line: 5,
                start_col: 12,
                end_line: 7,
                end_col: 6,
            }),
        },
    ];

    let mut contains_parent = HashMap::new();
    contains_parent.insert(SymbolId(1), SymbolId(0));
    contains_parent.insert(SymbolId(2), SymbolId(0));

    let mut builder = ChunkBuilder::new(FileId(1), &path).include_text(true);
    let chunks = builder
        .build(&symbols, &contains_parent, &[])
        .expect("chunk build");

    let parent_summary = chunks
        .iter()
        .find(|c| c.kind == ChunkKind::ParentSummary && c.symbol_id == Some(SymbolId(0)))
        .expect("impl parent summary");
    assert_eq!(parent_summary.parent_chunk_id, None);

    let child_bodies: Vec<_> = chunks
        .iter()
        .filter(|c| {
            c.kind == ChunkKind::SymbolBody
                && matches!(c.symbol_id, Some(SymbolId(1) | SymbolId(2)))
        })
        .collect();
    assert_eq!(child_bodies.len(), 2);
    for child in child_bodies {
        assert_eq!(
            child.parent_chunk_id,
            Some(parent_summary.id.expect("chunk id"))
        );
        assert!(!child.text_hash.is_empty());
        assert!(child.token_count > 0);
    }

    let impl_chunks: Vec<_> = chunks
        .iter()
        .filter(|c| c.symbol_id == Some(SymbolId(0)))
        .collect();
    assert!(impl_chunks.iter().any(|c| c.kind == ChunkKind::SymbolDecl));
    assert!(impl_chunks.iter().any(|c| c.kind == ChunkKind::SymbolBody));
    assert!(impl_chunks.iter().any(|c| c.kind == ChunkKind::ParentSummary));
}

#[test]
fn extract_lines_from_file_empty_file_returns_empty_string() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.rs");
    std::fs::write(&path, "").unwrap();

    let result = extract_lines_from_file(&path, 1, 1, 0).unwrap();
    assert_eq!(result, "");
}

#[test]
fn extract_lines_from_file_normal_range() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.rs");
    std::fs::write(&path, "line1\nline2\nline3\nline4\nline5\n").unwrap();

    let result = extract_lines_from_file(&path, 2, 3, 0).unwrap();
    assert_eq!(result, "line2\nline3\n");
}

#[test]
fn extract_lines_from_file_applies_context_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.rs");
    std::fs::write(&path, "line1\nline2\nline3\nline4\nline5\n").unwrap();

    let result = extract_lines_from_file(&path, 3, 3, 1).unwrap();
    assert_eq!(result, "line2\nline3\nline4\n");
}

#[test]
fn extract_lines_from_file_start_greater_than_end_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.rs");
    std::fs::write(&path, "line1\nline2\nline3\n").unwrap();

    let result = extract_lines_from_file(&path, 5, 2, 0).unwrap();
    assert_eq!(result, "");
}

#[test]
fn extract_lines_from_file_missing_file_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.rs");

    let err = extract_lines_from_file(&path, 1, 1, 0).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn truncate_large_body_small_body_returns_full_snippet() {
    let lines = [
        "line1", "line2", "line3", "line4", "line5", "line6", "line7", "line8", "line9", "line10",
    ];

    let snippet = truncate_large_body(&lines, 2, 4, 1, 10, 0);
    assert_eq!(snippet, "line2\nline3\nline4\n");
    assert!(!snippet.contains("omitted"));
}

#[test]
fn truncate_large_body_large_body_includes_omission_marker() {
    let lines: Vec<String> = (1..=120).map(|i| format!("line{i}")).collect();
    let lines_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();

    let snippet = truncate_large_body(&lines_refs, 1, 120, 1, 120, 0);

    assert!(snippet.starts_with("line1\n"));
    assert!(snippet.contains("// ... 89 lines omitted ...\n"));
    assert!(snippet.contains("line120\n"));
    assert!(!snippet.contains("line50\n"));
}

#[test]
fn truncate_large_body_invalid_range_returns_empty() {
    let lines = vec!["a", "b", "c"];

    let snippet = truncate_large_body(&lines, 5, 2, 1, 1, 0);
    assert_eq!(snippet, "");
}

fn sample_symbol_without_body(
    id: SymbolId,
    path: &std::path::Path,
    qualified_name: &str,
    start_line: usize,
    end_line: usize,
) -> Symbol {
    Symbol { nesting_depth: 0, lines_of_code: 0, complexity_proxy: 0, param_count: 0, parent_symbol_id: None, fan_in: 0, fan_out: 0, coupling: 0.0, cohesion: 0.0,
        id: Some(id),
        file_id: Some(FileId(1)),
        name: qualified_name.to_string(),
        qualified_name: qualified_name.to_string(),
        kind: SymbolKind::Function,
        language: LanguageId::rust(),
        file: path.to_path_buf(),
        range: TextRange {
            start_line,
            start_col: 1,
            end_line,
            end_col: 1,
        },
        body_range: None,
    }
}

fn sample_occurrence(
    path: &std::path::Path,
    enclosing_symbol: Option<SymbolId>,
    raw_text: &str,
    start_line: usize,
    end_line: usize,
) -> Occurrence {
    Occurrence {
        id: None,
        file_id: Some(FileId(1)),
        enclosing_symbol,
        enclosing_temp_index: None,
        kind: OccurrenceKind::Call,
        raw_text: raw_text.to_string(),
        file: path.to_path_buf(),
        range: TextRange {
            start_line,
            start_col: 1,
            end_line,
            end_col: 1,
        },
        language: LanguageId::rust(),
        backend_id: BackendId::new("test-backend"),
    }
}

#[test]
fn builder_without_include_text_produces_metadata_only_chunks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plain.rs");
    std::fs::write(&path, "const ANSWER: i32 = 42;\n").unwrap();

    let symbols = vec![sample_symbol_without_body(
        SymbolId(1),
        &path,
        "plain::ANSWER",
        1,
        1,
    )];

    let mut builder = ChunkBuilder::new(FileId(1), &path);
    let chunks = builder.build(&symbols, &HashMap::new(), &[]).expect("build");

    assert_eq!(chunks.len(), 2);
    for chunk in &chunks {
        assert!(chunk.text.is_none());
        assert_eq!(chunk.token_count, 0);
        assert!(!chunk.text_hash.is_empty());
    }
    assert!(chunks.iter().any(|c| c.kind == ChunkKind::SymbolDecl));
    assert!(chunks.iter().any(|c| c.kind == ChunkKind::SymbolBody));
}

#[test]
fn builder_skips_symbols_without_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("skip.rs");
    std::fs::write(&path, "fn orphan() {}\n").unwrap();

    let symbols = vec![Symbol { nesting_depth: 0, lines_of_code: 0, complexity_proxy: 0, param_count: 0, parent_symbol_id: None, fan_in: 0, fan_out: 0, coupling: 0.0, cohesion: 0.0,
        id: None,
        file_id: Some(FileId(1)),
        name: "orphan".to_string(),
        qualified_name: "skip::orphan".to_string(),
        kind: SymbolKind::Function,
        language: LanguageId::rust(),
        file: path.clone(),
        range: TextRange {
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 16,
        },
        body_range: None,
    }];

    let mut builder = ChunkBuilder::new(FileId(1), &path);
    let chunks = builder.build(&symbols, &HashMap::new(), &[]).expect("build");
    assert!(chunks.is_empty());
}

#[test]
fn builder_symbol_without_body_range_extracts_full_range() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("const.rs");
    std::fs::write(&path, "const VALUE: i32 = 7;\n").unwrap();

    let symbols = vec![sample_symbol_without_body(
        SymbolId(1),
        &path,
        "const::VALUE",
        1,
        1,
    )];

    let mut builder = ChunkBuilder::new(FileId(1), &path).include_text(true);
    let chunks = builder.build(&symbols, &HashMap::new(), &[]).expect("build");

    let body = chunks
        .iter()
        .find(|c| c.kind == ChunkKind::SymbolBody)
        .expect("body chunk");
    assert_eq!(body.start_line, 1);
    assert_eq!(body.end_line, 1);
    assert_eq!(body.text.as_deref(), Some("const VALUE: i32 = 7;\n"));
}

#[test]
fn builder_missing_file_with_include_text_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.rs");

    let symbols = vec![sample_symbol_without_body(
        SymbolId(1),
        &path,
        "missing::X",
        1,
        1,
    )];

    let mut builder = ChunkBuilder::new(FileId(1), &path).include_text(true);
    let err = builder.build(&symbols, &HashMap::new(), &[]).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn builder_context_lines_affects_extracted_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("context.rs");
    std::fs::write(
        &path,
        "line1\nline2\nline3\nline4\nline5\n",
    )
    .unwrap();

    let symbols = vec![Symbol { nesting_depth: 0, lines_of_code: 0, complexity_proxy: 0, param_count: 0, parent_symbol_id: None, fan_in: 0, fan_out: 0, coupling: 0.0, cohesion: 0.0,
        id: Some(SymbolId(1)),
        file_id: Some(FileId(1)),
        name: "middle".to_string(),
        qualified_name: "context::middle".to_string(),
        kind: SymbolKind::Function,
        language: LanguageId::rust(),
        file: path.clone(),
        range: TextRange {
            start_line: 3,
            start_col: 1,
            end_line: 3,
            end_col: 7,
        },
        body_range: Some(TextRange {
            start_line: 3,
            start_col: 8,
            end_line: 3,
            end_col: 12,
        }),
    }];

    let mut builder = ChunkBuilder::new(FileId(1), &path)
        .include_text(true)
        .context_lines(1);
    let chunks = builder.build(&symbols, &HashMap::new(), &[]).expect("build");

    let decl = chunks
        .iter()
        .find(|c| c.kind == ChunkKind::SymbolDecl)
        .expect("decl chunk");
    assert_eq!(decl.text.as_deref(), Some("line2\nline3\nline4\n"));
}

#[test]
fn occurrence_chunks_use_enclosing_symbol_and_raw_text_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("occ.rs");
    std::fs::write(
        &path,
        "fn helper() {\n    callee();\n}\n",
    )
    .unwrap();

    let symbols = vec![Symbol { nesting_depth: 0, lines_of_code: 0, complexity_proxy: 0, param_count: 0, parent_symbol_id: None, fan_in: 0, fan_out: 0, coupling: 0.0, cohesion: 0.0,
        id: Some(SymbolId(1)),
        file_id: Some(FileId(1)),
        name: "helper".to_string(),
        qualified_name: "occ::helper".to_string(),
        kind: SymbolKind::Function,
        language: LanguageId::rust(),
        file: path.clone(),
        range: TextRange {
            start_line: 1,
            start_col: 1,
            end_line: 3,
            end_col: 2,
        },
        body_range: Some(TextRange {
            start_line: 1,
            start_col: 12,
            end_line: 3,
            end_col: 2,
        }),
    }];

    let occurrences = vec![
        sample_occurrence(&path, Some(SymbolId(1)), "callee", 2, 2),
        sample_occurrence(&path, None, "orphan_call", 2, 2),
    ];

    let mut builder = ChunkBuilder::new(FileId(1), &path)
        .include_text(true)
        .context_lines(0);
    let chunks = builder
        .build(&symbols, &HashMap::new(), &occurrences)
        .expect("build");

    let occ_chunks: Vec<_> = chunks
        .iter()
        .filter(|c| c.kind == ChunkKind::Occurrence)
        .collect();
    assert_eq!(occ_chunks.len(), 2);

    let enclosed = occ_chunks
        .iter()
        .find(|c| c.symbol_id == Some(SymbolId(1)))
        .expect("enclosed occurrence");
    assert_eq!(enclosed.qualified_name, "occ::helper");
    assert_eq!(enclosed.text.as_deref(), Some("    callee();\n"));

    let orphan = occ_chunks
        .iter()
        .find(|c| c.symbol_id.is_none())
        .expect("orphan occurrence");
    assert_eq!(orphan.qualified_name, "orphan_call");
    assert_eq!(orphan.parent_chunk_id, None);
}

#[test]
fn occurrence_chunks_without_include_text_have_no_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("occ_meta.rs");
    std::fs::write(&path, "fn f() { g(); }\n").unwrap();

    let occurrences = vec![sample_occurrence(&path, None, "g", 1, 1)];

    let mut builder = ChunkBuilder::new(FileId(1), &path);
    let chunks = builder
        .build(&[], &HashMap::new(), &occurrences)
        .expect("build");

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].kind, ChunkKind::Occurrence);
    assert!(chunks[0].text.is_none());
    assert_eq!(chunks[0].token_count, 0);
}

#[test]
fn parent_summary_without_include_text_has_empty_hash_only_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parent.rs");
    std::fs::write(
        &path,
        "mod parent {\n    fn child() {}\n}\n",
    )
    .unwrap();

    let symbols = vec![
        Symbol { nesting_depth: 0, lines_of_code: 0, complexity_proxy: 0, param_count: 0, parent_symbol_id: None, fan_in: 0, fan_out: 0, coupling: 0.0, cohesion: 0.0,
            id: Some(SymbolId(0)),
            file_id: Some(FileId(1)),
            name: "parent".to_string(),
            qualified_name: "parent::parent".to_string(),
            kind: SymbolKind::Module,
            language: LanguageId::rust(),
            file: path.clone(),
            range: TextRange {
                start_line: 1,
                start_col: 1,
                end_line: 3,
                end_col: 2,
            },
            body_range: Some(TextRange {
                start_line: 1,
                start_col: 12,
                end_line: 3,
                end_col: 2,
            }),
        },
        Symbol { nesting_depth: 0, lines_of_code: 0, complexity_proxy: 0, param_count: 0, parent_symbol_id: None, fan_in: 0, fan_out: 0, coupling: 0.0, cohesion: 0.0,
            id: Some(SymbolId(1)),
            file_id: Some(FileId(1)),
            name: "child".to_string(),
            qualified_name: "parent::parent::child".to_string(),
            kind: SymbolKind::Function,
            language: LanguageId::rust(),
            file: path.clone(),
            range: TextRange {
                start_line: 2,
                start_col: 5,
                end_line: 2,
                end_col: 18,
            },
            body_range: Some(TextRange {
                start_line: 2,
                start_col: 12,
                end_line: 2,
                end_col: 18,
            }),
        },
    ];

    let mut contains_parent = HashMap::new();
    contains_parent.insert(SymbolId(1), SymbolId(0));

    let mut builder = ChunkBuilder::new(FileId(1), &path);
    let chunks = builder
        .build(&symbols, &contains_parent, &[])
        .expect("build");

    let parent_summary = chunks
        .iter()
        .find(|c| c.kind == ChunkKind::ParentSummary)
        .expect("parent summary");
    assert!(parent_summary.text.is_none());
    assert_eq!(parent_summary.token_count, 0);
}