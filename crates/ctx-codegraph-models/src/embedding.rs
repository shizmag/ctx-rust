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
    let names: Vec<String> = session.inputs().iter().map(|input| input.name().to_string()).collect();

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