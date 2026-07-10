use ctx_codegraph_chunk::ChunkBuilder;
use ctx_codegraph_chunk::model::ChunkKind;
use ctx_codegraph_chunk::{extract_lines_from_file, truncate_large_body};
use ctx_codegraph_lang::model::{
    FileId, LanguageId, Symbol, SymbolId, SymbolKind, TextRange,
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
        Symbol {
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
        Symbol {
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
        Symbol {
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