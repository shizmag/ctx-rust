use ctx_codegraph::index::BuildIndexOptions;
use ctx_codegraph::model::IndexState;
use ctx_codegraph::service::GraphContextService;
use ctx_codegraph::storage::{find_workspace_root, get_index_state};
use ctx_core::scan;
use ctx_models::{Mode, ScanOptions};
use std::fmt::Write as _;

use super::tools::{usage_stats_json, usage_summary_text};  // enhanced for full queryable dump + summary (enable collection/comparisons vs other MCPs)

pub const INDEX_STATUS_URI: &str = "ctx://index/status";
pub const PROJECT_TREE_URI: &str = "ctx://project/tree";
pub const MCP_STATS_URI: &str = "ctx://stats/mcp";  // queryable stats resource for dumping metrics data

pub fn list_resources() -> serde_json::Value {
    serde_json::json!({
        "resources": [
            {
                "uri": INDEX_STATUS_URI,
                "name": "Index Status",
                "description": "Codegraph index build status and metadata",
                "mimeType": "text/markdown"
            },
            {
                "uri": PROJECT_TREE_URI,
                "name": "Project Tree",
                "description": "Brief project tree summary",
                "mimeType": "text/markdown"
            },
            {
                "uri": MCP_STATS_URI,
                "name": "MCP Usage Stats",
                "description": "Comprehensive session metrics for ctx MCP comparisons (call counts, tokens, timings, errors, context sizes, formats etc.)",
                "mimeType": "application/json"
            }
        ]
    })
}

pub fn read_resource(service: &GraphContextService, uri: &str) -> Result<String, String> {
    match uri {
        INDEX_STATUS_URI => read_index_status(service),
        PROJECT_TREE_URI => read_project_tree(service),
        MCP_STATS_URI => read_mcp_stats(),
        _ => Err(format!("Unknown resource: {}", uri)),
    }
}

fn read_index_status(service: &GraphContextService) -> Result<String, String> {
    let root = service.repo_root();
    let db_path = root.join(".ctx-codegraph/codegraph.sqlite");
    let options = BuildIndexOptions {
        use_lsp: false,
        ..Default::default()
    };

    let state = get_index_state(root, &options).unwrap_or(IndexState::Missing);
    let conn = service.lock_conn();

    let meta_value = |key: &str| -> Option<String> {
        conn.query_row("SELECT value FROM metadata WHERE key = ?", [key], |row| {
            row.get::<_, String>(0)
        })
        .ok()
    };

    let file_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
        .unwrap_or(0);
    let symbol_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
        .unwrap_or(0);
    let edge_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))
        .unwrap_or(0);

    let mut out = String::new();
    writeln!(out, "# Codegraph Index Status\n").unwrap();
    writeln!(out, "- Workspace: `{}`", root.display()).unwrap();
    writeln!(out, "- Database: `{}`", db_path.display()).unwrap();
    writeln!(out, "- State: `{:?}`", state).unwrap();
    writeln!(out, "- Files indexed: {}", file_count).unwrap();
    writeln!(out, "- Symbols: {}", symbol_count).unwrap();
    writeln!(out, "- Edges: {}", edge_count).unwrap();

    if let Some(version) = meta_value("schema_version") {
        writeln!(out, "- Schema version: {}", version).unwrap();
    }
    if let Some(resolver) = meta_value("resolver_id") {
        writeln!(out, "- Resolver: {}", resolver).unwrap();
    }
    if let Some(strategy) = meta_value("change_detection_strategy") {
        writeln!(out, "- Change detection: {}", strategy).unwrap();
    }

    if matches!(state, IndexState::Missing | IndexState::NeedsFullRebuild(_)) {
        writeln!(
            out,
            "\n> Index may be missing or stale. Run `ctx graph build --with-lsp` or use the `rebuild_index` tool."
        )
        .unwrap();
    }

    // Enhanced: surface MCP usage metrics via existing resource for collectability/comparisons
    writeln!(out, "\n{}", usage_summary_text()).unwrap();

    Ok(out)
}

fn read_project_tree(service: &GraphContextService) -> Result<String, String> {
    let root = service.repo_root();
    let scan_options = ScanOptions {
        max_depth: Some(4),
        max_file_size: 512 * 1024,
        mode: Mode::Smart,
        exclude: Vec::new(),
    };

    let scan_result = scan(root, scan_options)
        .map_err(|e| format!("Failed to scan project tree: {}", e))?;

    let mut out = String::new();
    writeln!(out, "# Project Tree Summary\n").unwrap();
    writeln!(out, "- Root: `{}`", root.display()).unwrap();
    writeln!(out, "- Files: {}", scan_result.summary.files).unwrap();
    writeln!(out, "- Directories: {}", scan_result.summary.dirs).unwrap();
    writeln!(out, "- Total lines: {}", scan_result.summary.lines).unwrap();
    writeln!(out, "- Estimated tokens: {}\n", scan_result.summary.tokens).unwrap();
    writeln!(out, "## Top-level entries\n").unwrap();

    for child in &scan_result.root.children {
        let kind = match child.kind {
            ctx_models::NodeKind::Directory => "dir",
            ctx_models::NodeKind::File => "file",
            _ => "other",
        };
        writeln!(
            out,
            "- {} `{}` ({} files, {} dirs)",
            kind,
            child.name,
            child.stats.files,
            child.stats.dirs
        )
        .unwrap();
    }

    let workspace = find_workspace_root(root);
    if workspace != root {
        writeln!(out, "\n- Workspace root: `{}`", workspace.display()).unwrap();
    }

    Ok(out)
}

/// Read full MCP stats as JSON text (queryable dump for metrics collection and ctx vs other MCP comparison).
fn read_mcp_stats() -> Result<String, String> {
    let json = usage_stats_json();
    let pretty = serde_json::to_string_pretty(&json)
        .unwrap_or_else(|_| json.to_string());
    Ok(format!("# MCP Usage Stats (JSON dump)\n\n```json\n{}\n```", pretty))
}