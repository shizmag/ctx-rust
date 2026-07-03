#[derive(Debug, thiserror::Error)]
pub enum TuiError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("scan error: {0}")]
    Scan(#[from] ctx_core::ScanError),

    #[error("clipboard error: {0}")]
    Clipboard(#[from] arboard::Error),
}
