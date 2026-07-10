use std::path::Path;

use tokenizers::Tokenizer;

use crate::error::ModelError;

const MAX_TOKENS: usize = 512;

#[derive(Debug)]
enum TokenizerBackend {
    HuggingFace(Tokenizer),
    Simple(SimpleTokenizer),
}

#[derive(Debug, Default)]
struct SimpleTokenizer;

impl SimpleTokenizer {
    fn encode(&self, text: &str) -> (Vec<i64>, Vec<i64>) {
        let tokens: Vec<i64> = text
            .split_whitespace()
            .take(MAX_TOKENS)
            .enumerate()
            .map(|(idx, _)| idx as i64 + 1)
            .collect();
        let attention_mask = vec![1_i64; tokens.len()];
        (tokens, attention_mask)
    }

    fn encode_batch(&self, texts: &[String]) -> Vec<(Vec<i64>, Vec<i64>)> {
        texts.iter().map(|text| self.encode(text)).collect()
    }
}

/// Tokenizer for code/text inputs used by embedding and reranker models.
pub struct CodeTokenizer {
    backend: TokenizerBackend,
}

impl CodeTokenizer {
    pub fn from_dir(tokenizer_dir: &Path) -> Result<Self, ModelError> {
        let tokenizer_path = tokenizer_dir.join("tokenizer.json");
        if tokenizer_path.exists() {
            let tokenizer = Tokenizer::from_file(&tokenizer_path)
                .map_err(|e| ModelError::Tokenizer(e.to_string()))?;
            Ok(Self {
                backend: TokenizerBackend::HuggingFace(tokenizer),
            })
        } else {
            Ok(Self {
                backend: TokenizerBackend::Simple(SimpleTokenizer),
            })
        }
    }

    pub fn encode(&self, text: &str) -> Result<(Vec<i64>, Vec<i64>), ModelError> {
        match &self.backend {
            TokenizerBackend::HuggingFace(tokenizer) => {
                let encoding = tokenizer
                    .encode(text, true)
                    .map_err(|e| ModelError::Tokenizer(e.to_string()))?;
                let mut ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
                if ids.len() > MAX_TOKENS {
                    ids.truncate(MAX_TOKENS);
                }
                let attention_mask = vec![1_i64; ids.len()];
                Ok((ids, attention_mask))
            }
            TokenizerBackend::Simple(simple) => Ok(simple.encode(text)),
        }
    }

    pub fn encode_batch(&self, texts: &[String]) -> Result<Vec<(Vec<i64>, Vec<i64>)>, ModelError> {
        match &self.backend {
            TokenizerBackend::HuggingFace(_) => texts
                .iter()
                .map(|text| self.encode(text))
                .collect(),
            TokenizerBackend::Simple(simple) => Ok(simple.encode_batch(texts)),
        }
    }

    pub fn encode_pair(&self, left: &str, right: &str) -> Result<(Vec<i64>, Vec<i64>), ModelError> {
        match &self.backend {
            TokenizerBackend::HuggingFace(tokenizer) => {
                let encoding = tokenizer
                    .encode((left, right), true)
                    .map_err(|e| ModelError::Tokenizer(e.to_string()))?;
                let mut ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
                if ids.len() > MAX_TOKENS {
                    ids.truncate(MAX_TOKENS);
                }
                let attention_mask = vec![1_i64; ids.len()];
                Ok((ids, attention_mask))
            }
            TokenizerBackend::Simple(simple) => {
                let combined = format!("{left} {right}");
                Ok(simple.encode(&combined))
            }
        }
    }
}