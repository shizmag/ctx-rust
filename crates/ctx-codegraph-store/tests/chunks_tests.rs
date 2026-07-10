use ctx_codegraph_chunk::ChunkId;
use ctx_codegraph_store::storage::{
    build_search_indexes, load_chunk, load_chunks_by_ids, load_chunks_for_symbol,
};
use ctx_config::Config;

mod common;
use common::{indexed_db, lexical_search_options, setup_mini_rust_project};

#[test]
fn test_chunk_load_paths_after_search_build() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    setup_mini_rust_project(root);

    let (conn, index, _registry) = indexed_db(root, common::no_search_options());
    let report = build_search_indexes(
        &conn,
        root,
        &lexical_search_options(),
        &Config::default(),
    )
    .unwrap();
    assert!(report.chunks_written > 0);

    let mut all_chunk_ids = Vec::new();
    for symbol in &index.symbols {
        if let Some(symbol_id) = symbol.id {
            let chunks = load_chunks_for_symbol(&conn, symbol_id).unwrap();
            if !chunks.is_empty() {
                all_chunk_ids.extend(chunks.iter().filter_map(|c| c.id));
            }
        }
    }
    assert!(!all_chunk_ids.is_empty());

    let loaded_batch = load_chunks_by_ids(&conn, &all_chunk_ids).unwrap();
    assert_eq!(loaded_batch.len(), all_chunk_ids.len());

    for chunk in &loaded_batch {
        assert!(chunk.start_line <= chunk.end_line);
        assert!(!chunk.qualified_name.is_empty());
        assert!(!chunk.text_hash.is_empty());
    }

    let with_missing = {
        let mut ids = all_chunk_ids.clone();
        ids.push(ChunkId(9_999_999));
        ids
    };
    let partial = load_chunks_by_ids(&conn, &with_missing).unwrap();
    assert_eq!(partial.len(), all_chunk_ids.len());

    let first_id = all_chunk_ids[0];
    let single = load_chunk(&conn, first_id).unwrap().expect("chunk should exist");
    assert_eq!(single.id, Some(first_id));
}