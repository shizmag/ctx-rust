use ctx_codegraph_chunk::ChunkId;
use ctx_codegraph_lang::model::SymbolId;
use ctx_codegraph_search::{ChunkHit, rrf_fuse};

#[test]
fn rrf_fusion_combines_ranked_lists() {
    let lexical = vec![
        ChunkHit {
            chunk_id: ChunkId(1),
            symbol_id: Some(SymbolId(10)),
            score: 0.9,
        },
        ChunkHit {
            chunk_id: ChunkId(2),
            symbol_id: Some(SymbolId(20)),
            score: 0.8,
        },
    ];
    let dense = vec![
        ChunkHit {
            chunk_id: ChunkId(2),
            symbol_id: Some(SymbolId(20)),
            score: 0.95,
        },
        ChunkHit {
            chunk_id: ChunkId(3),
            symbol_id: Some(SymbolId(30)),
            score: 0.7,
        },
    ];

    let fused = rrf_fuse(&[lexical, dense], 60);
    assert_eq!(fused.len(), 3);
    assert_eq!(fused[0].chunk_id, ChunkId(2));
    assert!(fused[0].score > fused[1].score);
    assert_eq!(fused[0].symbol_id, Some(SymbolId(20)));

    let only_first = rrf_fuse(&[vec![fused[0].clone()]], 60);
    assert_eq!(only_first.len(), 1);
    assert!(only_first[0].score > 0.0);
}