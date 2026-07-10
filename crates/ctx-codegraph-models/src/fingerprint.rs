use sha2::{Digest, Sha256};
use std::path::Path;

use crate::error::ModelError;

/// Returns the SHA-256 hex digest of the file at `path`.
pub fn file_fingerprint(path: &Path) -> Result<String, ModelError> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}