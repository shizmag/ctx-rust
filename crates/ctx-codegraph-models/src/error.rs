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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn model_not_found_display_includes_path() {
        let path = PathBuf::from("/tmp/missing.onnx");
        let err = ModelError::ModelNotFound(path.clone());
        assert_eq!(
            err.to_string(),
            format!("model file not found: {}", path.display())
        );
    }

    #[test]
    fn tokenizer_display_includes_message() {
        let err = ModelError::Tokenizer("bad vocab".into());
        assert_eq!(err.to_string(), "tokenizer error: bad vocab");
    }

    #[test]
    fn onnx_display_wraps_underlying_error() {
        let source = ort::Error::new("session build failed");
        let err = ModelError::Onnx(source);
        assert!(err.to_string().starts_with("onnx runtime error:"));
        assert!(err.to_string().contains("session build failed"));
    }

    #[test]
    fn io_display_wraps_underlying_error() {
        let source = io::Error::new(io::ErrorKind::NotFound, "file missing");
        let err = ModelError::Io(source);
        assert_eq!(err.to_string(), "io error: file missing");
    }

    #[test]
    fn inference_display_includes_message() {
        let err = ModelError::Inference("no outputs".into());
        assert_eq!(err.to_string(), "inference error: no outputs");
    }

    #[test]
    fn invalid_embedding_dim_display_includes_expected_and_got() {
        let err = ModelError::InvalidEmbeddingDim {
            expected: 768,
            got: 512,
        };
        assert_eq!(
            err.to_string(),
            "invalid embedding dimension: expected 768, got 512"
        );
    }
}