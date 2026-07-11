use arrow::array::AsArray;
use arrow::datatypes::{DataType, Field, Float32Type, Int64Type, Schema};
use arrow_array::{
    FixedSizeListArray, Int64Array, RecordBatch as ArrowRecordBatch, RecordBatchIterator,
};
use ctx_codegraph_chunk::ChunkId;
use ctx_codegraph_models::EMBEDDING_DIM;
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Connection, DistanceType, Table};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::runtime::Runtime;

pub use ctx_codegraph_models::EMBEDDING_DIM as DENSE_EMBEDDING_DIM;

const TABLE_NAME: &str = "chunk_embeddings";
const VECTOR_COLUMN: &str = "vector";

#[derive(Debug, thiserror::Error)]
pub enum DenseError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("lancedb error: {0}")]
    LanceDb(#[from] lancedb::Error),

    #[error("arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

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
    connection: Connection,
    table: Table,
    runtime: Runtime,
}

impl DenseIndex {
    pub fn open(workspace: &Path) -> Result<Self, DenseError> {
        let db_path = dense_storage_path(workspace);
        std::fs::create_dir_all(&db_path)?;
        let db_uri = db_path
            .to_str()
            .ok_or_else(|| DenseError::Other("dense index path is not valid UTF-8".into()))?
            .to_string();

        let runtime = Runtime::new().map_err(|err| DenseError::Other(err.to_string()))?;
        let (connection, table) = runtime.block_on(async {
            let connection = connect(&db_uri).execute().await?;
            let table = ensure_table(&connection).await?;
            Ok::<_, DenseError>((connection, table))
        })?;

        Ok(Self {
            db_path,
            connection,
            table,
            runtime,
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn count(&self) -> Result<u64, DenseError> {
        self.block_on(async {
            let count = self.table.count_rows(None).await?;
            Ok(count as u64)
        })
    }

    pub fn upsert_batch(&mut self, records: &[EmbeddingRecord]) -> Result<(), DenseError> {
        if records.is_empty() {
            return Ok(());
        }

        for record in records {
            if record.embedding.len() != EMBEDDING_DIM {
                return Err(DenseError::Other(format!(
                    "embedding for chunk {} has dim {}, expected {}",
                    record.chunk_id.0,
                    record.embedding.len(),
                    EMBEDDING_DIM
                )));
            }
        }

        let batch = records_to_batch(records)?;
        let schema = batch.schema();
        let reader = RecordBatchIterator::new(std::iter::once(Ok(batch)), schema);

        self.block_on(async {
            let mut merge = self.table.merge_insert(&["chunk_id"]);
            merge.when_matched_update_all(None);
            merge.when_not_matched_insert_all();
            merge.execute(Box::new(reader)).await?;
            Ok(())
        })
    }

    pub fn add_batch(&mut self, records: &[EmbeddingRecord]) -> Result<(), DenseError> {
        if records.is_empty() {
            return Ok(());
        }

        for record in records {
            if record.embedding.len() != EMBEDDING_DIM {
                return Err(DenseError::Other(format!(
                    "embedding for chunk {} has dim {}, expected {}",
                    record.chunk_id.0,
                    record.embedding.len(),
                    EMBEDDING_DIM
                )));
            }
        }

        let batch = records_to_batch(records)?;
        let schema = batch.schema();
        let reader = RecordBatchIterator::new(std::iter::once(Ok(batch)), schema);
        let boxed_reader: Box<dyn arrow_array::RecordBatchReader + Send> = Box::new(reader);

        self.block_on(async {
            self.table.add(boxed_reader).execute().await?;
            Ok(())
        })
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

        let limit = limit.max(1);
        let query = query_embedding.to_vec();

        let mut hits = self.block_on(async {
            let batches = self.table
                .query()
                .nearest_to(query.as_slice())?
                .column(VECTOR_COLUMN)
                .distance_type(DistanceType::Cosine)
                .limit(limit)
                .execute()
                .await?
                .try_collect::<Vec<ArrowRecordBatch>>()
                .await?;
            hits_from_batches(&batches)
        })?;

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.chunk_id.0.cmp(&b.chunk_id.0))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    pub fn remove_chunk_ids(&mut self, chunk_ids: &[ChunkId]) -> Result<(), DenseError> {
        if chunk_ids.is_empty() {
            return Ok(());
        }

        let predicate = chunk_ids
            .iter()
            .map(|chunk_id| chunk_id.0.to_string())
            .collect::<Vec<_>>()
            .join(", ");

        self.block_on(async {
            let filter = format!("chunk_id IN ({predicate})");
            self.table.delete(filter.as_str()).await?;
            Ok(())
        })
    }

    pub fn clear(&mut self) -> Result<(), DenseError> {
        let new_table = self.block_on(async {
            let _ = self.connection.drop_table(TABLE_NAME, &[]).await;
            let table = ensure_table(&self.connection).await?;
            Ok(table)
        })?;
        self.table = new_table;
        Ok(())
    }

    fn block_on<F, T>(&self, future: F) -> Result<T, DenseError>
    where
        F: std::future::Future<Output = Result<T, DenseError>>,
    {
        self.runtime.block_on(future)
    }
}

/// Returns the LanceDB storage directory for dense embeddings.
pub fn dense_storage_path(workspace: &Path) -> PathBuf {
    workspace.join(".ctx-codegraph/dense")
}

/// Returns the legacy SQLite path for dense embeddings (pre-LanceDB migration).
pub fn legacy_dense_sqlite_path(workspace: &Path) -> PathBuf {
    workspace.join(".ctx-codegraph/dense.sqlite")
}

/// Read-only count of rows in the LanceDB dense index without creating storage.
pub fn peek_dense_embedding_count(workspace: &Path) -> u64 {
    let db_path = dense_storage_path(workspace);
    if !db_path.exists() {
        return 0;
    }

    let db_uri = match db_path.to_str() {
        Some(uri) => uri.to_string(),
        None => return 0,
    };

    let runtime = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(_) => return 0,
    };

    runtime.block_on(async {
        let connection = match connect(&db_uri).execute().await {
            Ok(connection) => connection,
            Err(_) => return 0,
        };

        let table = match connection.open_table(TABLE_NAME).execute().await {
            Ok(table) => table,
            Err(lancedb::Error::TableNotFound { .. }) => return 0,
            Err(_) => return 0,
        };

        match table.count_rows(None).await {
            Ok(count) => count as u64,
            Err(_) => 0,
        }
    })
}

/// Read-only count of rows in the legacy `dense.sqlite` index (health messaging only).
pub fn legacy_dense_sqlite_count(workspace: &Path) -> u64 {
    let path = legacy_dense_sqlite_path(workspace);
    if !path.is_file() {
        return 0;
    }

    let conn = match rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Ok(conn) => conn,
        Err(_) => return 0,
    };

    let table_exists = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [TABLE_NAME],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .unwrap_or(false);

    if !table_exists {
        return 0;
    }

    conn.query_row(
        &format!("SELECT COUNT(*) FROM {TABLE_NAME}"),
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count.max(0) as u64)
    .unwrap_or(0)
}

/// Returns the number of rows in the workspace dense embedding index.
pub fn dense_embedding_count(workspace: &Path) -> u64 {
    peek_dense_embedding_count(workspace)
}

fn embedding_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("chunk_id", DataType::Int64, false),
        Field::new(
            VECTOR_COLUMN,
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                EMBEDDING_DIM as i32,
            ),
            false,
        ),
    ]))
}

async fn ensure_table(connection: &Connection) -> Result<Table, DenseError> {
    match connection.open_table(TABLE_NAME).execute().await {
        Ok(table) => Ok(table),
        Err(lancedb::Error::TableNotFound { .. }) => connection
            .create_empty_table(TABLE_NAME, embedding_schema())
            .execute()
            .await
            .map_err(DenseError::from),
        Err(err) => Err(DenseError::from(err)),
    }
}

async fn open_table(connection: &Connection) -> Result<Table, DenseError> {
    connection
        .open_table(TABLE_NAME)
        .execute()
        .await
        .map_err(DenseError::from)
}

fn records_to_batch(records: &[EmbeddingRecord]) -> Result<ArrowRecordBatch, DenseError> {
    let schema = embedding_schema();
    let chunk_ids = Int64Array::from_iter_values(records.iter().map(|record| record.chunk_id.0));
    let vectors = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
        records
            .iter()
            .map(|record| {
                Some(
                    record
                        .embedding
                        .iter()
                        .map(|value| Some(*value))
                        .collect::<Vec<_>>(),
                )
            }),
        EMBEDDING_DIM as i32,
    );

    ArrowRecordBatch::try_new(
        schema,
        vec![Arc::new(chunk_ids), Arc::new(vectors)],
    )
    .map_err(DenseError::from)
}

fn hits_from_batches(batches: &[ArrowRecordBatch]) -> Result<Vec<DenseHit>, DenseError> {
    let mut hits = Vec::new();

    for batch in batches {
        let chunk_ids = batch
            .column_by_name("chunk_id")
            .ok_or_else(|| DenseError::Other("missing chunk_id column".into()))?
            .as_primitive::<Int64Type>();
        let distances = batch
            .column_by_name("_distance")
            .ok_or_else(|| DenseError::Other("missing _distance column".into()))?
            .as_primitive::<Float32Type>();

        for row in 0..batch.num_rows() {
            let distance = distances.value(row);
            hits.push(DenseHit {
                chunk_id: ChunkId(chunk_ids.value(row)),
                score: 1.0 - distance,
            });
        }
    }

    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
        assert!(index.db_path().ends_with("dense"));
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
    fn clear_removes_all_embeddings() {
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

        index.clear().unwrap();
        assert_eq!(index.count().unwrap(), 0);
        let hits = index.search_knn(&sample_embedding(1.0), 5).unwrap();
        assert!(hits.is_empty());
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

    #[test]
    fn peek_dense_embedding_count_does_not_create_storage() {
        let dir = tempdir().unwrap();
        let dense_path = dense_storage_path(dir.path());

        assert_eq!(peek_dense_embedding_count(dir.path()), 0);
        assert!(!dense_path.exists());

        fs::create_dir_all(&dense_path).unwrap();
        assert_eq!(peek_dense_embedding_count(dir.path()), 0);

        let runtime = Runtime::new().unwrap();
        let db_uri = dense_path.to_str().unwrap().to_string();
        runtime.block_on(async {
            let connection = connect(&db_uri).execute().await.unwrap();
            let err = connection.open_table(TABLE_NAME).execute().await.unwrap_err();
            assert!(matches!(err, lancedb::Error::TableNotFound { .. }));
        });
    }

    #[test]
    fn legacy_dense_sqlite_count_reads_existing_rows() {
        let dir = tempdir().unwrap();
        let sqlite_path = legacy_dense_sqlite_path(dir.path());
        fs::create_dir_all(sqlite_path.parent().unwrap()).unwrap();

        let conn = rusqlite::Connection::open(&sqlite_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE chunk_embeddings (chunk_id INTEGER PRIMARY KEY, embedding BLOB NOT NULL);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chunk_embeddings (chunk_id, embedding) VALUES (1, X'0102'), (2, X'0304')",
            [],
        )
        .unwrap();
        drop(conn);

        assert_eq!(legacy_dense_sqlite_count(dir.path()), 2);
        assert_eq!(peek_dense_embedding_count(dir.path()), 0);
    }

    #[test]
    fn dense_embedding_count_reports_rows() {
        let dir = tempdir().unwrap();
        assert_eq!(dense_embedding_count(dir.path()), 0);
        assert!(!dense_storage_path(dir.path()).exists());

        let mut index = DenseIndex::open(dir.path()).unwrap();
        index
            .upsert_batch(&[EmbeddingRecord {
                chunk_id: ChunkId(1),
                embedding: sample_embedding(1.0),
            }])
            .unwrap();

        assert_eq!(peek_dense_embedding_count(dir.path()), 1);
        assert_eq!(dense_embedding_count(dir.path()), 1);
    }
}