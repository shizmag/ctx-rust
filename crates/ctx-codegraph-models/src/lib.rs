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
    fn l2_normalize_is_reexported() {
        let mut vector = vec![3.0_f32, 4.0];
        l2_normalize(&mut vector);
        assert!((vector[0] - 0.6).abs() < 1e-6);
        assert!((vector[1] - 0.8).abs() < 1e-6);
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
    fn model_error_display_includes_path_for_missing_model() {
        let path = PathBuf::from("/tmp/missing-model.onnx");
        let err = ModelError::ModelNotFound(path.clone());
        assert_eq!(
            err.to_string(),
            format!("model file not found: {}", path.display())
        );
    }

    #[test]
    #[ignore = "requires local ONNX models; set CTX_TEST_MODELS=1 to run"]
    fn load_default_models_when_env_set() {
        if std::env::var("CTX_TEST_MODELS").ok().as_deref() != Some("1") {
            return;
        }

        let paths = ModelPaths::default_paths();
        assert!(
            paths.embedding_onnx.exists(),
            "embedding model missing at {}",
            paths.embedding_onnx.display()
        );

        let mut embedding = EmbeddingModel::load(&paths.embedding_onnx, &paths.embedding_tokenizer)
            .expect("embedding");
        let vectors = embedding
            .embed_texts(&["fn main() {}".to_string()])
            .expect("embed");
        assert_eq!(vectors.len(), 1);
        assert_eq!(vectors[0].len(), EMBEDDING_DIM);

        if let Some(reranker_path) = paths.reranker_onnx.as_ref() {
            let rerank_tokenizer = paths
                .rerank_tokenizer
                .as_ref()
                .expect("rerank tokenizer dir");
            let mut reranker =
                RerankerModel::load(reranker_path, rerank_tokenizer).expect("reranker");
            let scores = reranker
                .score_pairs("main function", &["fn main() {}".to_string()])
                .expect("score");
            assert_eq!(scores.len(), 1);
        }
    }
}