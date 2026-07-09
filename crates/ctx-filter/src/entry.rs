use std::path::{Path, PathBuf};

use ctx_models::NodeKind;

#[derive(Debug, Clone)]
pub struct FilterEntry {
    pub path: PathBuf,
    pub name: String,
    pub kind: NodeKind,
    pub depth: usize,
    pub bytes: Option<u64>,
}

impl FilterEntry {
    pub fn new(path: PathBuf, kind: NodeKind, depth: usize, bytes: Option<u64>) -> Self {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();

        Self {
            path,
            name,
            kind,
            depth,
            bytes,
        }
    }

    pub fn extension(&self) -> Option<&str> {
        Path::new(&self.name)
            .extension()
            .and_then(|name| name.to_str())
    }

    pub fn is_dir(&self) -> bool {
        self.kind == NodeKind::Directory
    }

    pub fn is_file(&self) -> bool {
        self.kind == NodeKind::File
    }

    pub fn is_symlink(&self) -> bool {
        self.kind == NodeKind::Symlink
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_models::NodeKind;

    #[test]
    fn name_is_basename_of_path() {
        let entry = FilterEntry::new(
            PathBuf::from("src/lib/main.rs"),
            NodeKind::File,
            2,
            None,
        );
        assert_eq!(entry.name, "main.rs");
        assert_eq!(entry.depth, 2);
    }

    #[test]
    fn extension_returns_final_suffix() {
        let entry = FilterEntry::new(PathBuf::from("archive.tar.gz"), NodeKind::File, 0, None);
        assert_eq!(entry.extension(), Some("gz"));
    }

    #[test]
    fn extension_is_none_for_extensionless_names() {
        let entry = FilterEntry::new(PathBuf::from("Makefile"), NodeKind::File, 0, None);
        assert_eq!(entry.extension(), None);
    }

    #[test]
    fn extension_is_none_for_dotfiles_without_suffix() {
        let entry = FilterEntry::new(PathBuf::from(".gitignore"), NodeKind::File, 0, None);
        assert_eq!(entry.extension(), None);
    }

    #[test]
    fn bytes_field_is_preserved() {
        let entry = FilterEntry::new(PathBuf::from("data.bin"), NodeKind::File, 0, Some(4096));
        assert_eq!(entry.bytes, Some(4096));
    }

    #[test]
    fn kind_predicates_match_node_kind() {
        let file = FilterEntry::new(PathBuf::from("a.rs"), NodeKind::File, 0, None);
        let dir = FilterEntry::new(PathBuf::from("src"), NodeKind::Directory, 0, None);
        let link = FilterEntry::new(PathBuf::from("link"), NodeKind::Symlink, 0, None);

        assert!(file.is_file());
        assert!(!file.is_dir());
        assert!(!file.is_symlink());

        assert!(dir.is_dir());
        assert!(!dir.is_file());

        assert!(link.is_symlink());
        assert!(!link.is_file());
    }
}
