use ctx_codegraph_chunk::ChunkId;
use ctx_codegraph_lang::model::SymbolId;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct ChunkHit {
    pub chunk_id: ChunkId,
    pub symbol_id: Option<SymbolId>,
    pub score: f32,
}

pub fn rrf_fuse(lists: &[Vec<ChunkHit>], k: usize) -> Vec<ChunkHit> {
    let k = k.max(1);
    let mut scores: HashMap<ChunkId, (f32, Option<SymbolId>)> = HashMap::new();

    for list in lists {
        for (rank, hit) in list.iter().enumerate() {
            let contribution = 1.0 / (k as f32 + rank as f32 + 1.0);
            let entry = scores.entry(hit.chunk_id).or_insert((0.0, hit.symbol_id));
            entry.0 += contribution;
            if entry.1.is_none() {
                entry.1 = hit.symbol_id;
            }
        }
    }

    let mut fused: Vec<ChunkHit> = scores
        .into_iter()
        .map(|(chunk_id, (score, symbol_id))| ChunkHit {
            chunk_id,
            symbol_id,
            score,
        })
        .collect();
    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.chunk_id.0.cmp(&b.chunk_id.0))
    });
    fused
}