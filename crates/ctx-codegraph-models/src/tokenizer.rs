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

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_tokenizer() -> CodeTokenizer {
        let dir = tempfile::tempdir().unwrap();
        CodeTokenizer::from_dir(dir.path()).unwrap()
    }

    #[test]
    fn from_dir_without_tokenizer_json_uses_simple_backend() {
        let dir = tempfile::tempdir().unwrap();
        let tokenizer = CodeTokenizer::from_dir(dir.path()).unwrap();
        let (ids, mask) = tokenizer.encode("alpha beta").unwrap();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(mask, vec![1, 1]);
    }

    #[test]
    fn encode_assigns_incrementing_token_ids_per_word() {
        let tokenizer = simple_tokenizer();
        let (ids, mask) = tokenizer.encode("one two three").unwrap();
        assert_eq!(ids, vec![1, 2, 3]);
        assert_eq!(mask, vec![1, 1, 1]);
    }

    #[test]
    fn encode_batch_encodes_each_text() {
        let tokenizer = simple_tokenizer();
        let texts = vec!["a b".to_string(), "c".to_string()];
        let batch = tokenizer.encode_batch(&texts).unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].0, vec![1, 2]);
        assert_eq!(batch[1].0, vec![1]);
    }

    #[test]
    fn encode_pair_combines_inputs_for_simple_backend() {
        let tokenizer = simple_tokenizer();
        let (ids, mask) = tokenizer.encode_pair("query", "document body").unwrap();
        assert_eq!(ids, vec![1, 2, 3]);
        assert_eq!(mask, vec![1, 1, 1]);
    }

    #[test]
    fn encode_truncates_simple_backend_at_max_tokens() {
        let tokenizer = simple_tokenizer();
        let words: Vec<&str> = (0..600).map(|_| "word").collect();
        let text = words.join(" ");
        let (ids, mask) = tokenizer.encode(&text).unwrap();
        assert_eq!(ids.len(), MAX_TOKENS);
        assert_eq!(mask.len(), MAX_TOKENS);
        assert_eq!(ids[0], 1);
        assert_eq!(ids[MAX_TOKENS - 1], MAX_TOKENS as i64);
    }

    #[test]
    fn from_dir_with_invalid_tokenizer_json_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tokenizer.json"), "not valid json").unwrap();

        let result = CodeTokenizer::from_dir(dir.path());
        assert!(matches!(result, Err(ModelError::Tokenizer(_))));
    }

    fn write_minimal_hf_tokenizer(dir: &std::path::Path) {
        let json = r#"{
            "version": "1.0",
            "truncation": null,
            "padding": null,
            "added_tokens": [],
            "normalizer": null,
            "pre_tokenizer": { "type": "Whitespace" },
            "post_processor": null,
            "decoder": null,
            "model": {
                "type": "WordLevel",
                "vocab": { "<unk>": 0, "hello": 1, "world": 2, "query": 3, "doc": 4 },
                "unk_token": "<unk>"
            }
        }"#;
        std::fs::write(dir.join("tokenizer.json"), json).unwrap();
    }

    #[test]
    fn from_dir_with_tokenizer_json_uses_huggingface_backend() {
        let dir = tempfile::tempdir().unwrap();
        write_minimal_hf_tokenizer(dir.path());

        let tokenizer = CodeTokenizer::from_dir(dir.path()).unwrap();
        let (ids, mask) = tokenizer.encode("hello world").unwrap();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(mask, vec![1, 1]);
    }

    #[test]
    fn huggingface_encode_pair_tokenizes_both_segments() {
        let dir = tempfile::tempdir().unwrap();
        write_minimal_hf_tokenizer(dir.path());

        let tokenizer = CodeTokenizer::from_dir(dir.path()).unwrap();
        let (ids, mask) = tokenizer.encode_pair("query", "doc").unwrap();
        assert!(!ids.is_empty());
        assert_eq!(ids.len(), mask.len());
        assert!(ids.contains(&3));
        assert!(ids.contains(&4));
    }

    #[test]
    fn huggingface_encode_batch_encodes_each_text() {
        let dir = tempfile::tempdir().unwrap();
        write_minimal_hf_tokenizer(dir.path());

        let tokenizer = CodeTokenizer::from_dir(dir.path()).unwrap();
        let texts = vec!["hello".to_string(), "world".to_string()];
        let batch = tokenizer.encode_batch(&texts).unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].0, vec![1]);
        assert_eq!(batch[1].0, vec![2]);
    }
}