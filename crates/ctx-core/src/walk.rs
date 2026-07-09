use ignore::{Walk, WalkBuilder};
use std::path::{Path, PathBuf};

pub fn setup_walker(root_path: &Path) -> Walk {
    WalkBuilder::new(root_path)
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .build()
}

pub fn is_inside_pruned_dir(path: &Path, pruned_dirs: &[PathBuf]) -> bool {
    pruned_dirs
        .iter()
        .any(|dir| path != dir && path.starts_with(dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn is_inside_pruned_dir_returns_false_for_empty_list() {
        let path = PathBuf::from("/tmp/project/src/main.rs");
        assert!(!is_inside_pruned_dir(&path, &[]));
    }

    #[test]
    fn is_inside_pruned_dir_returns_false_when_path_equals_pruned_dir() {
        let pruned = PathBuf::from("/tmp/project/target");
        assert!(!is_inside_pruned_dir(&pruned, &[pruned.clone()]));
    }

    #[test]
    fn is_inside_pruned_dir_returns_true_for_nested_path() {
        let pruned = PathBuf::from("/tmp/project/target");
        let nested = PathBuf::from("/tmp/project/target/debug/app");
        assert!(is_inside_pruned_dir(&nested, &[pruned]));
    }

    #[test]
    fn is_inside_pruned_dir_returns_false_for_unrelated_path() {
        let pruned = PathBuf::from("/tmp/project/target");
        let other = PathBuf::from("/tmp/project/src/main.rs");
        assert!(!is_inside_pruned_dir(&other, &[pruned]));
    }

    #[test]
    fn setup_walker_visits_all_entries_including_hidden() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join(".hidden_file"), "secret\n").unwrap();

        let walker = setup_walker(root);
        let paths: Vec<_> = walker
            .filter_map(Result::ok)
            .map(|entry| entry.path().to_path_buf())
            .collect();

        assert!(paths.contains(&root.join("src")));
        assert!(paths.contains(&root.join("src/main.rs")));
        assert!(paths.contains(&root.join(".hidden_file")));
    }
}
