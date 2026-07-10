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
    pub tokenizer_dir: PathBuf,
}

impl ModelPaths {
    pub fn new(
        embedding_onnx: impl Into<PathBuf>,
        reranker_onnx: Option<PathBuf>,
        tokenizer_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            embedding_onnx: embedding_onnx.into(),
            reranker_onnx,
            tokenizer_dir: tokenizer_dir.into(),
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
        let tokenizer_dir = embedding_onnx
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| embedding_onnx.clone());

        Self {
            embedding_onnx,
            reranker_onnx,
            tokenizer_dir,
        }
    }
}