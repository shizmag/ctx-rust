use std::path::PathBuf;

pub mod util;
pub use util::{format_bytes, get_relative_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Smart,
    All,
    Code,
    Docs,
    Llm,
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub max_depth: Option<usize>,
    pub max_file_size: u64,
    pub mode: Mode,
    pub exclude: Vec<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            mode: Mode::Smart,
            max_depth: None,
            max_file_size: 512 * 1024,
            exclude: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Directory = 0,
    File = 1,
    Symlink = 2,
    Other = 3,
}

#[derive(Debug, Clone, Default)]
pub struct NodeStats {
    pub files: usize,
    pub dirs: usize,
    pub lines: usize,
    pub bytes: u64,
    pub tokens: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileStats {
    pub lines: usize,
    pub bytes: u64,
    pub tokens: usize,
    pub is_text: bool,
    pub skipped_reason: Option<StatsSkipReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatsSkipReason {
    TooLarge,
    NonUtf8,
    NotAFile,
}

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub children: Vec<TreeNode>,
    pub kind: NodeKind,
    pub name: String,
    pub path: PathBuf,
    pub stats: NodeStats,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectSummary {
    pub files: usize,
    pub dirs: usize,
    pub lines: usize,
    pub bytes: u64,
    pub tokens: usize,
    pub hidden_files: usize,
    pub hidden_dirs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HiddenReason {
    VcsInternals,
    Dependencies,
    BuildArtifacts,
    Cache,
    Lockfile,
    Temporary,
    Generated,
    LargeFile,
    Binary,
    Gitignored,
    NonCode,
    NonDocs,
}

impl HiddenReason {
    pub fn label(&self) -> &'static str {
        match self {
            HiddenReason::VcsInternals => "VCS internals",
            HiddenReason::Dependencies => "dependencies",
            HiddenReason::BuildArtifacts => "build artifacts",
            HiddenReason::Cache => "cache",
            HiddenReason::Lockfile => "lockfile",
            HiddenReason::Temporary => "temporary file",
            HiddenReason::Generated => "generated file",
            HiddenReason::LargeFile => "large file",
            HiddenReason::Binary => "binary file",
            HiddenReason::Gitignored => "gitignored",
            HiddenReason::NonCode => "non-code file",
            HiddenReason::NonDocs => "non-document file",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HiddenItem {
    pub reason: HiddenReason,
    pub path: PathBuf,
    pub is_dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visibility {
    Visible,
    Hidden(HiddenReason),
}

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub root: TreeNode,
    pub summary: ProjectSummary,
    pub hidden: Vec<HiddenItem>,
}

#[derive(Debug, Clone)]
pub struct TreeLine<'a> {
    pub node: &'a TreeNode,
    pub prefix: String,
    pub is_root: bool,
    pub is_last: bool,
    pub depth: usize,
}

pub fn walk_tree_lines<'a, F>(root: &'a TreeNode, mut callback: F)
where
    F: FnMut(&TreeLine<'a>) -> bool,
{
    fn walk<'a, F>(
        node: &'a TreeNode,
        prefix: &str,
        is_last: bool,
        is_root: bool,
        depth: usize,
        callback: &mut F,
    ) where
        F: FnMut(&TreeLine<'a>) -> bool,
    {
        let line = TreeLine {
            node,
            prefix: prefix.to_string(),
            is_root,
            is_last,
            depth,
        };

        let descend = callback(&line);
        if !descend {
            return;
        }

        let next_prefix = if is_root {
            "".to_string()
        } else {
            format!("{}{}", prefix, if is_last { "    " } else { "│   " })
        };

        let count = node.children.len();
        for (i, child) in node.children.iter().enumerate() {
            let child_is_last = i == count - 1;
            walk(
                child,
                &next_prefix,
                child_is_last,
                false,
                depth + 1,
                callback,
            );
        }
    }

    walk(root, "", true, true, 0, &mut callback);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSkipReason {
    TooLarge,
    NonUtf8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileContentResult {
    Text(String),
    Skipped(FileSkipReason),
    ReadError(String),
}

pub fn read_file_content(path: &std::path::Path, max_file_size: u64) -> FileContentResult {
    let metadata = match path.metadata() {
        Ok(m) => m,
        Err(err) => return FileContentResult::ReadError(err.to_string()),
    };

    if !metadata.is_file() {
        return FileContentResult::ReadError("Not a file".to_string());
    }

    if metadata.len() > max_file_size {
        return FileContentResult::Skipped(FileSkipReason::TooLarge);
    }

    match std::fs::read_to_string(path) {
        Ok(content) => FileContentResult::Text(content),
        Err(err) => {
            if err.kind() == std::io::ErrorKind::InvalidData {
                FileContentResult::Skipped(FileSkipReason::NonUtf8)
            } else {
                FileContentResult::ReadError(err.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_node(name: &str, kind: NodeKind, children: Vec<TreeNode>) -> TreeNode {
        TreeNode {
            name: name.to_string(),
            path: PathBuf::from(name),
            kind,
            stats: NodeStats::default(),
            children,
        }
    }

    #[test]
    fn test_walk_one_directory() {
        let root = create_test_node("root", NodeKind::Directory, vec![]);
        let mut lines = Vec::new();
        walk_tree_lines(&root, |line| {
            lines.push(line.clone());
            true
        });

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].node.name, "root");
        assert_eq!(lines[0].prefix, "");
        assert!(lines[0].is_root);
        assert!(lines[0].is_last);
        assert_eq!(lines[0].depth, 0);
    }

    #[test]
    fn test_walk_several_files() {
        let root = create_test_node(
            "root",
            NodeKind::Directory,
            vec![
                create_test_node("a.txt", NodeKind::File, vec![]),
                create_test_node("b.txt", NodeKind::File, vec![]),
            ],
        );

        let mut lines = Vec::new();
        walk_tree_lines(&root, |line| {
            lines.push(line.clone());
            true
        });

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].node.name, "root");
        assert!(lines[0].is_root);

        assert_eq!(lines[1].node.name, "a.txt");
        assert_eq!(lines[1].prefix, "");
        assert!(!lines[1].is_root);
        assert!(!lines[1].is_last);
        assert_eq!(lines[1].depth, 1);

        assert_eq!(lines[2].node.name, "b.txt");
        assert_eq!(lines[2].prefix, "");
        assert!(!lines[2].is_root);
        assert!(lines[2].is_last);
        assert_eq!(lines[2].depth, 1);
    }

    #[test]
    fn test_walk_nested_directories() {
        let root = create_test_node(
            "root",
            NodeKind::Directory,
            vec![create_test_node(
                "sub",
                NodeKind::Directory,
                vec![create_test_node("file.txt", NodeKind::File, vec![])],
            )],
        );

        let mut lines = Vec::new();
        walk_tree_lines(&root, |line| {
            lines.push(line.clone());
            true
        });

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].node.name, "root");
        assert_eq!(lines[1].node.name, "sub");
        assert_eq!(lines[1].prefix, "");
        assert!(lines[1].is_last);

        assert_eq!(lines[2].node.name, "file.txt");
        assert_eq!(lines[2].prefix, "    ");
        assert!(lines[2].is_last);
        assert_eq!(lines[2].depth, 2);
    }

    #[test]
    fn test_sorting_and_stability() {
        // Build children in unsorted order: file first, then directory, then file
        let mut children = vec![
            create_test_node("z.txt", NodeKind::File, vec![]),
            create_test_node("sub_b", NodeKind::Directory, vec![]),
            create_test_node("sub_a", NodeKind::Directory, vec![]),
            create_test_node("a.txt", NodeKind::File, vec![]),
        ];

        // Apply same sorting logic as TreeBuilder
        children.sort_by(|a, b| {
            match (a.kind == NodeKind::Directory, b.kind == NodeKind::Directory) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });

        let root = create_test_node("root", NodeKind::Directory, children);

        let mut lines = Vec::new();
        walk_tree_lines(&root, |line| {
            lines.push(line.clone());
            true
        });

        // Sorted order should be: directories first (sub_a, sub_b), then files (a.txt, z.txt)
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[1].node.name, "sub_a");
        assert_eq!(lines[1].node.kind, NodeKind::Directory);

        assert_eq!(lines[2].node.name, "sub_b");
        assert_eq!(lines[2].node.kind, NodeKind::Directory);

        assert_eq!(lines[3].node.name, "a.txt");
        assert_eq!(lines[3].node.kind, NodeKind::File);

        assert_eq!(lines[4].node.name, "z.txt");
        assert_eq!(lines[4].node.kind, NodeKind::File);
    }

    #[test]
    fn test_read_file_content_small_utf8() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("small.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let res = read_file_content(&file_path, 1024);
        match res {
            FileContentResult::Text(content) => assert_eq!(content, "hello world"),
            other => panic!("Expected FileContentResult::Text, got {:?}", other),
        }
    }

    #[test]
    fn test_read_file_content_empty() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("empty.txt");
        std::fs::write(&file_path, "").unwrap();

        let res = read_file_content(&file_path, 1024);
        match res {
            FileContentResult::Text(content) => assert_eq!(content, ""),
            other => panic!("Expected FileContentResult::Text, got {:?}", other),
        }
    }

    #[test]
    fn test_read_file_content_too_large() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("large.txt");
        std::fs::write(&file_path, "some content that is larger than 5 bytes").unwrap();

        let res = read_file_content(&file_path, 5);
        match res {
            FileContentResult::Skipped(FileSkipReason::TooLarge) => {}
            other => panic!(
                "Expected FileContentResult::Skipped(FileSkipReason::TooLarge), got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_read_file_content_non_utf8() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("binary.bin");
        std::fs::write(&file_path, vec![0, 159, 146, 150]).unwrap(); // Invalid UTF-8 bytes

        let res = read_file_content(&file_path, 1024);
        match res {
            FileContentResult::Skipped(FileSkipReason::NonUtf8) => {}
            other => panic!(
                "Expected FileContentResult::Skipped(FileSkipReason::NonUtf8), got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_read_file_content_missing() {
        let path = PathBuf::from("non_existent_file_12345.txt");
        let res = read_file_content(&path, 1024);
        match res {
            FileContentResult::ReadError(_) => {}
            other => panic!("Expected FileContentResult::ReadError, got {:?}", other),
        }
    }
}
