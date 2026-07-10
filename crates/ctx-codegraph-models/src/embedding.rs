use std::path::Path;

use ort::session::Session;
use ort::value::Tensor;

use crate::error::ModelError;
use crate::tokenizer::CodeTokenizer;

pub const EMBEDDING_DIM: usize = 768;

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

        let session = Session::builder()?
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