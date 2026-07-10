use ctx_codegraph_lang::error::CodeGraphError;
use ctx_codegraph_lang::index::BuildIndexOptions;
use ctx_codegraph_lang::model::{
    AffectedSet, BuildReport, CodeIndex, IndexDiff, IndexState, RebuildReason,
};
use std::path::{Path, PathBuf};

pub use ctx_codegraph_store::storage::{
    find_symbols, load_callees, load_callers, load_edges_for_symbol, load_edges_from,
    load_edges_to, load_file_span, load_index, load_occurrence, load_symbol, load_symbols_by_ids,
    load_symbols_for_file, resolve_symbol, save_index, validate_index_invariants,
};

pub use ctx_codegraph_store::storage::{
    StagedFileUpdate, check_db_compatibility_with_registry, clear_index_with_registry,
    compute_affected_set_with_registry, compute_index_diff_with_registry,
    ensure_index_with_registry, get_index_state_with_registry, rebuild_index_db_with_registry,
    run_full_rebuild_with_registry, run_incremental_update_with_registry,
    validate_index_db_with_registry,
};

use crate::registry::global_registry;

pub fn init_schema(conn: &rusqlite::Connection) -> Result<(), CodeGraphError> {
    ctx_codegraph_store::storage::init_schema(conn, global_registry())
}

pub fn find_workspace_root(start_dir: &Path) -> PathBuf {
    ctx_codegraph_store::storage::find_workspace_root(start_dir, global_registry())
}

pub fn open_codegraph_db(root: &Path) -> Result<rusqlite::Connection, CodeGraphError> {
    ctx_codegraph_store::storage::open_codegraph_db(root, global_registry())
}

pub fn open_db(root: &Path) -> Result<rusqlite::Connection, CodeGraphError> {
    ctx_codegraph_store::storage::open_db(root, global_registry())
}

pub fn write_metadata(root: &Path, key: &str, value: &str) -> Result<(), CodeGraphError> {
    ctx_codegraph_store::storage::write_metadata(root, global_registry(), key, value)
}

pub fn read_metadata(root: &Path, key: &str) -> Option<String> {
    ctx_codegraph_store::storage::read_metadata(root, global_registry(), key)
}

pub fn check_db_compatibility(
    conn: &rusqlite::Connection,
    options: &BuildIndexOptions,
) -> Result<Option<RebuildReason>, CodeGraphError> {
    check_db_compatibility_with_registry(conn, options, global_registry())
}

pub fn compute_index_diff(
    conn: &rusqlite::Connection,
    workspace_root: &Path,
    options: &BuildIndexOptions,
) -> Result<IndexDiff, CodeGraphError> {
    compute_index_diff_with_registry(conn, workspace_root, options, global_registry())
}

pub fn get_index_state(
    root: &Path,
    options: &BuildIndexOptions,
) -> Result<IndexState, CodeGraphError> {
    get_index_state_with_registry(root, options, global_registry())
}

pub fn validate_index_db(root: &Path, options: &BuildIndexOptions) -> Result<bool, CodeGraphError> {
    validate_index_db_with_registry(root, options, global_registry())
}

pub fn clear_index(conn: &mut rusqlite::Connection) -> Result<(), CodeGraphError> {
    clear_index_with_registry(conn, global_registry())
}

pub fn compute_affected_set(
    conn: &rusqlite::Connection,
    diff: &IndexDiff,
    staged: &[StagedFileUpdate],
) -> Result<AffectedSet, CodeGraphError> {
    compute_affected_set_with_registry(conn, diff, staged, global_registry())
}

pub fn rebuild_index_db(
    root: &Path,
    options: BuildIndexOptions,
) -> Result<(CodeIndex, BuildReport), CodeGraphError> {
    rebuild_index_db_with_registry(root, options, global_registry())
}

pub fn ensure_index(
    root: &Path,
    options: BuildIndexOptions,
) -> Result<rusqlite::Connection, CodeGraphError> {
    ensure_index_with_registry(root, options, global_registry())
}

pub fn run_full_rebuild(
    conn: &mut rusqlite::Connection,
    workspace_root: &Path,
    options: BuildIndexOptions,
    reason: Option<RebuildReason>,
) -> Result<(CodeIndex, BuildReport), CodeGraphError> {
    run_full_rebuild_with_registry(
        conn,
        workspace_root,
        options,
        reason,
        global_registry(),
    )
}

pub fn run_incremental_update(
    conn: &mut rusqlite::Connection,
    workspace_root: &Path,
    options: BuildIndexOptions,
    diff: IndexDiff,
) -> Result<(CodeIndex, BuildReport), CodeGraphError> {
    run_incremental_update_with_registry(
        conn,
        workspace_root,
        options,
        diff,
        global_registry(),
    )
}