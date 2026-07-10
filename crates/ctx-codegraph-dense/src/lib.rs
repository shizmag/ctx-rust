use ctx_codegraph_chunk::ChunkId;
use ctx_codegraph_models::EMBEDDING_DIM;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

pub use ctx_codegraph_models::EMBEDDING_DIM as DENSE_EMBEDDING_DIM;

#[derive(Debug, thiserror::Error)]
pub enum DenseError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("dense index error: {0}")]
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DenseHit {
    pub chunk_id: ChunkId,
    pub score: f32,
}

#[derive(Debug, Clone)]
pub struct EmbeddingRecord {
    pub chunk_id: ChunkId,
    pub embedding: Vec<f32>,
}

pub struct DenseIndex {
    db_path: PathBuf,
    conn: Connection,
}

impl DenseIndex {
    pub fn open(workspace: &Path) -> Result<Self, DenseError> {
        let db_dir = workspace.join(".ctx-codegraph");
        std::fs::create_dir_all(&db_dir)?;
        let db_path = db_dir.join("dense.sqlite");
        let conn = Connection::open(&db_path)?;
        init_schema(&conn)?;
        Ok(Self { db_path, conn })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn upsert_batch(&mut self, records: &[EmbeddingRecord]) -> Result<(), DenseError> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO chunk_embeddings (chunk_id, embedding)
                 VALUES (?1, ?2)
                 ON CONFLICT(chunk_id) DO UPDATE SET embedding = excluded.embedding",
            )?;

            for record in records {
                if record.embedding.len() != EMBEDDING_DIM {
                    return Err(DenseError::Other(format!(
                        "embedding for chunk {} has dim {}, expected {}",
                        record.chunk_id.0,
                        record.embedding.len(),
                        EMBEDDING_DIM
                    )));
                }
                let blob = embedding_to_blob(&record.embedding);
                stmt.execute(params![record.chunk_id.0, blob])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn search_knn(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<DenseHit>, DenseError> {
        if query_embedding.len() != EMBEDDING_DIM {
            return Err(DenseError::Other(format!(
                "query embedding has dim {}, expected {}",
                query_embedding.len(),
                EMBEDDING_DIM
            )));
        }

        let mut stmt = self.conn.prepare("SELECT chunk_id, embedding FROM chunk_embeddings")?;
        let rows = stmt.query_map([], |row| {
            let chunk_id: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((chunk_id, blob))
        })?;

        let mut hits = Vec::new();
        for row in rows {
            let (chunk_id, blob) = row?;
            let embedding = blob_to_embedding(&blob)?;
            let score = cosine_similarity(query_embedding, &embedding);
            hits.push(DenseHit {
                chunk_id: ChunkId(chunk_id),
                score,
            });
        }

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.chunk_id.0.cmp(&b.chunk_id.0))
        });
        hits.truncate(limit.max(1));
        Ok(hits)
    }

    pub fn remove_chunk_ids(&mut self, chunk_ids: &[ChunkId]) -> Result<(), DenseError> {
        if chunk_ids.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare("DELETE FROM chunk_embeddings WHERE chunk_id = ?1")?;
            for chunk_id in chunk_ids {
                stmt.execute(params![chunk_id.0])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

fn init_schema(conn: &Connection) -> Result<(), DenseError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS chunk_embeddings (
            chunk_id INTEGER PRIMARY KEY,
            embedding BLOB NOT NULL
        );",
    )?;
    Ok(())
}

fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    embedding
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn blob_to_embedding(blob: &[u8]) -> Result<Vec<f32>, DenseError> {
    if !blob.len().is_multiple_of(4) {
        return Err(DenseError::Other(format!(
            "invalid embedding blob length: {}",
            blob.len()
        )));
    }

    let mut embedding = Vec::with_capacity(blob.len() / 4);
    for chunk in blob.chunks_exact(4) {
        let bytes: [u8; 4] = chunk.try_into().expect("chunk size checked");
        embedding.push(f32::from_le_bytes(bytes));
    }
    Ok(embedding)
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let mut dot = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;

    for (left, right) in a.iter().zip(b.iter()) {
        dot += left * right;
        norm_a += left * left;
        norm_b += right * right;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom <= f32::EPSILON {
        0.0
    } else {
        dot / denom
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_embedding(seed: f32) -> Vec<f32> {
        (0..EMBEDDING_DIM)
            .map(|idx| ((idx as f32 + seed) % 17.0) / 17.0)
            .collect()
    }

    #[test]
    fn dense_index_open_creates_db() {
        let dir = tempdir().unwrap();
        let index = DenseIndex::open(dir.path()).unwrap();
        assert!(index.db_path().exists());
        assert!(index.db_path().ends_with("dense.sqlite"));
    }

    #[test]
    fn upsert_and_search_knn() {
        let dir = tempdir().unwrap();
        let mut index = DenseIndex::open(dir.path()).unwrap();

        index
            .upsert_batch(&[
                EmbeddingRecord {
                    chunk_id: ChunkId(1),
                    embedding: sample_embedding(1.0),
                },
                EmbeddingRecord {
                    chunk_id: ChunkId(2),
                    embedding: sample_embedding(2.0),
                },
            ])
            .unwrap();

        let query = sample_embedding(1.0);
        let hits = index.search_knn(&query, 2).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].chunk_id, ChunkId(1));
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn upsert_rejects_wrong_embedding_dimension() {
        let dir = tempdir().unwrap();
        let mut index = DenseIndex::open(dir.path()).unwrap();
        let err = index
            .upsert_batch(&[EmbeddingRecord {
                chunk_id: ChunkId(1),
                embedding: vec![0.1, 0.2, 0.3],
            }])
            .unwrap_err()
            .to_string();
        assert!(err.contains("expected"));
        assert!(err.contains(&EMBEDDING_DIM.to_string()));
    }

    #[test]
    fn search_knn_rejects_wrong_query_dimension() {
        let dir = tempdir().unwrap();
        let index = DenseIndex::open(dir.path()).unwrap();
        let err = index
            .search_knn(&[0.1, 0.2], 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("query embedding has dim"));
    }

    #[test]
    fn search_knn_respects_limit() {
        let dir = tempdir().unwrap();
        let mut index = DenseIndex::open(dir.path()).unwrap();
        index
            .upsert_batch(&[
                EmbeddingRecord {
                    chunk_id: ChunkId(1),
                    embedding: sample_embedding(1.0),
                },
                EmbeddingRecord {
                    chunk_id: ChunkId(2),
                    embedding: sample_embedding(2.0),
                },
                EmbeddingRecord {
                    chunk_id: ChunkId(3),
                    embedding: sample_embedding(3.0),
                },
            ])
            .unwrap();

        let hits = index.search_knn(&sample_embedding(2.0), 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk_id, ChunkId(2));
    }

    #[test]
    fn remove_chunk_ids_deletes_embeddings() {
        let dir = tempdir().unwrap();
        let mut index = DenseIndex::open(dir.path()).unwrap();
        index
            .upsert_batch(&[
                EmbeddingRecord {
                    chunk_id: ChunkId(1),
                    embedding: sample_embedding(1.0),
                },
                EmbeddingRecord {
                    chunk_id: ChunkId(2),
                    embedding: sample_embedding(2.0),
                },
            ])
            .unwrap();

        index.remove_chunk_ids(&[ChunkId(1)]).unwrap();
        let hits = index.search_knn(&sample_embedding(1.0), 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk_id, ChunkId(2));
    }

    #[test]
    fn remove_empty_chunk_ids_is_noop() {
        let dir = tempdir().unwrap();
        let mut index = DenseIndex::open(dir.path()).unwrap();
        index
            .upsert_batch(&[EmbeddingRecord {
                chunk_id: ChunkId(1),
                embedding: sample_embedding(1.0),
            }])
            .unwrap();
        index.remove_chunk_ids(&[]).unwrap();
        let hits = index.search_knn(&sample_embedding(1.0), 1).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn upsert_overwrites_existing_chunk() {
        let dir = tempdir().unwrap();
        let mut index = DenseIndex::open(dir.path()).unwrap();
        index
            .upsert_batch(&[EmbeddingRecord {
                chunk_id: ChunkId(1),
                embedding: sample_embedding(1.0),
            }])
            .unwrap();
        index
            .upsert_batch(&[EmbeddingRecord {
                chunk_id: ChunkId(1),
                embedding: sample_embedding(99.0),
            }])
            .unwrap();

        let hits = index.search_knn(&sample_embedding(99.0), 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk_id, ChunkId(1));
        assert!(hits[0].score > 0.99);
    }
}