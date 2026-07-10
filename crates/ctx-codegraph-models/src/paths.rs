use std::path::{Path, PathBuf};

/// Default embedding ONNX path used when no explicit path is configured.
pub const DEFAULT_EMBEDDING_ONNX: &str =
    "/Users/vladimirkasterin/models/embeddings/snowflake-arctic-embed-m-v2.0/model.onnx";

/// Default reranker ONNX path used when no explicit path is configured.
pub const DEFAULT_RERANKER_ONNX: &str =
    "/Users/vladimirkasterin/models/reranker/jina-reranker-v2-base-multilingual/model.onnx";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelPaths {
    pub embedding_onnx: PathBuf,
    pub reranker_onnx: Option<PathBuf>,
    pub embedding_tokenizer: PathBuf,
    pub rerank_tokenizer: Option<PathBuf>,
}

impl ModelPaths {
    pub fn new(
        embedding_onnx: impl Into<PathBuf>,
        reranker_onnx: Option<PathBuf>,
        embedding_tokenizer: impl Into<PathBuf>,
        rerank_tokenizer: Option<PathBuf>,
    ) -> Self {
        Self {
            embedding_onnx: embedding_onnx.into(),
            reranker_onnx,
            embedding_tokenizer: embedding_tokenizer.into(),
            rerank_tokenizer,
        }
    }

    pub fn default_paths() -> Self {
        let embedding_onnx = PathBuf::from(DEFAULT_EMBEDDING_ONNX);
        let reranker_onnx = {
            let path = PathBuf::from(DEFAULT_RERANKER_ONNX);
            if path.exists() {
                Some(path)
            } else {
                None
            }
        };
        let embedding_tokenizer = embedding_onnx
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| embedding_onnx.clone());
        let rerank_tokenizer = reranker_onnx
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf));

        Self {
            embedding_onnx,
            reranker_onnx,
            embedding_tokenizer,
            rerank_tokenizer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stores_explicit_paths() {
        let paths = ModelPaths::new(
            "/tmp/embedding.onnx",
            Some(PathBuf::from("/tmp/reranker.onnx")),
            "/tmp/embedding-tokenizer",
            Some(PathBuf::from("/tmp/rerank-tokenizer")),
        );

        assert_eq!(paths.embedding_onnx, PathBuf::from("/tmp/embedding.onnx"));
        assert_eq!(
            paths.reranker_onnx,
            Some(PathBuf::from("/tmp/reranker.onnx"))
        );
        assert_eq!(
            paths.embedding_tokenizer,
            PathBuf::from("/tmp/embedding-tokenizer")
        );
        assert_eq!(
            paths.rerank_tokenizer,
            Some(PathBuf::from("/tmp/rerank-tokenizer"))
        );
    }

    #[test]
    fn default_paths_uses_embedding_constant_and_parent_tokenizer_dir() {
        let paths = ModelPaths::default_paths();
        let expected_embedding = PathBuf::from(DEFAULT_EMBEDDING_ONNX);

        assert_eq!(paths.embedding_onnx, expected_embedding);
        assert_eq!(
            paths.embedding_tokenizer,
            expected_embedding
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| expected_embedding.clone())
        );

        let reranker_path = PathBuf::from(DEFAULT_RERANKER_ONNX);
        if reranker_path.exists() {
            assert_eq!(paths.reranker_onnx, Some(reranker_path.clone()));
            assert_eq!(
                paths.rerank_tokenizer,
                reranker_path.parent().map(Path::to_path_buf)
            );
        } else {
            assert_eq!(paths.reranker_onnx, None);
            assert_eq!(paths.rerank_tokenizer, None);
        }
    }
}