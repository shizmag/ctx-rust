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

        let mut embedding =
            EmbeddingModel::load(&paths.embedding_onnx, &paths.tokenizer_dir).expect("embedding");
        let vectors = embedding
            .embed_texts(&["fn main() {}".to_string()])
            .expect("embed");
        assert_eq!(vectors.len(), 1);
        assert_eq!(vectors[0].len(), EMBEDDING_DIM);

        if let Some(reranker_path) = paths.reranker_onnx.as_ref() {
            let mut reranker =
                RerankerModel::load(reranker_path, &paths.tokenizer_dir).expect("reranker");
            let scores = reranker
                .score_pairs("main function", &["fn main() {}".to_string()])
                .expect("score");
            assert_eq!(scores.len(), 1);
        }
    }
}