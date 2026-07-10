use std::path::Path;

use ort::session::Session;
use ort::value::Tensor;

use crate::error::ModelError;
use crate::tokenizer::CodeTokenizer;

pub struct RerankerModel {
    session: Session,
    tokenizer: CodeTokenizer,
    input_ids_name: String,
    attention_mask_name: String,
}

impl RerankerModel {
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

    pub fn score_pairs(&mut self, query: &str, docs: &[String]) -> Result<Vec<f32>, ModelError> {
        if docs.is_empty() {
            return Ok(Vec::new());
        }

        let encodings: Vec<(Vec<i64>, Vec<i64>)> = docs
            .iter()
            .map(|doc| self.tokenizer.encode_pair(query, doc))
            .collect::<Result<_, _>>()?;

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
            return Err(ModelError::Inference("reranker model returned no outputs".into()));
        }

        let (output_shape, output_data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| ModelError::Inference(e.to_string()))?;

        let shape: &[i64] = output_shape;
        let scores = if shape.len() == 1 {
            output_data.to_vec()
        } else if shape.len() == 2 {
            let cols = shape[1] as usize;
            if cols == 1 {
                output_data.to_vec()
            } else {
                (0..shape[0] as usize)
                    .map(|row| output_data[row * cols])
                    .collect()
            }
        } else {
            return Err(ModelError::Inference(format!(
                "unexpected reranker output shape: {shape:?}"
            )));
        };

        Ok(scores)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
        let missing = Path::new("/nonexistent/ctx-codegraph-models/reranker.onnx");
        let tokenizer_dir = Path::new("/tmp");

        let err = match RerankerModel::load(missing, tokenizer_dir) {
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

        let err = match RerankerModel::load(&model_path, dir.path()) {
            Err(err) => err,
            Ok(_) => panic!("expected Onnx error"),
        };
        assert!(matches!(err, ModelError::Onnx(_)));
    }

    #[test]
    fn load_returns_tokenizer_error_for_invalid_tokenizer_json() {
        let model_path = Path::new(crate::paths::DEFAULT_RERANKER_ONNX);
        if !model_path.exists() {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tokenizer.json"), "not valid json").unwrap();

        let err = match RerankerModel::load(model_path, dir.path()) {
            Err(err) => err,
            Ok(_) => panic!("expected Tokenizer error"),
        };
        assert!(matches!(err, ModelError::Tokenizer(_)));
    }

    fn load_default_reranker_model() -> Option<RerankerModel> {
        let paths = crate::paths::ModelPaths::default_paths();
        let reranker_path = paths.reranker_onnx.as_ref()?;
        let rerank_tokenizer = paths.rerank_tokenizer.as_ref()?;
        Some(
            RerankerModel::load(reranker_path, rerank_tokenizer)
                .expect("reranker model"),
        )
    }

    #[test]
    #[ignore = "requires local ONNX models; set CTX_TEST_MODELS=1 to run"]
    fn score_pairs_empty_docs_returns_empty() {
        if std::env::var("CTX_TEST_MODELS").ok().as_deref() != Some("1") {
            return;
        }

        let mut model = load_default_reranker_model().expect("reranker model path");
        let scores = model.score_pairs("query", &[]).unwrap();
        assert!(scores.is_empty());
    }

    #[test]
    #[ignore = "requires local ONNX models; set CTX_TEST_MODELS=1 to run"]
    fn score_pairs_single_doc_returns_one_score() {
        if std::env::var("CTX_TEST_MODELS").ok().as_deref() != Some("1") {
            return;
        }

        let mut model = load_default_reranker_model().expect("reranker model path");
        let scores = model
            .score_pairs("main function", &["fn main() {}".to_string()])
            .expect("score single doc");

        assert_eq!(scores.len(), 1);
        assert!(scores[0].is_finite());
    }

    #[test]
    #[ignore = "requires local ONNX models; set CTX_TEST_MODELS=1 to run"]
    fn score_pairs_multi_doc_batch_returns_score_per_doc() {
        if std::env::var("CTX_TEST_MODELS").ok().as_deref() != Some("1") {
            return;
        }

        let docs = vec![
            "fn main() {}".to_string(),
            "struct Point { x: f32, y: f32 }".to_string(),
            "fn add(a: i32, b: i32) -> i32 { a + b }".to_string(),
        ];

        let mut model = load_default_reranker_model().expect("reranker model path");
        let scores = model
            .score_pairs("entry point function", &docs)
            .expect("score multi-doc batch");

        assert_eq!(scores.len(), docs.len());
        assert!(scores.iter().all(|score| score.is_finite()));
    }
}