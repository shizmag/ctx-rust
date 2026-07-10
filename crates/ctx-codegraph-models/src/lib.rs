pub mod embedding;
pub mod error;
pub mod fingerprint;
pub mod paths;
pub mod reranker;
pub mod tokenizer;

pub use embedding::{EmbeddingModel, EMBEDDING_DIM, l2_normalize};
pub use error::ModelError;
pub use fingerprint::file_fingerprint;
pub use paths::{ModelPaths, DEFAULT_EMBEDDING_ONNX, DEFAULT_RERANKER_ONNX};
pub use reranker::RerankerModel;
pub use tokenizer::CodeTokenizer;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn embedding_dim_constant_is_public() {
        assert_eq!(EMBEDDING_DIM, 768);
    }

    #[test]
    fn default_onnx_constants_are_non_empty_paths() {
        assert!(!DEFAULT_EMBEDDING_ONNX.is_empty());
        assert!(!DEFAULT_RERANKER_ONNX.is_empty());
        assert!(DEFAULT_EMBEDDING_ONNX.ends_with(".onnx"));
        assert!(DEFAULT_RERANKER_ONNX.ends_with(".onnx"));
    }

    #[test]
    fn l2_normalize_is_reexported() {
        let mut vector = vec![3.0_f32, 4.0];
        l2_normalize(&mut vector);
        assert!((vector[0] - 0.6).abs() < 1e-6);
        assert!((vector[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn file_fingerprint_reexport_computes_sha256_hex() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("sample.txt");
        std::fs::write(&file_path, b"ctx-codegraph-models").unwrap();

        let digest = file_fingerprint(&file_path).unwrap();
        assert_eq!(digest.len(), 64);
        assert_eq!(
            digest,
            "b86138d3c1d9ace53b17a2bf014e2292dd97dd512e6434b8e9fdfe9b8cdb56d9"
        );
    }

    #[test]
    fn code_tokenizer_reexport_is_usable() {
        let dir = tempfile::tempdir().unwrap();
        let tokenizer = CodeTokenizer::from_dir(dir.path()).unwrap();
        let (ids, mask) = tokenizer.encode("alpha beta").unwrap();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(mask, vec![1, 1]);
    }

    #[test]
    fn model_paths_round_trips_through_serde() {
        let paths = ModelPaths::new(
            "/tmp/embedding.onnx",
            Some(PathBuf::from("/tmp/reranker.onnx")),
            "/tmp/embedding-tokenizer",
            None,
        );

        let json = serde_json::to_string(&paths).unwrap();
        let decoded: ModelPaths = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, paths);
    }

    #[test]
    fn model_error_variants_are_reexported_and_display_correctly() {
        let path = PathBuf::from("/tmp/missing-model.onnx");
        assert_eq!(
            ModelError::ModelNotFound(path.clone()).to_string(),
            format!("model file not found: {}", path.display())
        );
        assert_eq!(
            ModelError::Tokenizer("bad vocab".into()).to_string(),
            "tokenizer error: bad vocab"
        );
        assert_eq!(
            ModelError::Inference("no outputs".into()).to_string(),
            "inference error: no outputs"
        );
        assert_eq!(
            ModelError::InvalidEmbeddingDim {
                expected: EMBEDDING_DIM,
                got: 512,
            }
            .to_string(),
            format!("invalid embedding dimension: expected {EMBEDDING_DIM}, got 512")
        );
    }

    #[test]
    fn embedding_and_reranker_types_are_reexported() {
        fn assert_embedding_loadable(path: &std::path::Path, tokenizer_dir: &std::path::Path) {
            let err = match EmbeddingModel::load(path, tokenizer_dir) {
                Err(err) => err,
                Ok(_) => panic!("expected ModelNotFound error"),
            };
            assert!(matches!(err, ModelError::ModelNotFound(_)));
        }

        fn assert_reranker_loadable(path: &std::path::Path, tokenizer_dir: &std::path::Path) {
            let err = match RerankerModel::load(path, tokenizer_dir) {
                Err(err) => err,
                Ok(_) => panic!("expected ModelNotFound error"),
            };
            assert!(matches!(err, ModelError::ModelNotFound(_)));
        }

        let missing = std::path::Path::new("/nonexistent/ctx-codegraph-models/model.onnx");
        let tokenizer_dir = std::path::Path::new("/tmp");
        assert_embedding_loadable(missing, tokenizer_dir);
        assert_reranker_loadable(missing, tokenizer_dir);
    }
}