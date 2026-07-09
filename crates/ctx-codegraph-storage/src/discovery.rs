/// Returns true when a directory name should be excluded from indexing walks.
pub fn should_skip_dir(name: &str) -> bool {
    matches!(
        name,
        "target"
            | ".git"
            | ".codegraph"
            | ".ctx-codegraph"
            | ".venv"
            | "venv"
            | ".env"
            | "node_modules"
            | "__pycache__"
            | "build"
            | "dist"
    )
}