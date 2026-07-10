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

#[cfg(test)]
mod tests {
    use super::should_skip_dir;

    #[test]
    fn skips_common_vendor_and_build_dirs() {
        for name in [
            "target",
            ".git",
            "node_modules",
            "__pycache__",
            ".venv",
            "build",
            "dist",
        ] {
            assert!(should_skip_dir(name), "expected skip for {name}");
        }
    }

    #[test]
    fn does_not_skip_source_dirs() {
        for name in ["src", "lib", "tests", "crates", "my-project"] {
            assert!(!should_skip_dir(name), "expected no skip for {name}");
        }
    }
}