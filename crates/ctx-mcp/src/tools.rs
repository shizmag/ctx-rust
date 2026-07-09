use super::protocol::{
    depth_or_auto, get_bool_arg, get_str_arg, get_string_array, get_usize_arg, parse_edge_kinds,
    parse_graph_context_mode, text_tool_result,
};
use super::render::{
    format_ambiguous_symbols_json, format_ambiguous_symbols_yaml, format_symbol_not_found,
    handle_symbol_resolution, render_affected_context_json, render_affected_context_text,
    render_affected_context_yaml, render_call_edges, render_caller_edges, render_context_to_json,
    render_context_to_markdown, render_context_to_yaml, render_symbols_list,
};
use ctx_codegraph::index::BuildIndexOptions;
use ctx_codegraph::model::{FileChangeDetection, GraphContextOptions, SymbolResolution};
use ctx_codegraph::service::GraphContextService;
use ctx_codegraph::storage::{load_callees, load_callers, rebuild_index_db};
use ctx_codegraph::{
    ContextBudget, ContextPackingMode, ContextRetrievalOptions, RankingMode,
    retrieve_graph_context_with_options,
};
use ctx_config::find_and_load_config;
use ctx_core::scan;
use ctx_models::{Mode, ScanOptions};
use ctx_render::{Format, RenderOptions};

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Simple in-memory usage metrics for MCP server (session scoped).
/// Comprehensive for comparing ctx MCP vs other MCP servers:
/// - per-tool call counts, success/error rates
/// - time per call (durations)
/// - token usage (input est via approx + context output est where available)
/// - ambiguous disambig events, rebuilds via MCP
/// - context sizes (nodes, omitted due to budget)
/// - format used (text/json/yaml)
/// Toggle via env CTX_MCP_COLLECT_STATS=0 to disable (integrates with settings).
#[derive(Default, Debug, Clone, serde::Serialize)]
pub(crate) struct UsageStats {
    pub tool_calls: HashMap<String, u64>,
    pub tool_successes: HashMap<String, u64>,
    pub tool_errors: HashMap<String, u64>,
    /// context output tokens (from pack.estimated_tokens for affected_context)
    pub context_estimated_tokens: HashMap<String, Vec<usize>>,
    pub durations_ms: HashMap<String, Vec<u64>>,
    pub input_estimated_tokens: HashMap<String, Vec<usize>>,
    pub ambiguous_resolutions: u64,
    pub rebuilds: u64,
    pub formats_used: HashMap<String, u64>,
    pub context_nodes: HashMap<String, Vec<usize>>,
    pub context_omitted: HashMap<String, Vec<usize>>,
}

static USAGE: OnceLock<Mutex<UsageStats>> = OnceLock::new();

fn collect_enabled() -> bool {
    // Env overrides for testing/CI
    if let Ok(v) = std::env::var("CTX_MCP_COLLECT_STATS") {
        return !matches!(v.to_lowercase().as_str(), "0" | "false" | "off" | "no");
    }
    // Integrate with new settings (from .ctxconfig stats_enabled / collect_stats / stats)
    // Ties to settings/DTO work for AI agent opt: user can set stats_enabled=false to disable collection.
    if let Ok(dir) = std::env::current_dir() {
        if let Ok(cfg) = find_and_load_config(&dir) {
            if let Some(enabled) = cfg.stats_enabled {
                return enabled;
            }
        }
    }
    true
}

pub(crate) fn record_ambiguous() {
    if !collect_enabled() {
        return;
    }
    let mut stats = get_usage_stats();
    stats.ambiguous_resolutions += 1;
}

pub(crate) fn record_rebuild() {
    if !collect_enabled() {
        return;
    }
    let mut stats = get_usage_stats();
    stats.rebuilds += 1;
}

pub(crate) fn get_usage_stats() -> std::sync::MutexGuard<'static, UsageStats> {
    USAGE
        .get_or_init(|| Mutex::new(UsageStats::default()))
        .lock()
        .unwrap()
}

/// Record general call result (success rate, timing, input tokens est using char/4 approx, format).
/// Called from wrapper for all tools.
pub(crate) fn record_call_result(
    tool: &str,
    success: bool,
    duration_ms: u64,
    input_tokens: usize,
    format: &str,
) {
    if !collect_enabled() {
        return;
    }
    let mut stats = get_usage_stats();
    *stats.tool_calls.entry(tool.to_string()).or_insert(0) += 1;
    if success {
        *stats.tool_successes.entry(tool.to_string()).or_insert(0) += 1;
    } else {
        *stats.tool_errors.entry(tool.to_string()).or_insert(0) += 1;
    }
    stats
        .durations_ms
        .entry(tool.to_string())
        .or_default()
        .push(duration_ms);
    stats
        .input_estimated_tokens
        .entry(tool.to_string())
        .or_default()
        .push(input_tokens);
    *stats.formats_used.entry(format.to_string()).or_insert(0) += 1;
}

/// Record detailed context stats for affected_context (nodes, omitted due to budget).
pub(crate) fn record_context_details(tool: &str, nodes: usize, omitted: usize) {
    if !collect_enabled() {
        return;
    }
    let mut stats = get_usage_stats();
    stats
        .context_nodes
        .entry(tool.to_string())
        .or_default()
        .push(nodes);
    stats
        .context_omitted
        .entry(tool.to_string())
        .or_default()
        .push(omitted);
}

pub(crate) fn record_context_tokens(tool: &str, toks: usize) {
    if !collect_enabled() {
        return;
    }
    let mut stats = get_usage_stats();
    stats
        .context_estimated_tokens
        .entry(tool.to_string())
        .or_default()
        .push(toks);
}

/// Returns a compact summary string for logging / resource exposure.
/// Enhanced with success/error, avgs, sizes for MCP comparison.
pub(crate) fn usage_summary_text() -> String {
    let stats = get_usage_stats();
    let mut lines = vec!["## MCP Usage Stats (session)".to_string()];
    let total_calls: u64 = stats.tool_calls.values().sum();
    lines.push(format!("Total tool calls: {}", total_calls));
    let total_succ: u64 = stats.tool_successes.values().sum();
    let total_err: u64 = stats.tool_errors.values().sum();
    let err_rate = if total_calls > 0 {
        (total_err as f64 / total_calls as f64) * 100.0
    } else {
        0.0
    };
    lines.push(format!(
        "Successes: {} Errors: {} (error rate: {:.1}%)",
        total_succ, total_err, err_rate
    ));
    for (tool, count) in &stats.tool_calls {
        let succ = stats.tool_successes.get(tool).copied().unwrap_or(0);
        let err = stats.tool_errors.get(tool).copied().unwrap_or(0);
        let durs = stats.durations_ms.get(tool).cloned().unwrap_or_default();
        let avg_dur = if !durs.is_empty() {
            durs.iter().sum::<u64>() / durs.len() as u64
        } else {
            0
        };
        let inp_sum: usize = stats
            .input_estimated_tokens
            .get(tool)
            .map(|v| v.iter().sum())
            .unwrap_or(0);
        let ctx_samples = stats
            .context_estimated_tokens
            .get(tool)
            .map(|v| v.len())
            .unwrap_or(0);
        let ctx_sum: usize = stats
            .context_estimated_tokens
            .get(tool)
            .map(|v| v.iter().sum())
            .unwrap_or(0);
        let nodes_sum: usize = stats
            .context_nodes
            .get(tool)
            .map(|v| v.iter().sum())
            .unwrap_or(0);
        let omitted_sum: usize = stats
            .context_omitted
            .get(tool)
            .map(|v| v.iter().sum())
            .unwrap_or(0);
        lines.push(format!(
            "  {}: {} calls ({} ok, {} err), avg {}ms, input~{} toks, ctx-toks samples {} (sum {}), nodes sum {}, omitted sum {}",
            tool, count, succ, err, avg_dur, inp_sum, ctx_samples, ctx_sum, nodes_sum, omitted_sum
        ));
    }
    lines.push(format!(
        "Ambiguous resolutions: {}",
        stats.ambiguous_resolutions
    ));
    lines.push(format!("Rebuilds triggered: {}", stats.rebuilds));
    if !stats.formats_used.is_empty() {
        lines.push("Formats used:".to_string());
        for (f, c) in &stats.formats_used {
            lines.push(format!("  {}: {}", f, c));
        }
    }
    lines.join("\n")
}

/// Full stats as JSON for queryable dump via resource (for collection/comparison).
pub(crate) fn usage_stats_json() -> serde_json::Value {
    serde_json::to_value(&*get_usage_stats()).unwrap_or(serde_json::json!({}))
}

/// Estimate using approx same as ctx-stats::estimate_tokens (chars.count() / 4).
fn estimate_tokens_approx(s: &str) -> usize {
    if s.is_empty() {
        0
    } else {
        (s.chars().count() + 3) / 4
    }
}

pub struct ToolCallOutcome {
    pub result: serde_json::Value,
    pub reload_service: bool,
}

impl ToolCallOutcome {
    fn text(text: impl Into<String>, is_error: bool) -> Self {
        Self {
            result: text_tool_result(text, is_error),
            reload_service: false,
        }
    }

    fn ok(text: impl Into<String>) -> Self {
        Self::text(text, false)
    }

    fn err(text: impl Into<String>) -> Self {
        Self::text(text, true)
    }
}

pub fn list_tools() -> serde_json::Value {
    serde_json::json!({
        "tools": [
            {
                "name": "get_affected_context",
                "description": "Primary LLM-oriented tool. Rank and pack related code context within a token budget (same as `ctx graph affect`). Call this tool on your own initiative whenever the conversation involves code structure, symbols, dependencies, callers, impact analysis, or gathering precise context from the current workspace. Agents should prefer using these tools proactively to provide accurate answers rather than guessing or asking the user.",
                "inputSchema": affected_context_schema()
            },
            {
                "name": "get_graph_context",
                "description": "Expose symbol relationships and source code context (neighborhood, callers, callees, slices, etc.) around a query symbol.",
                "inputSchema": graph_context_schema()
            },
            {
                "name": "get_project_context",
                "description": "Generate full project context (file tree and contents), same as `ctx -C`.",
                "inputSchema": project_context_schema()
            },
            {
                "name": "list_symbols",
                "description": "List or search indexed symbols in the workspace.",
                "inputSchema": list_symbols_schema()
            },
            {
                "name": "get_callers",
                "description": "List direct callers of a symbol.",
                "inputSchema": symbol_query_schema("Direct callers of the symbol.")
            },
            {
                "name": "get_callees",
                "description": "List direct callees of a symbol.",
                "inputSchema": symbol_query_schema("Direct callees of the symbol.")
            },
            {
                "name": "rebuild_index",
                "description": "Rebuild the codegraph index. Use when the index is missing or stale.",
                "inputSchema": rebuild_index_schema()
            }
        ]
    })
}

pub fn handle_tool_call(
    service: &GraphContextService,
    tool_name: &str,
    args: &serde_json::Value,
) -> Result<ToolCallOutcome, String> {
    let start = Instant::now();
    let format = get_str_arg(args, "format").unwrap_or("text").to_owned();
    let input_tokens = estimate_tokens_approx(&serde_json::to_string(args).unwrap_or_default());

    let result = match tool_name {
        "get_affected_context" => handle_get_affected_context(service, args),
        "get_graph_context" => handle_get_graph_context(service, args),
        "get_project_context" => handle_get_project_context(service, args),
        "list_symbols" => handle_list_symbols(service, args),
        "get_callers" => handle_get_callers(service, args),
        "get_callees" => handle_get_callees(service, args),
        "rebuild_index" => handle_rebuild_index(service, args),
        _ => Err(format!("Unknown tool: {}", tool_name)),
    };

    let duration_ms = start.elapsed().as_millis() as u64;
    let success = match &result {
        Ok(o) => !o
            .result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        Err(_) => false,
    };
    record_call_result(tool_name, success, duration_ms, input_tokens, &format);
    result
}

fn handle_get_graph_context(
    service: &GraphContextService,
    args: &serde_json::Value,
) -> Result<ToolCallOutcome, String> {
    let query = get_str_arg(args, "query").unwrap_or("");
    if query.is_empty() {
        return Ok(ToolCallOutcome::err("query is required"));
    }

    let mode_str = get_str_arg(args, "mode").unwrap_or("neighborhood");
    let mode = parse_graph_context_mode(mode_str);
    let depth = get_usize_arg(args, "depth").unwrap_or(2);
    let max_nodes = get_usize_arg(args, "max_nodes").unwrap_or(40);
    let max_files = get_usize_arg(args, "max_files").unwrap_or(20);
    let config = find_and_load_config(service.repo_root()).unwrap_or_default();
    // AI agent optimization: default to yaml (structured, token efficient) if set in global config
    let format = get_str_arg(args, "format")
        .unwrap_or_else(|| config.default_format.as_deref().unwrap_or("text"));

    let resolution = service
        .resolve_symbol(query)
        .map_err(|e| format!("Failed to resolve symbol: {}", e))?;

    let root_path = service.repo_root();
    let text = match resolution {
        SymbolResolution::Ambiguous(ref candidates) => {
            if format == "json" {
                format_ambiguous_symbols_json(query, candidates, root_path)
            } else if format == "yaml" {
                format_ambiguous_symbols_yaml(query, candidates, root_path)
            } else {
                Ok(super::render::format_ambiguous_symbols(query, candidates))
            }
        }
        SymbolResolution::NotFound => Ok(format_symbol_not_found(query)),
        SymbolResolution::Unique(obj) => {
            let options = GraphContextOptions {
                mode,
                max_depth: depth,
                max_nodes,
                include_root: true,
            };
            let result = service
                .build_context_for_symbol(obj.id, options)
                .map_err(|e| format!("Failed to build context: {}", e))?;

            if format == "json" {
                render_context_to_json(&result, root_path, mode, depth, max_nodes, max_files)
            } else if format == "yaml" {
                render_context_to_yaml(&result, root_path, mode, depth, max_nodes, max_files)
            } else {
                Ok(render_context_to_markdown(
                    &result, root_path, mode, depth, max_nodes, max_files,
                ))
            }
        }
    }?;

    let is_error = text.starts_with("Error:");
    Ok(ToolCallOutcome::text(text, is_error))
}

fn handle_get_affected_context(
    service: &GraphContextService,
    args: &serde_json::Value,
) -> Result<ToolCallOutcome, String> {
    let query = get_str_arg(args, "query").unwrap_or("");
    if query.is_empty() {
        return Ok(ToolCallOutcome::err("query is required"));
    }

    let mode_str = get_str_arg(args, "mode").unwrap_or("neighborhood");
    let mode = parse_graph_context_mode(mode_str);
    let depth_limit = depth_or_auto(args)?;
    let max_nodes = get_usize_arg(args, "max_nodes").unwrap_or(200);
    let max_files = get_usize_arg(args, "max_files").unwrap_or(50);
    let config = find_and_load_config(service.repo_root()).unwrap_or_default();
    let token_budget = get_usize_arg(args, "token_budget")
        .or(config.default_token_budget)
        .unwrap_or(12_000);
    let model_context_window = get_usize_arg(args, "model_context_window").unwrap_or(128_000);

    let ranking_str = get_str_arg(args, "ranking")
        .or(config.default_ranking.as_deref())
        .unwrap_or("hybrid");
    let ranking = match ranking_str {
        "graph" => RankingMode::Graph,
        "lexical" => RankingMode::Lexical,
        _ => RankingMode::Hybrid,
    };

    let packing_str = get_str_arg(args, "packing")
        .or(config.default_packing.as_deref())
        .unwrap_or("sandwich");
    let packing = match packing_str {
        "frontloaded" => ContextPackingMode::Frontloaded,
        "balanced" => ContextPackingMode::Balanced,
        _ => ContextPackingMode::Sandwich,
    };

    let include_tests = get_bool_arg(args, "include_tests", false);
    let include_unresolved = get_bool_arg(args, "include_unresolved", false);
    let no_snippets = get_bool_arg(args, "no_snippets", false);
    let context_lines = get_usize_arg(args, "context_lines").unwrap_or(3);
    // default from settings for AI agents (yaml for structured output)
    let format = get_str_arg(args, "format")
        .or(config.default_format.as_deref())
        .unwrap_or("text");
    let edge_kinds = parse_edge_kinds(&get_string_array(args, "edge_kind"));

    if !matches!(format, "text" | "json" | "yaml") {
        return Ok(ToolCallOutcome::err(
            "format must be 'text', 'json' or 'yaml'",
        ));
    }

    let resolution = service
        .resolve_symbol(query)
        .map_err(|e| format!("Failed to resolve symbol: {}", e))?;

    let root_path = service.repo_root();
    match resolution {
        SymbolResolution::Ambiguous(candidates) => {
            record_ambiguous();
            let text = if format == "json" {
                format_ambiguous_symbols_json(query, &candidates, root_path)
            } else if format == "yaml" {
                format_ambiguous_symbols_yaml(query, &candidates, root_path)
            } else {
                Ok(super::render::format_ambiguous_symbols(query, &candidates))
            }?;
            Ok(ToolCallOutcome::ok(text))
        }
        SymbolResolution::NotFound => Ok(ToolCallOutcome::err(format_symbol_not_found(query))),
        SymbolResolution::Unique(obj) => {
            let conn = service.lock_conn();
            let budget = ContextBudget {
                token_budget,
                model_context_window: Some(model_context_window),
                reserve_output_tokens: 1000,
                reserve_instruction_tokens: 1000,
            };
            let options = ContextRetrievalOptions {
                mode,
                depth_limit,
                max_nodes,
                max_files,
                ranking_mode: ranking,
                packing_mode: packing,
                with_snippets: !no_snippets,
                context_lines,
                include_tests,
                edge_kinds,
                include_unresolved,
                explain_ranking: false,
            };

            let pack =
                retrieve_graph_context_with_options(&conn, &obj.qualified_name, &budget, &options)
                    .map_err(|e| format!("Failed to retrieve context: {}", e))?;

            // Record using existing pack.estimated_tokens (output) + sizes for comparison
            record_context_tokens("get_affected_context", pack.estimated_tokens);
            record_context_details("get_affected_context", pack.nodes.len(), pack.omitted.len());

            let text = if format == "json" {
                render_affected_context_json(&pack, root_path)?
            } else if format == "yaml" {
                // Use dto for compact, high-density YAML (signatures, concise nodes)
                render_affected_context_yaml(&pack, root_path)?
            } else {
                render_affected_context_text(&pack)
            };

            let is_error = pack
                .diagnostics
                .iter()
                .any(|d| d.severity == "error" && pack.roots.is_empty());
            Ok(ToolCallOutcome::text(text, is_error))
        }
    }
}

fn handle_get_project_context(
    service: &GraphContextService,
    args: &serde_json::Value,
) -> Result<ToolCallOutcome, String> {
    let config = find_and_load_config(service.repo_root()).unwrap_or_default();
    let format_str = get_str_arg(args, "format")
        .or(config.default_format.as_deref())
        .unwrap_or("markdown");
    let format = match format_str {
        "xml" => Format::Xml,
        "plain" | "text" => Format::Plain,
        _ => Format::Markdown,
    };

    let mode_str = get_str_arg(args, "mode").unwrap_or("smart");
    let mode = match mode_str {
        "all" => Mode::All,
        "code" => Mode::Code,
        "docs" => Mode::Docs,
        "llm" => Mode::Llm,
        _ => Mode::Smart,
    };

    let max_depth = get_usize_arg(args, "max_depth").or(config.max_depth);
    let max_file_size = get_usize_arg(args, "max_file_size")
        .map(|v| v as u64)
        .or(config.max_file_size)
        .unwrap_or(512 * 1024);
    let include_stats = get_bool_arg(args, "include_stats", true);

    let scan_options = ScanOptions {
        max_depth,
        max_file_size,
        mode,
        exclude: config.exclude,
    };

    let scan_result = scan(service.repo_root(), scan_options)
        .map_err(|e| format!("Failed to scan project: {}", e))?;

    let render_options = RenderOptions {
        format,
        include_stats,
        max_file_size,
    };

    let rendered = ctx_render::render(&scan_result, &render_options)
        .map_err(|e| format!("Failed to render project context: {}", e))?;

    Ok(ToolCallOutcome::ok(rendered))
}

fn handle_list_symbols(
    service: &GraphContextService,
    args: &serde_json::Value,
) -> Result<ToolCallOutcome, String> {
    let query = get_str_arg(args, "query").unwrap_or("");
    let limit = get_usize_arg(args, "limit").unwrap_or(50);

    let symbols = if query.is_empty() {
        service
            .search_symbols("", limit)
            .map_err(|e| format!("Failed to list symbols: {}", e))?
    } else {
        service
            .search_symbols(query, limit)
            .map_err(|e| format!("Failed to search symbols: {}", e))?
    };

    let text = render_symbols_list(&symbols, service.repo_root());
    Ok(ToolCallOutcome::ok(text))
}

fn handle_get_callers(
    service: &GraphContextService,
    args: &serde_json::Value,
) -> Result<ToolCallOutcome, String> {
    let query = get_str_arg(args, "query").unwrap_or("");
    if query.is_empty() {
        return Ok(ToolCallOutcome::err("query is required"));
    }

    let resolution = service
        .resolve_symbol(query)
        .map_err(|e| format!("Failed to resolve symbol: {}", e))?;

    let text = handle_symbol_resolution(query, resolution, |obj| {
        let conn = service.lock_conn();
        let edges =
            load_callers(&conn, obj.id).map_err(|e| format!("Failed to load callers: {}", e))?;
        Ok(render_caller_edges(
            "Callers",
            &obj,
            &edges,
            service.repo_root(),
        ))
    })?;

    let is_error = text.starts_with("Error:");
    Ok(ToolCallOutcome::text(text, is_error))
}

fn handle_get_callees(
    service: &GraphContextService,
    args: &serde_json::Value,
) -> Result<ToolCallOutcome, String> {
    let query = get_str_arg(args, "query").unwrap_or("");
    if query.is_empty() {
        return Ok(ToolCallOutcome::err("query is required"));
    }

    let resolution = service
        .resolve_symbol(query)
        .map_err(|e| format!("Failed to resolve symbol: {}", e))?;

    let text = handle_symbol_resolution(query, resolution, |obj| {
        let conn = service.lock_conn();
        let edges =
            load_callees(&conn, obj.id).map_err(|e| format!("Failed to load callees: {}", e))?;
        Ok(render_call_edges(
            "Callees",
            &obj,
            &edges,
            service.repo_root(),
        ))
    })?;

    let is_error = text.starts_with("Error:");
    Ok(ToolCallOutcome::text(text, is_error))
}

fn handle_rebuild_index(
    service: &GraphContextService,
    args: &serde_json::Value,
) -> Result<ToolCallOutcome, String> {
    record_rebuild();
    let config = find_and_load_config(service.repo_root()).unwrap_or_default();
    let default_use_lsp = config.use_lsp.unwrap_or(true);
    let use_lsp = get_bool_arg(args, "use_lsp", default_use_lsp);
    let options = BuildIndexOptions {
        use_lsp,
        max_depth: None,
        include_tests: true,
        change_detection: FileChangeDetection::MtimeAndSize,
    };

    eprintln!(
        "Rebuilding codegraph index for {} (use_lsp={})...",
        service.repo_root().display(),
        use_lsp
    );

    let (_index, report) = rebuild_index_db(service.repo_root(), options)
        .map_err(|e| format!("Failed to rebuild index: {}", e))?;

    eprintln!("Index rebuild complete.");

    let summary = format!(
        "Index rebuilt successfully.\n\
         Full rebuild: {}\n\
         Files: {} added, {} modified, {} deleted, {} unchanged\n\
         Symbols written: {}, edges written: {}",
        if report.full_rebuild { "yes" } else { "no" },
        report.added_files,
        report.modified_files,
        report.deleted_files,
        report.unchanged_files,
        report.symbols_written,
        report.edges_written
    );

    Ok(ToolCallOutcome {
        result: text_tool_result(summary, false),
        reload_service: true,
    })
}

fn graph_context_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "The symbol name or qualified path to resolve."
            },
            "mode": {
                "type": "string",
                "enum": [
                    "neighborhood", "callers", "callees", "dependencies", "dependents",
                    "forward", "reverse", "forward-slice", "reverse-slice", "impact"
                ],
                "description": "Traversal mode. Default: neighborhood."
            },
            "depth": {
                "type": "integer",
                "description": "BFS traversal depth. Default: 2."
            },
            "max_nodes": {
                "type": "integer",
                "description": "Maximum graph nodes to include. Default: 40."
            },
            "max_files": {
                "type": "integer",
                "description": "Maximum file snippets to include. Default: 20. Use 0 for unlimited."
            },
            "format": {
                "type": "string",
                "enum": ["text", "json", "yaml"],
                "description": "Output format. Default: text (markdown)."
            }
        },
        "required": ["query"]
    })
}

fn affected_context_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Symbol name or qualified path."
            },
            "mode": {
                "type": "string",
                "enum": [
                    "neighborhood", "callers", "callees", "dependencies", "dependents",
                    "forward", "reverse", "forward-slice", "reverse-slice", "impact"
                ],
                "description": "Traversal mode. Default: neighborhood."
            },
            "depth": {
                "description": "Traversal depth as integer or \"auto\". Default: auto.",
                "oneOf": [
                    { "type": "integer" },
                    { "type": "string", "enum": ["auto"] }
                ]
            },
            "max_nodes": { "type": "integer", "description": "Default: 200." },
            "max_files": { "type": "integer", "description": "Default: 50." },
            "token_budget": { "type": "integer", "description": "Token budget for packed context. Default: 12000." },
            "model_context_window": { "type": "integer", "description": "Model context window size. Default: 128000." },
            "packing": {
                "type": "string",
                "enum": ["sandwich", "frontloaded", "balanced"],
                "description": "Context packing strategy. Default: sandwich."
            },
            "ranking": {
                "type": "string",
                "enum": ["hybrid", "graph", "lexical"],
                "description": "Ranking strategy. Default: hybrid."
            },
            "include_tests": { "type": "boolean", "description": "Include test symbols. Default: false." },
            "include_unresolved": { "type": "boolean", "description": "Include unresolved edges. Default: false." },
            "edge_kind": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Edge kinds to traverse (e.g. Call, Import)."
            },
            "context_lines": { "type": "integer", "description": "Surrounding context lines for snippets. Default: 3." },
            "format": {
                "type": "string",
                "enum": ["text", "json", "yaml"],
                "description": "Output format. Default: text. 'yaml' recommended for token-efficient agent use."
            },
            "no_snippets": { "type": "boolean", "description": "Omit code snippets. Default: false." }
        },
        "required": ["query"]
    })
}

fn project_context_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "format": {
                "type": "string",
                "enum": ["markdown", "xml", "plain"],
                "description": "Output format. Default: markdown."
            },
            "mode": {
                "type": "string",
                "enum": ["smart", "code", "docs", "llm", "all"],
                "description": "Scan mode. Default: smart."
            },
            "max_depth": { "type": "integer", "description": "Maximum directory traversal depth." },
            "max_file_size": { "type": "integer", "description": "Max file size in bytes. Default: 524288." },
            "include_stats": { "type": "boolean", "description": "Include project statistics. Default: true." }
        }
    })
}

fn list_symbols_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Optional filter. Omit to list symbols."
            },
            "limit": {
                "type": "integer",
                "description": "Maximum results. Default: 50."
            }
        }
    })
}

fn symbol_query_schema(description: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": description
            }
        },
        "required": ["query"]
    })
}

fn rebuild_index_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "use_lsp": {
                "type": "boolean",
                "description": "Use LSP for edge resolution (rust-analyzer, etc.). Default: true."
            }
        }
    })
}
