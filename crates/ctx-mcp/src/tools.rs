use super::protocol::{
    depth_or_auto, get_bool_arg, get_str_arg, get_usize_arg, make_tool_result,
    parse_graph_context_mode, text_tool_result,
};
use super::render::{
    affected_context_to_structured_value, kind_to_str, render_affected_context_json,
    render_affected_context_text, render_affected_context_yaml, render_symbols_list,
};
use ctx_dto::{serialize_json, serialize_yaml};
use std::path::{Path, PathBuf};
use ctx_codegraph::index::BuildIndexOptions;
use ctx_codegraph::service::GraphContextService;
use ctx_codegraph::storage::rebuild_index_db;
use ctx_codegraph::{
    ContextBudget, ContextPackingMode, HybridRetrievalOptions, RetrievalStrategy,
    retrieve_context_for_service,
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

/// Persist the current (last) usage stats as JSON into the codegraph DB metadata under "mcp_last_stats".
/// Only writes if index DB exists (via write_metadata guard) and collection is enabled.
/// Called at MCP server shutdown so that `ctx stats` can surface last known session stats.
pub(crate) fn persist_mcp_stats(workspace_root: &std::path::Path) {
    if !collect_enabled() {
        return;
    }
    let json = usage_stats_json();
    let json_str = match serde_json::to_string(&json) {
        Ok(s) => s,
        Err(_) => return,
    };
    // write_metadata will no-op (err) if no index DB present; ignore result for silent persist.
    let _ = ctx_codegraph::storage::write_metadata(workspace_root, "mcp_last_stats", &json_str);
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
                "name": "retrieve_context",
                "title": "Retrieve Context",
                "description": "Primary LLM-oriented tool. Hybrid/graph/lexical/dense retrieval with token-budget packing (replaces get_affected_context, get_graph_context, search_code, get_callers, get_callees). When to use: impact analysis, callers/callees, text search, or gathering packed context before answering code questions. Examples: {\"name\":\"retrieve_context\",\"arguments\":{\"query\":\"my_func\",\"strategy\":\"hybrid\",\"format\":\"yaml\",\"token_budget\":8000}}; graph callers: {\"query\":\"foo\",\"strategy\":\"graph\",\"graph_mode\":\"callers\",\"depth\":3}. Self-correction: refine `query` with qualified paths from list_symbols. Prefer yaml (default) for token efficiency.",
                "inputSchema": retrieve_context_schema(),
                "annotations": { "readOnlyHint": true },
                "outputSchema": retrieve_context_output_schema()
            },
            {
                "name": "list_symbols",
                "title": "List Symbols",
                "description": "List or search indexed symbols in the workspace. When to use: to discover symbols, resolve names, or disambiguate before retrieve_context; supports optional kind filter. Example: list_symbols(query: \"run\", kind: \"fn\", limit: 20). Self-correction: use results to pick exact qualified name for subsequent retrieve_context calls.",
                "inputSchema": list_symbols_schema(),
                "annotations": { "readOnlyHint": true },
                "outputSchema": list_symbols_output_schema()
            },
            {
                "name": "read_file",
                "title": "Read File Content",
                "description": "Read full or partial content of any file (path relative to workspace root, or absolute under it). Supports start_line/end_line/max_lines for slices. Returns raw text (for direct source) or structured json/yaml with metadata. Use proactively and often for exact source code of files or regions (when graph-packed context or symbol snippets are insufficient). Works with or without code index. Complements all other tools for complete files, configs, docs, generated, etc.",
                "inputSchema": read_file_schema(),
                "annotations": { "readOnlyHint": true }
            },
            {
                "name": "rebuild_index",
                "title": "Rebuild Index",
                "description": "Rebuild the codegraph index (optionally with embeddings/lexical search indexes). When to use: only when index missing/stale or hybrid retrieval fails. Example: rebuild_index(with_all: true) or rebuild_index(use_lsp: true, with_emb: true).",
                "inputSchema": rebuild_index_schema(),
                "annotations": { "destructiveHint": true }
            },
            {
                "name": "get_project_context",
                "title": "Get Project Context",
                "description": "Generate full project context (file tree and contents), same as `ctx -C`. When to use: for broad workspace overview or when no specific symbol query. Supports markdown/xml/plain.",
                "inputSchema": project_context_schema(),
                "annotations": { "readOnlyHint": true }
            }
        ]
    })
}

pub fn handle_tool_call(
    service: Option<&GraphContextService>,
    root: &Path,
    tool_name: &str,
    args: &serde_json::Value,
) -> Result<ToolCallOutcome, String> {
    let start = Instant::now();
    let format = get_str_arg(args, "format").unwrap_or("text").to_owned();
    let input_tokens = estimate_tokens_approx(&serde_json::to_string(args).unwrap_or_default());

    let result = match tool_name {
        "retrieve_context" | "list_symbols" => {
            let s = service.ok_or_else(|| format!("{} requires a code index (no DB). Use rebuild_index first to enable graph tools.", tool_name))?;
            match tool_name {
                "retrieve_context" => handle_retrieve_context(s, args),
                "list_symbols" => handle_list_symbols(s, args),
                _ => unreachable!(),
            }
        }
        "get_project_context" => handle_get_project_context(root, args),
        "rebuild_index" => handle_rebuild_index(root, args),
        "read_file" => handle_read_file(root, args),
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

fn handle_retrieve_context(
    service: &GraphContextService,
    args: &serde_json::Value,
) -> Result<ToolCallOutcome, String> {
    let query = get_str_arg(args, "query").unwrap_or("");
    if query.is_empty() {
        return Ok(ToolCallOutcome::err("query is required"));
    }

    let config = find_and_load_config(service.repo_root()).unwrap_or_default();

    let strategy_str = get_str_arg(args, "strategy")
        .or(config.default_retrieval_strategy.as_deref())
        .unwrap_or("hybrid");
    let mut strategy = RetrievalStrategy::from_str(strategy_str);
    if !config.search_auto_enabled() && strategy != RetrievalStrategy::Graph {
        strategy = RetrievalStrategy::Graph;
    }

    let graph_mode_str = get_str_arg(args, "graph_mode").unwrap_or("neighborhood");
    let mode = parse_graph_context_mode(graph_mode_str);
    let depth_limit = depth_or_auto(args)?;
    let max_nodes = get_usize_arg(args, "max_nodes").unwrap_or(200);
    let max_files = get_usize_arg(args, "max_files").unwrap_or(50);
    let token_budget = get_usize_arg(args, "token_budget")
        .or(config.default_token_budget)
        .unwrap_or(12_000);
    let model_context_window = get_usize_arg(args, "model_context_window").unwrap_or(128_000);

    let packing_str = get_str_arg(args, "packing")
        .or(config.default_packing.as_deref())
        .unwrap_or("sandwich");
    let packing = match packing_str {
        "frontloaded" => ContextPackingMode::Frontloaded,
        "balanced" => ContextPackingMode::Balanced,
        _ => ContextPackingMode::Sandwich,
    };

    let include_tests = get_bool_arg(args, "include_tests", false);
    let no_snippets = get_bool_arg(args, "no_snippets", false);
    let context_lines = get_usize_arg(args, "context_lines").unwrap_or(3);
    let format = get_str_arg(args, "format")
        .or(config.default_format.as_deref())
        .unwrap_or("yaml");

    let include_unresolved = get_bool_arg(args, "include_unresolved", false);
    let explain_ranking = get_bool_arg(args, "explain_ranking", false);
    let mut edge_kinds = Vec::new();
    if let Some(arr) = args.get("edge_kinds").or_else(|| args.get("edge_kind")).and_then(|v| v.as_array()) {
        for val in arr {
            if let Some(s) = val.as_str() {
                if let Some(kind) = ctx_codegraph::model::EdgeKind::from_str(s) {
                    edge_kinds.push(kind);
                }
            }
        }
    }

    if !matches!(format, "text" | "json" | "yaml") {
        return Ok(ToolCallOutcome::err(
            "format must be 'text', 'json' or 'yaml'",
        ));
    }

    let budget = ContextBudget {
        token_budget,
        model_context_window: Some(model_context_window),
        reserve_output_tokens: 1000,
        reserve_instruction_tokens: 1000,
    };

    let mut hybrid_options = HybridRetrievalOptions::default();
    hybrid_options.strategy = strategy;
    hybrid_options.graph_options.mode = mode;
    hybrid_options.graph_options.depth_limit = depth_limit;
    hybrid_options.graph_options.max_nodes = max_nodes;
    hybrid_options.graph_options.max_files = max_files;
    hybrid_options.graph_options.packing_mode = packing;
    hybrid_options.graph_options.with_snippets = !no_snippets;
    hybrid_options.graph_options.context_lines = context_lines;
    hybrid_options.graph_options.include_tests = include_tests;
    hybrid_options.graph_options.edge_kinds = edge_kinds;
    hybrid_options.graph_options.include_unresolved = include_unresolved;
    hybrid_options.graph_options.explain_ranking = explain_ranking;

    let pack = retrieve_context_for_service(service, query, &budget, &hybrid_options, &config)
        .map_err(|e| format!("Failed to retrieve context: {}", e))?;

    let root_path = service.repo_root();
    record_context_tokens("retrieve_context", pack.estimated_tokens);
    record_context_details("retrieve_context", pack.nodes.len(), pack.omitted.len());

    let text = if format == "json" {
        render_affected_context_json(&pack, root_path)?
    } else if format == "yaml" {
        render_affected_context_yaml(&pack, root_path)?
    } else {
        render_affected_context_text(&pack)
    };

    let is_error = pack
        .diagnostics
        .iter()
        .any(|d| d.severity == "error" && pack.roots.is_empty());
    if (format == "json" || format == "yaml") && !is_error {
        if let Ok(scv) = affected_context_to_structured_value(&pack, root_path) {
            return Ok(ToolCallOutcome {
                result: make_tool_result(text, Some(scv), is_error),
                reload_service: false,
            });
        }
    }
    Ok(ToolCallOutcome::text(text, is_error))
}

fn handle_get_project_context(
    root: &Path,
    args: &serde_json::Value,
) -> Result<ToolCallOutcome, String> {
    let config = find_and_load_config(root).unwrap_or_default();
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

    let scan_result = scan(root, scan_options)
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
    let kind_filter = get_str_arg(args, "kind");

    let mut symbols = if query.is_empty() {
        service
            .search_symbols("", limit)
            .map_err(|e| format!("Failed to list symbols: {}", e))?
    } else {
        service
            .search_symbols(query, limit)
            .map_err(|e| format!("Failed to search symbols: {}", e))?
    };

    if let Some(kf) = kind_filter {
        symbols.retain(|s| kind_to_str(s.kind) == kf);
    }

    let text = render_symbols_list(&symbols, service.repo_root());
    Ok(ToolCallOutcome::ok(text))
}

fn handle_rebuild_index(
    root: &Path,
    args: &serde_json::Value,
) -> Result<ToolCallOutcome, String> {
    record_rebuild();
    let config = find_and_load_config(root).unwrap_or_default();
    let default_use_lsp = config.use_lsp.unwrap_or(true);
    let with_all = get_bool_arg(args, "with_all", false);
    let use_lsp = if with_all {
        get_bool_arg(args, "use_lsp", true)
    } else {
        get_bool_arg(args, "use_lsp", default_use_lsp)
    };

    let with_embeddings = if get_bool_arg(args, "without_emb", false) {
        Some(false)
    } else if get_bool_arg(args, "with_emb", false) || with_all {
        Some(true)
    } else {
        None
    };
    let with_lexical = if get_bool_arg(args, "without_lex", false) {
        Some(false)
    } else if get_bool_arg(args, "with_lex", false) || with_all {
        Some(true)
    } else {
        None
    };

    let options = BuildIndexOptions { extraction_tier: None,
        use_lsp,
        with_embeddings,
        with_lexical,
        ..Default::default()
    };

    let emb_enabled = config.effective_with_embeddings(options.with_embeddings);
    let lex_enabled = config.effective_with_lexical(options.with_lexical);

    eprintln!(
        "Rebuilding codegraph index for {} (use_lsp={}, with_embeddings={}, with_lexical={})...",
        root.display(),
        use_lsp,
        emb_enabled,
        lex_enabled,
    );

    let (_index, report) = rebuild_index_db(root, options)
        .map_err(|e| format!("Failed to rebuild index: {}", e))?;

    eprintln!("Index rebuild complete.");

    let summary = format!(
        "Index rebuilt successfully.\n\
         Full rebuild: {}\n\
         With embeddings: {}, with lexical: {}\n\
         Files: {} added, {} modified, {} deleted, {} unchanged\n\
         Symbols written: {}, edges written: {}",
        if report.full_rebuild { "yes" } else { "no" },
        emb_enabled,
        lex_enabled,
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

fn retrieve_context_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Symbol name, qualified path, or free-text search query."
            },
            "strategy": {
                "type": "string",
                "enum": ["hybrid", "graph", "lexical", "dense"],
                "description": "Retrieval strategy. Default: hybrid. Use graph for callers/callees/neighborhood; hybrid/lexical/dense for text search."
            },
            "graph_mode": {
                "type": "string",
                "enum": [
                    "neighborhood", "callers", "callees", "dependencies", "dependents",
                    "forward", "reverse", "forward-slice", "reverse-slice", "impact"
                ],
                "description": "Graph traversal mode (when strategy=graph or for graph expansion). Default: neighborhood."
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
            "include_tests": { "type": "boolean", "description": "Include test symbols. Default: false." },
            "context_lines": { "type": "integer", "description": "Surrounding context lines for snippets. Default: 3." },
            "format": {
                "type": "string",
                "enum": ["text", "json", "yaml"],
                "description": "Output format. Default: yaml."
            },
            "no_snippets": { "type": "boolean", "description": "Omit code snippets. Default: false." },
            "edge_kinds": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Specific edge kinds to traverse (e.g. syntax, heuristic, unresolved, lsp-exact)."
            },
            "include_unresolved": {
                "type": "boolean",
                "description": "Include unresolved edges. Default: false."
            },
            "explain_ranking": {
                "type": "boolean",
                "description": "Provide a detailed textual explanation of ranking scores. Default: false."
            }
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
            },
            "kind": {
                "type": "string",
                "description": "Optional filter by kind (e.g. 'fn', 'struct', 'method', 'enum'). Matches kind_to_str codes."
            }
        }
    })
}

fn rebuild_index_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "with_all": {
                "type": "boolean",
                "description": "Enable all build methods: LSP resolution, dense embeddings, and lexical search. Default: false."
            },
            "use_lsp": {
                "type": "boolean",
                "description": "Use LSP for edge resolution (rust-analyzer, etc.). Default: true (or from .ctxconfig); with with_all, defaults to true unless explicitly false."
            },
            "with_emb": {
                "type": "boolean",
                "description": "Force building embedding search index. Default: auto from .ctxconfig embedding_model."
            },
            "without_emb": {
                "type": "boolean",
                "description": "Skip embedding search index build."
            },
            "with_lex": {
                "type": "boolean",
                "description": "Force building lexical search index. Default: auto from .ctxconfig."
            },
            "without_lex": {
                "type": "boolean",
                "description": "Skip lexical search index build."
            }
        }
    })
}

// Simple output schemas for structuredContent (match the DTO shapes used for json/yaml outputs).
fn retrieve_context_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "query": { "type": "string" },
            "mode": { "type": "string" },
            "token_budget": { "type": "integer" },
            "estimated_tokens": { "type": "integer" },
            "roots": { "type": "array", "items": { "type": "object" } },
            "nodes": { "type": "array", "items": { "type": "object" } },
            "diagnostics": { "type": "array", "items": { "type": "object" } }
        }
    })
}

fn list_symbols_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "symbols": { "type": "array", "items": { "type": "object" } }
        }
    })
}

fn read_file_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "File path relative to workspace root, or absolute path that must be under the workspace root."
            },
            "start_line": {
                "type": "integer",
                "description": "1-based inclusive start line. Default: 1 (start of file)."
            },
            "end_line": {
                "type": "integer",
                "description": "1-based inclusive end line. Default: end of file."
            },
            "max_lines": {
                "type": "integer",
                "description": "Cap on number of lines returned from start."
            },
            "format": {
                "type": "string",
                "enum": ["text", "json", "yaml"],
                "description": "text = raw content (ideal for pasting exact source); json/yaml = structured with metadata + sliced content. Default: text."
            }
        },
        "required": ["path"]
    })
}

// --- File tools impl (std fs, no index dep for read; security: never escape root) ---

fn resolve_path_under_root(root: &Path, input: &str) -> Result<PathBuf, String> {
    let candidate = if Path::new(input).is_absolute() {
        PathBuf::from(input)
    } else {
        root.join(input)
    };
    match candidate.strip_prefix(root) {
        Ok(rel) => {
            let mut safe = PathBuf::new();
            for comp in rel.components() {
                use std::path::Component::*;
                match comp {
                    ParentDir => return Err(format!("Path escapes root (contains ..): {}", input)),
                    CurDir => {}
                    _ => safe.push(comp),
                }
            }
            Ok(root.join(safe))
        }
        Err(_) => Err(format!("Path escapes workspace root: {}", input)),
    }
}

fn slice_lines<'a>(
    lines: &[&'a str],
    start: Option<usize>,
    end: Option<usize>,
    max_lines: Option<usize>,
) -> (Vec<&'a str>, usize, usize, bool) {
    let n = lines.len();
    if n == 0 {
        return (vec![], 0, 0, false);
    }
    let mut s = start.unwrap_or(1).max(1);
    let mut e = end.unwrap_or(n);
    if let Some(m) = max_lines {
        let wanted = e.saturating_sub(s) + 1;
        if wanted > m {
            e = s + m.saturating_sub(1);
        }
    }
    if s > n {
        s = n;
    }
    if e > n {
        e = n;
    }
    if s > e {
        s = e;
    }
    let sliced = if s == 0 { vec![] } else { lines[(s - 1)..e].to_vec() };
    let did_limit = start.is_some() || end.is_some() || max_lines.is_some();
    let truncated = did_limit && (s > 1 || e < n);
    (sliced, s, e, truncated)
}

fn handle_read_file(root: &Path, args: &serde_json::Value) -> Result<ToolCallOutcome, String> {
    let path_str = get_str_arg(args, "path").unwrap_or("");
    if path_str.trim().is_empty() {
        return Ok(ToolCallOutcome::err("path is required"));
    }
    let start_line = get_usize_arg(args, "start_line");
    let end_line = get_usize_arg(args, "end_line");
    let max_lines = get_usize_arg(args, "max_lines");
    let format = get_str_arg(args, "format").unwrap_or("text");
    if !matches!(format, "text" | "json" | "yaml") {
        return Ok(ToolCallOutcome::err("format must be 'text', 'json' or 'yaml'"));
    }

    let file_path = resolve_path_under_root(root, path_str)?;
    let meta = std::fs::metadata(&file_path)
        .map_err(|e| format!("Cannot access '{}': {}", path_str, e))?;
    if !meta.is_file() {
        return Ok(ToolCallOutcome::err(format!("Not a regular file: {}", path_str)));
    }

    let full = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Read failed for '{}': {}", path_str, e))?;
    let all_lines: Vec<&str> = full.lines().collect();
    let total = all_lines.len();
    let (sliced, from, to, trunc) = slice_lines(&all_lines, start_line, end_line, max_lines);
    let content_str = sliced.join("\n");
    let rel = file_path
        .strip_prefix(root)
        .unwrap_or(&file_path)
        .to_string_lossy()
        .to_string();

    let out_text = match format {
        "json" => {
            let v = serde_json::json!({
                "path": rel,
                "start_line": from,
                "end_line": to,
                "total_lines": total,
                "content": content_str,
                "truncated": trunc,
            });
            serialize_json(&v)?
        }
        "yaml" => {
            let v = serde_json::json!({
                "path": rel,
                "start_line": from,
                "end_line": to,
                "total_lines": total,
                "content": content_str,
                "truncated": trunc,
            });
            serialize_yaml(&v)?
        }
        _ => {
            // text: prefer clean content (exact source for agents)
            if from == 1 && to == total && !trunc {
                content_str
            } else {
                format!(
                    "// file: {} (lines {}-{} of {}{})\n{}",
                    rel,
                    from,
                    to,
                    total,
                    if trunc { ", truncated" } else { "" },
                    content_str
                )
            }
        }
    };

    Ok(ToolCallOutcome::ok(out_text))
}


