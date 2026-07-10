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