use ctx_codegraph_chunk::ChunkBuilder;
use ctx_codegraph_chunk::model::ChunkKind;
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