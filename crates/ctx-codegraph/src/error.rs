#[derive(Debug, thiserror::Error)]
pub enum CodeGraphError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("tree-sitter parse error: {0}")]
    Parse(String),

    #[error("lsp error: {0}")]
    Lsp(String),

    #[error("symbol not found: {0}")]
    SymbolNotFound(String),

    #[error("ambiguous symbol: {0}")]
    AmbiguousSymbol(String),
}
