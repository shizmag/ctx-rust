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

#[test]
fn rrf_fuse_empty_lists_returns_empty() {
    let fused = rrf_fuse(&[], 60);
    assert!(fused.is_empty());

    let fused_empty_inner = rrf_fuse(&[vec![], vec![]], 60);
    assert!(fused_empty_inner.is_empty());
}

#[test]
fn rrf_fuse_zero_k_uses_minimum_one() {
    let list = vec![ChunkHit {
        chunk_id: ChunkId(1),
        symbol_id: Some(SymbolId(1)),
        score: 0.0,
    }];

    let fused = rrf_fuse(&[list], 0);
    assert_eq!(fused.len(), 1);
    assert!((fused[0].score - 0.5).abs() < f32::EPSILON);
}

#[test]
fn rrf_fuse_backfills_symbol_id_when_first_hit_lacks_one() {
    let first = vec![ChunkHit {
        chunk_id: ChunkId(5),
        symbol_id: None,
        score: 0.0,
    }];
    let second = vec![ChunkHit {
        chunk_id: ChunkId(5),
        symbol_id: Some(SymbolId(55)),
        score: 0.0,
    }];

    let fused = rrf_fuse(&[first, second], 60);
    assert_eq!(fused.len(), 1);
    assert_eq!(fused[0].symbol_id, Some(SymbolId(55)));
}

#[test]
fn rrf_fuse_tie_breaks_by_chunk_id_when_scores_equal() {
    let list_a = vec![ChunkHit {
        chunk_id: ChunkId(2),
        symbol_id: Some(SymbolId(2)),
        score: 0.0,
    }];
    let list_b = vec![ChunkHit {
        chunk_id: ChunkId(1),
        symbol_id: Some(SymbolId(1)),
        score: 0.0,
    }];

    let fused = rrf_fuse(&[list_a, list_b], 60);
    assert_eq!(fused.len(), 2);
    assert!((fused[0].score - fused[1].score).abs() < f32::EPSILON);
    assert_eq!(fused[0].chunk_id, ChunkId(1));
    assert_eq!(fused[1].chunk_id, ChunkId(2));
}