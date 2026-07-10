use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("model file not found: {0}")]
    ModelNotFound(PathBuf),

    #[error("tokenizer error: {0}")]
    Tokenizer(String),

    #[error("onnx runtime error: {0}")]
    Onnx(#[from] ort::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("inference error: {0}")]
    Inference(String),

    #[error("invalid embedding dimension: expected {expected}, got {got}")]
    InvalidEmbeddingDim { expected: usize, got: usize },
}