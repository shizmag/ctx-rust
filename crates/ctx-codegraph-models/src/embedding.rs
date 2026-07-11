use std::path::Path;

use ort::session::Session;
use ort::value::Tensor;

use crate::error::ModelError;
use crate::tokenizer::CodeTokenizer;

pub const EMBEDDING_DIM: usize = 768;
pub const DEFAULT_EMBED_BATCH_SIZE: usize = 16;

/// Yields half-open index ranges `[start, end)` covering `0..len` in batches of `batch_size`.
pub fn batch_ranges(len: usize, batch_size: usize) -> impl Iterator<Item = std::ops::Range<usize>> {
    assert!(batch_size > 0, "batch_size must be > 0");
    (0..len).step_by(batch_size).map(move |start| {
        let end = (start + batch_size).min(len);
        start..end
    })
}

pub struct EmbeddingModel {
    session: Session,
    tokenizer: CodeTokenizer,
    input_ids_name: String,
    attention_mask_name: String,
}

impl EmbeddingModel {
    pub fn load(model_path: &Path, tokenizer_dir: &Path) -> Result<Self, ModelError> {
        if !model_path.exists() {
            return Err(ModelError::ModelNotFound(model_path.to_path_buf()));
        }

        let intra_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(4)
            .max(1);
        let mut builder = Session::builder().map_err(ModelError::Onnx)?;
        builder = builder
            .with_intra_threads(intra_threads)
            .map_err(|e| ModelError::Inference(e.to_string()))?;
        let session = builder
            .commit_from_file(model_path)
            .map_err(ModelError::Onnx)?;

        let (input_ids_name, attention_mask_name) = discover_text_inputs(&session)?;

        Ok(Self {
            session,
            tokenizer: CodeTokenizer::from_dir(tokenizer_dir)?,
            input_ids_name,
            attention_mask_name,
        })
    }

    pub fn embed_texts(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, ModelError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let encodings = self.tokenizer.encode_batch(texts)?;
        let batch_size = encodings.len();
        let max_len = encodings
            .iter()
            .map(|(ids, _)| ids.len())
            .max()
            .unwrap_or(0)
            .max(1);

        let mut input_ids = vec![0_i64; batch_size * max_len];
        let mut attention_mask = vec![0_i64; batch_size * max_len];

        for (batch_idx, (ids, mask)) in encodings.iter().enumerate() {
            for (token_idx, &id) in ids.iter().enumerate() {
                input_ids[batch_idx * max_len + token_idx] = id;
            }
            for (token_idx, &mask_val) in mask.iter().enumerate() {
                attention_mask[batch_idx * max_len + token_idx] = mask_val;
            }
        }

        let shape = [batch_size as i64, max_len as i64];
        let input_ids_tensor = Tensor::from_array((shape, input_ids.into_boxed_slice()))?;
        let attention_mask_tensor = Tensor::from_array((shape, attention_mask.into_boxed_slice()))?;

        let outputs = self.session.run(ort::inputs![
            self.input_ids_name.as_str() => input_ids_tensor,
            self.attention_mask_name.as_str() => attention_mask_tensor,
        ])?;

        if outputs.len() == 0 {
            return Err(ModelError::Inference(
                "embedding model returned no outputs".into(),
            ));
        }

        let (output_shape, output_data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| ModelError::Inference(e.to_string()))?;

        let shape: &[i64] = output_shape;
        let rows = if shape.len() == 2 {
            shape[0] as usize
        } else if shape.len() == 3 {
            shape[0] as usize
        } else {
            return Err(ModelError::Inference(format!(
                "unexpected embedding output shape: {shape:?}"
            )));
        };

        let cols = if shape.len() == 2 {
            shape[1] as usize
        } else {
            shape[2] as usize
        };

        if cols != EMBEDDING_DIM {
            return Err(ModelError::InvalidEmbeddingDim {
                expected: EMBEDDING_DIM,
                got: cols,
            });
        }

        let mut embeddings = Vec::with_capacity(rows);
        for row in 0..rows {
            let start = row * cols;
            let end = start + cols;
            let mut vector = output_data[start..end].to_vec();
            l2_normalize(&mut vector);
            embeddings.push(vector);
        }

        Ok(embeddings)
    }

    /// Embeds texts in fixed-size batches using a single loaded model session.
    pub fn embed_texts_batched(
        &mut self,
        texts: &[String],
        batch_size: usize,
    ) -> Result<Vec<Vec<f32>>, ModelError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        assert!(batch_size > 0, "batch_size must be > 0");

        let mut all_embeddings = Vec::with_capacity(texts.len());
        for range in batch_ranges(texts.len(), batch_size) {
            let batch_embeddings = self.embed_texts(&texts[range])?;
            all_embeddings.extend(batch_embeddings);
        }
        Ok(all_embeddings)
    }
}

fn discover_text_inputs(session: &Session) -> Result<(String, String), ModelError> {
    let names: Vec<String> = session
        .inputs()
        .iter()
        .map(|input| input.name().to_string())
        .collect();
    discover_text_input_names(&names)
}

fn discover_text_input_names(names: &[String]) -> Result<(String, String), ModelError> {
    let input_ids = names
        .iter()
        .find(|name| name.contains("input_ids"))
        .cloned()
        .or_else(|| names.first().cloned())
        .ok_or_else(|| ModelError::Inference("model has no inputs".into()))?;

    let attention_mask = names
        .iter()
        .find(|name| name.contains("attention_mask"))
        .cloned()
        .unwrap_or_else(|| input_ids.clone());

    Ok((input_ids, attention_mask))
}

pub fn l2_normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for value in vector.iter_mut() {
            *value /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn batch_ranges_splits_evenly() {
        assert_eq!(
            batch_ranges(10, 3).collect::<Vec<_>>(),
            vec![0..3, 3..6, 6..9, 9..10]
        );
    }

    #[test]
    fn batch_ranges_empty_when_len_zero() {
        assert_eq!(batch_ranges(0, 32).collect::<Vec<_>>(), Vec::<std::ops::Range<usize>>::new());
    }

    #[test]
    fn batch_ranges_single_batch_when_len_smaller_than_batch_size() {
        assert_eq!(batch_ranges(5, 32).collect::<Vec<_>>(), vec![0..5]);
    }

    #[test]
    fn batch_ranges_exact_multiple_has_no_trailing_partial() {
        assert_eq!(
            batch_ranges(6, 3).collect::<Vec<_>>(),
            vec![0..3, 3..6]
        );
    }

    #[test]
    fn batch_ranges_covers_all_indices_without_gaps() {
        let len = 37;
        let batch_size = 8;
        let ranges: Vec<_> = batch_ranges(len, batch_size).collect();
        assert_eq!(ranges.first().map(|r| r.start), Some(0));
        assert_eq!(ranges.last().map(|r| r.end), Some(len));
        let covered: usize = ranges.iter().map(|r| r.end - r.start).sum();
        assert_eq!(covered, len);
        for window in ranges.windows(2) {
            assert_eq!(window[0].end, window[1].start, "batch ranges must be contiguous");
        }
    }

    #[test]
    fn default_embed_batch_size_is_positive() {
        assert!(DEFAULT_EMBED_BATCH_SIZE > 0);
    }

    /// `embed_texts_batched` short-circuits on empty input before any ONNX inference.
    #[test]
    fn embed_texts_batched_empty_input_uses_no_batch_ranges() {
        assert!(batch_ranges(0, DEFAULT_EMBED_BATCH_SIZE).next().is_none());
    }

    /// Single-text batched embedding uses exactly one batch range (no ONNX required to verify plan).
    #[test]
    fn embed_texts_batched_single_text_uses_one_batch_range() {
        let ranges: Vec<_> = batch_ranges(1, DEFAULT_EMBED_BATCH_SIZE).collect();
        assert_eq!(ranges, vec![0..1]);
    }

    #[test]
    fn l2_normalize_scales_to_unit_length() {
        let mut vector = vec![3.0_f32, 4.0];
        l2_normalize(&mut vector);
        assert!((vector[0] - 0.6).abs() < 1e-6);
        assert!((vector[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn l2_normalize_leaves_zero_vector_unchanged() {
        let mut vector = vec![0.0_f32, 0.0, 0.0];
        l2_normalize(&mut vector);
        assert_eq!(vector, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn l2_normalize_leaves_already_normalized_vector_unchanged() {
        let mut vector = vec![1.0_f32, 0.0];
        l2_normalize(&mut vector);
        assert!((vector[0] - 1.0).abs() < 1e-6);
        assert!(vector[1].abs() < 1e-6);
    }

    #[test]
    fn discover_text_input_names_finds_standard_inputs() {
        let names = vec![
            "token_type_ids".into(),
            "input_ids".into(),
            "attention_mask".into(),
        ];
        let (input_ids, attention_mask) = discover_text_input_names(&names).unwrap();
        assert_eq!(input_ids, "input_ids");
        assert_eq!(attention_mask, "attention_mask");
    }

    #[test]
    fn discover_text_input_names_falls_back_to_first_input() {
        let names = vec!["tokens".into(), "masks".into()];
        let (input_ids, attention_mask) = discover_text_input_names(&names).unwrap();
        assert_eq!(input_ids, "tokens");
        assert_eq!(attention_mask, "tokens");
    }

    #[test]
    fn discover_text_input_names_reuses_input_ids_when_mask_missing() {
        let names = vec!["input_ids".into(), "token_type_ids".into()];
        let (input_ids, attention_mask) = discover_text_input_names(&names).unwrap();
        assert_eq!(input_ids, "input_ids");
        assert_eq!(attention_mask, "input_ids");
    }

    #[test]
    fn discover_text_input_names_errors_when_empty() {
        let names: Vec<String> = vec![];
        let err = discover_text_input_names(&names).unwrap_err();
        assert!(matches!(err, ModelError::Inference(msg) if msg == "model has no inputs"));
    }

    #[test]
    fn load_returns_model_not_found_for_missing_file() {
        let missing = Path::new("/nonexistent/ctx-codegraph-models/embedding.onnx");
        let tokenizer_dir = Path::new("/tmp");

        let err = match EmbeddingModel::load(missing, tokenizer_dir) {
            Err(err) => err,
            Ok(_) => panic!("expected ModelNotFound error"),
        };
        assert!(matches!(err, ModelError::ModelNotFound(path) if path == missing));
    }

    #[test]
    fn load_returns_onnx_error_for_corrupt_model_file() {
        let dir = tempfile::tempdir().unwrap();
        let model_path = dir.path().join("corrupt.onnx");
        std::fs::write(&model_path, b"not a valid onnx file").unwrap();

        let err = match EmbeddingModel::load(&model_path, dir.path()) {
            Err(err) => err,
            Ok(_) => panic!("expected Onnx error"),
        };
        assert!(matches!(err, ModelError::Onnx(_)));
    }

    #[test]
    fn load_returns_tokenizer_error_for_invalid_tokenizer_json() {
        let model_path = Path::new(crate::paths::DEFAULT_EMBEDDING_ONNX);
        if !model_path.exists() {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tokenizer.json"), "not valid json").unwrap();

        let err = match EmbeddingModel::load(model_path, dir.path()) {
            Err(err) => err,
            Ok(_) => panic!("expected Tokenizer error"),
        };
        assert!(matches!(err, ModelError::Tokenizer(_)));
    }

    fn load_default_embedding_model() -> EmbeddingModel {
        let paths = crate::paths::ModelPaths::default_paths();
        EmbeddingModel::load(&paths.embedding_onnx, &paths.embedding_tokenizer)
            .expect("embedding model")
    }

    fn assert_unit_length(vector: &[f32]) {
        let norm: f32 = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-4,
            "expected unit-length embedding, got norm {norm}"
        );
    }

    #[test]
    #[ignore = "requires local ONNX models; set CTX_TEST_MODELS=1 to run"]
    fn embed_texts_empty_batch_returns_empty() {
        if std::env::var("CTX_TEST_MODELS").ok().as_deref() != Some("1") {
            return;
        }

        let mut model = load_default_embedding_model();
        let vectors = model.embed_texts(&[]).unwrap();
        assert!(vectors.is_empty());
    }

    #[test]
    #[ignore = "requires local ONNX models; set CTX_TEST_MODELS=1 to run"]
    fn embed_texts_single_text_produces_normalized_vector() {
        if std::env::var("CTX_TEST_MODELS").ok().as_deref() != Some("1") {
            return;
        }

        let mut model = load_default_embedding_model();
        let vectors = model
            .embed_texts(&["fn main() {}".to_string()])
            .expect("embed single text");

        assert_eq!(vectors.len(), 1);
        assert_eq!(vectors[0].len(), EMBEDDING_DIM);
        assert_unit_length(&vectors[0]);
    }

    #[test]
    #[ignore = "requires local ONNX models; set CTX_TEST_MODELS=1 to run"]
    fn embed_texts_batched_matches_single_call() {
        if std::env::var("CTX_TEST_MODELS").ok().as_deref() != Some("1") {
            return;
        }

        let texts = vec![
            "fn main() {}".to_string(),
            "struct Point { x: f32, y: f32 }".to_string(),
            "impl Display for Point {}".to_string(),
            "enum Color { Red, Green, Blue }".to_string(),
            "trait Greeter { fn greet(&self); }".to_string(),
        ];

        let mut model = load_default_embedding_model();
        let single = model.embed_texts(&texts).expect("embed batch");
        let batched = model
            .embed_texts_batched(&texts, 2)
            .expect("embed batched");

        assert_eq!(batched.len(), texts.len());
        assert_eq!(batched.len(), single.len());
        for (expected, actual) in single.iter().zip(batched.iter()) {
            assert_eq!(expected.len(), actual.len());
            for (a, b) in expected.iter().zip(actual.iter()) {
                assert!((a - b).abs() < 1e-5, "batched embeddings should match single call");
            }
        }
    }

    #[test]
    #[ignore = "requires local ONNX models; set CTX_TEST_MODELS=1 to run"]
    fn embed_texts_batch_returns_one_vector_per_text() {
        if std::env::var("CTX_TEST_MODELS").ok().as_deref() != Some("1") {
            return;
        }

        let texts = vec![
            "fn main() {}".to_string(),
            "struct Point { x: f32, y: f32 }".to_string(),
            "impl Display for Point {}".to_string(),
        ];

        let mut model = load_default_embedding_model();
        let vectors = model.embed_texts(&texts).expect("embed batch");

        assert_eq!(vectors.len(), texts.len());
        for vector in &vectors {
            assert_eq!(vector.len(), EMBEDDING_DIM);
            assert_unit_length(vector);
        }
    }
}