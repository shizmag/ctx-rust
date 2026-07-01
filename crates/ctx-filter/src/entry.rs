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
