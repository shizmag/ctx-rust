#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("walk error: {0}")]
    Walk(#[from] ignore::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
