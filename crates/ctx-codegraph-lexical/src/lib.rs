use ctx_codegraph_chunk::ChunkId;
use ctx_codegraph_lang::model::SymbolId;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, INDEXED, STORED, TEXT};
use tantivy::{Index, IndexWriter, ReloadPolicy, Term};

#[derive(Debug, thiserror::Error)]
pub enum LexicalError {
    #[error("tantivy error: {0}")]
    Tantivy(#[from] tantivy::TantivyError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("lexical index error: {0}")]
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChunkHit {
    pub chunk_id: ChunkId,
    pub symbol_id: Option<SymbolId>,
    pub score: f32,
}

#[derive(Debug, Clone)]
pub struct IndexDoc {
    pub chunk_id: ChunkId,
    pub symbol_id: Option<SymbolId>,
    pub path: String,
    pub qualified_name: String,
    pub text: String,
}

pub struct LexicalIndex {
    index_dir: PathBuf,
    index: Index,
    schema: Schema,
    chunk_id_field: Field,
    symbol_id_field: Field,
    path_field: Field,
    qualified_name_field: Field,
    text_field: Field,
}

impl LexicalIndex {
    pub fn open(workspace: &Path) -> Result<Self, LexicalError> {
        let index_dir = workspace.join(".ctx-codegraph").join("lexical");
        std::fs::create_dir_all(&index_dir)?;

        let mut schema_builder = Schema::builder();
        let chunk_id_field = schema_builder.add_i64_field("chunk_id", STORED | INDEXED);
        let symbol_id_field = schema_builder.add_i64_field("symbol_id", STORED | INDEXED);
        let path_field = schema_builder.add_text_field("path", TEXT | STORED);
        let qualified_name_field = schema_builder.add_text_field("qualified_name", TEXT | STORED);
        let text_field = schema_builder.add_text_field("text", TEXT | STORED);
        let schema = schema_builder.build();

        let index = if index_dir.join("meta.json").exists() {
            Index::open_in_dir(&index_dir)?
        } else {
            Index::create_in_dir(&index_dir, schema.clone())?
        };

        Ok(Self {
            index_dir,
            index,
            schema,
            chunk_id_field,
            symbol_id_field,
            path_field,
            qualified_name_field,
            text_field,
        })
    }

    pub fn index_dir(&self) -> &Path {
        &self.index_dir
    }

    pub fn build(&mut self, docs: &[IndexDoc]) -> Result<(), LexicalError> {
        if self.index_dir.exists() {
            let _ = std::fs::remove_dir_all(&self.index_dir);
        }
        std::fs::create_dir_all(&self.index_dir)?;
        self.index = Index::create_in_dir(&self.index_dir, self.schema.clone())?;
        let mut writer: IndexWriter = self.index.writer(50_000_000)?;

        for doc in docs {
            let mut tantivy_doc = tantivy::TantivyDocument::default();
            tantivy_doc.add_i64(self.chunk_id_field, doc.chunk_id.0);
            if let Some(symbol_id) = doc.symbol_id {
                tantivy_doc.add_i64(self.symbol_id_field, symbol_id.0);
            }
            tantivy_doc.add_text(self.path_field, &doc.path);
            tantivy_doc.add_text(self.qualified_name_field, &doc.qualified_name);
            tantivy_doc.add_text(self.text_field, &doc.text);
            writer.add_document(tantivy_doc)?;
        }

        writer.commit()?;
        Ok(())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<ChunkHit>, LexicalError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        let searcher = reader.searcher();

        let parser = QueryParser::for_index(
            &self.index,
            vec![
                self.text_field,
                self.qualified_name_field,
                self.path_field,
            ],
        );
        let parsed_query = parser
            .parse_query(query)
            .map_err(|e| LexicalError::Other(e.to_string()))?;

        let top_docs = searcher.search(
            &parsed_query,
            &TopDocs::with_limit(limit.max(1)),
        )?;

        let mut hits = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let retrieved: tantivy::TantivyDocument = searcher.doc(doc_address)?;
            let chunk_id = retrieved
                .get_first(self.chunk_id_field)
                .and_then(|value| value.as_i64())
                .map(ChunkId)
                .ok_or_else(|| LexicalError::Other("missing chunk_id in indexed document".into()))?;
            let symbol_id = retrieved
                .get_first(self.symbol_id_field)
                .and_then(|value| value.as_i64())
                .map(SymbolId);
            hits.push(ChunkHit {
                chunk_id,
                symbol_id,
                score,
            });
        }

        Ok(hits)
    }

    pub fn remove_chunk_ids(&mut self, chunk_ids: &[ChunkId]) -> Result<(), LexicalError> {
        if chunk_ids.is_empty() {
            return Ok(());
        }

        let unique: HashSet<i64> = chunk_ids.iter().map(|id| id.0).collect();
        let mut writer: IndexWriter = self.index.writer(50_000_000)?;

        for chunk_id in unique {
            let term = Term::from_field_i64(self.chunk_id_field, chunk_id);
            writer.delete_term(term);
        }

        writer.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_codegraph_lang::model::SymbolId;
    use tempfile::tempdir;

    fn sample_docs() -> Vec<IndexDoc> {
        vec![
            IndexDoc {
                chunk_id: ChunkId(1),
                symbol_id: Some(SymbolId(10)),
                path: "src/auth.rs".to_string(),
                qualified_name: "auth::authenticate".to_string(),
                text: "pub fn authenticate() -> bool { verify_token() }".to_string(),
            },
            IndexDoc {
                chunk_id: ChunkId(2),
                symbol_id: Some(SymbolId(20)),
                path: "src/db.rs".to_string(),
                qualified_name: "db::connect".to_string(),
                text: "pub fn connect() -> Connection { Connection::new() }".to_string(),
            },
            IndexDoc {
                chunk_id: ChunkId(3),
                symbol_id: None,
                path: "src/lib.rs".to_string(),
                qualified_name: "root".to_string(),
                text: "mod auth; mod db;".to_string(),
            },
        ]
    }

    #[test]
    fn lexical_index_open_creates_directory() {
        let dir = tempdir().unwrap();
        let index = LexicalIndex::open(dir.path()).unwrap();
        assert!(index.index_dir().exists());
        assert!(index.index_dir().ends_with("lexical"));
    }

    #[test]
    fn lexical_index_open_reuses_existing_index() {
        let dir = tempdir().unwrap();
        {
            let mut index = LexicalIndex::open(dir.path()).unwrap();
            index.build(&sample_docs()).unwrap();
        }
        let reopened = LexicalIndex::open(dir.path()).unwrap();
        let hits = reopened.search("authenticate", 5).unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].chunk_id, ChunkId(1));
    }

    #[test]
    fn lexical_index_build_and_search() {
        let dir = tempdir().unwrap();
        let mut index = LexicalIndex::open(dir.path()).unwrap();
        index.build(&sample_docs()).unwrap();

        let auth_hits = index.search("authenticate", 5).unwrap();
        assert_eq!(auth_hits.len(), 1);
        assert_eq!(auth_hits[0].chunk_id, ChunkId(1));
        assert_eq!(auth_hits[0].symbol_id, Some(SymbolId(10)));
        assert!(auth_hits[0].score > 0.0);

        let db_hits = index.search("connect database", 5).unwrap();
        assert!(!db_hits.is_empty());
        assert_eq!(db_hits[0].chunk_id, ChunkId(2));

        // Qualified names are indexed as text; search by symbol fragment (not `::` — query parser syntax).
        let qualified_hits = index.search("auth authenticate", 5).unwrap();
        assert!(!qualified_hits.is_empty());
        assert_eq!(qualified_hits[0].chunk_id, ChunkId(1));
    }

    #[test]
    fn lexical_index_search_empty_query_returns_no_hits() {
        let dir = tempdir().unwrap();
        let mut index = LexicalIndex::open(dir.path()).unwrap();
        index.build(&sample_docs()).unwrap();

        assert!(index.search("", 5).unwrap().is_empty());
        assert!(index.search("   ", 5).unwrap().is_empty());
    }

    #[test]
    fn lexical_index_remove_chunk_ids() {
        let dir = tempdir().unwrap();
        let mut index = LexicalIndex::open(dir.path()).unwrap();
        index.build(&sample_docs()).unwrap();

        assert!(!index.search("authenticate", 5).unwrap().is_empty());
        index.remove_chunk_ids(&[ChunkId(1)]).unwrap();
        let hits = index.search("authenticate", 5).unwrap();
        assert!(hits.is_empty() || hits[0].chunk_id != ChunkId(1));
    }

    #[test]
    fn lexical_index_remove_empty_chunk_ids_is_noop() {
        let dir = tempdir().unwrap();
        let mut index = LexicalIndex::open(dir.path()).unwrap();
        index.build(&sample_docs()).unwrap();
        index.remove_chunk_ids(&[]).unwrap();
        assert!(!index.search("authenticate", 5).unwrap().is_empty());
    }
}