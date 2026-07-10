use ctx_codegraph_models::{
    EmbeddingModel, EMBEDDING_DIM, ModelPaths, RerankerModel,
};

fn models_enabled() -> bool {
    std::env::var("CTX_TEST_MODELS").ok().as_deref() == Some("1")
}

#[test]
#[ignore = "requires local ONNX models; set CTX_TEST_MODELS=1 to run"]
fn load_default_models_and_run_inference() {
    if !models_enabled() {
        return;
    }

    let paths = ModelPaths::default_paths();
    assert!(
        paths.embedding_onnx.exists(),
        "embedding model missing at {}",
        paths.embedding_onnx.display()
    );

    let mut embedding =
        EmbeddingModel::load(&paths.embedding_onnx, &paths.embedding_tokenizer).expect("embedding");
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
        assert!(scores[0].is_finite());
    }
}