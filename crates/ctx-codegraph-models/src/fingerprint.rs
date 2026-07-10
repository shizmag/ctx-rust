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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn file_fingerprint_matches_sha256_hex_digest() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("sample.txt");
        let mut file = std::fs::File::create(&file_path).unwrap();
        write!(file, "ctx-codegraph-models").unwrap();

        let digest = file_fingerprint(&file_path).unwrap();
        assert_eq!(
            digest,
            "b86138d3c1d9ace53b17a2bf014e2292dd97dd512e6434b8e9fdfe9b8cdb56d9"
        );
    }
}