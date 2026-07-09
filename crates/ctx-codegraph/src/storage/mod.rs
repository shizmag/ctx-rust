mod compat;
mod diff;
mod persist;
mod query;
mod rebuild;
mod schema;
mod workspace;

pub use compat::{check_db_compatibility, check_db_compatibility_with_registry};
pub use diff::{
    compute_index_diff, compute_index_diff_with_registry, get_index_state,
    get_index_state_with_registry, validate_index_db, validate_index_db_with_registry,
};
pub use persist::{clear_index, clear_index_with_registry, load_index, save_index};
pub use query::{
    find_symbols, load_callees, load_callers, load_edges_for_symbol, load_edges_from,
    load_edges_to, load_file_span, load_occurrence, load_symbol, load_symbols_by_ids,
    load_symbols_for_file, resolve_symbol,
};
pub use rebuild::{
    StagedFileUpdate, compute_affected_set, compute_affected_set_with_registry, rebuild_index_db,
    rebuild_index_db_with_registry, run_full_rebuild, run_incremental_update,
    run_incremental_update_with_registry,
};
pub use schema::{init_schema, validate_index_invariants};
pub use workspace::{find_workspace_root, open_codegraph_db, open_db};
