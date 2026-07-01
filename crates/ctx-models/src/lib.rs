use std::path::PathBuf;

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
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            mode: Mode::Smart,
            max_depth: None,
            max_file_size: 512 * 1024,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone, Default)]
pub struct NodeStats {
    pub files: usize,
    pub dirs: usize,
    pub lines: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub childrens: Vec<TreeNode>,
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
