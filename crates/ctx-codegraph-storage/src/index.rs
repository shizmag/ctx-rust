use ctx_codegraph_lang::error::CodeGraphError;
use ctx_codegraph_lang::model::{CodeIndex, FileSnapshot};
use std::path::Path;

pub use ctx_codegraph_lang::index::{
    BuildIndexOptions, build_index_with_registry, compute_file_hash,
    create_file_snapshot_with_registry, get_mtime_ms, get_size_bytes,
    should_index_path_with_registry,
};

use crate::registry::global_registry;

pub fn create_file_snapshot(
    workspace_root: &Path,
    abs_path: &Path,
    change_detection: crate::model::FileChangeDetection,
    include_tests: bool,
) -> FileSnapshot {
    create_file_snapshot_with_registry(
        workspace_root,
        abs_path,
        change_detection,
        include_tests,
        global_registry(),
    )
}

pub fn build_index(root: &Path, options: BuildIndexOptions) -> Result<CodeIndex, CodeGraphError> {
    build_index_with_registry(root, options, global_registry())
}