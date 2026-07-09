use super::protocol::{
    depth_or_auto, get_bool_arg, get_str_arg, get_string_array, get_usize_arg,
    parse_edge_kinds, parse_graph_context_mode, text_tool_result,
};
use super::render::{
    format_symbol_not_found, handle_symbol_resolution, render_affected_context_text,
    render_caller_edges, render_call_edges, render_context_to_markdown, render_symbols_list,
};
use crate::index::BuildIndexOptions;
use crate::model::{FileChangeDetection, GraphContextOptions, SymbolResolution};
use crate::service::GraphContextService;
use crate::storage::{load_callees, load_callers, rebuild_index_db};
use crate::{
    ContextBudget, ContextPackingMode, ContextRetrievalOptions, RankingMode,
    retrieve_graph_context_with_options,
};
use ctx_config::find_and_load_config;
use ctx_core::scan;
use ctx_models::{Mode, ScanOptions};
use ctx_render::{Format, RenderOptions};
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
                "description": "Primary LLM-oriented tool. Rank and pack related code context within a token budget (same as `ctx graph affect`).",
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
    match tool_name {
        "get_affected_context" => handle_get_affected_context(service, args),
        "get_graph_context" => handle_get_graph_context(service, args),
        "get_project_context" => handle_get_project_context(service, args),
        "list_symbols" => handle_list_symbols(service, args),
        "get_callers" => handle_get_callers(service, args),
        "get_callees" => handle_get_callees(service, args),
        "rebuild_index" => handle_rebuild_index(service, args),
        _ => Err(format!("Unknown tool: {}", tool_name)),
    }
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

    let resolution = service
        .resolve_symbol(query)
        .map_err(|e| format!("Failed to resolve symbol: {}", e))?;

    let text = handle_symbol_resolution(query, resolution, |obj| {
        let options = GraphContextOptions {
            mode,
            max_depth: depth,
            max_nodes,
            include_root: true,
        };
        let result = service
            .build_context_for_symbol(obj.id, options)
            .map_err(|e| format!("Failed to build context: {}", e))?;
        Ok(render_context_to_markdown(
            &result,
            service.repo_root(),
            mode,
            depth,
            max_nodes,
            max_files,
        ))
    })?;

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
    let token_budget = get_usize_arg(args, "token_budget").unwrap_or(12_000);
    let model_context_window = get_usize_arg(args, "model_context_window").unwrap_or(128_000);

    let ranking = match get_str_arg(args, "ranking").unwrap_or("hybrid") {
        "graph" => RankingMode::Graph,
        "lexical" => RankingMode::Lexical,
        _ => RankingMode::Hybrid,
    };

    let packing = match get_str_arg(args, "packing").unwrap_or("sandwich") {
        "frontloaded" => ContextPackingMode::Frontloaded,
        "balanced" => ContextPackingMode::Balanced,
        _ => ContextPackingMode::Sandwich,
    };

    let include_tests = get_bool_arg(args, "include_tests", false);
    let include_unresolved = get_bool_arg(args, "include_unresolved", false);
    let no_snippets = get_bool_arg(args, "no_snippets", false);
    let context_lines = get_usize_arg(args, "context_lines").unwrap_or(3);
    let format = get_str_arg(args, "format").unwrap_or("text");
    let edge_kinds = parse_edge_kinds(&get_string_array(args, "edge_kind"));

    if format != "text" && format != "json" {
        return Ok(ToolCallOutcome::err(
            "format must be 'text' or 'json'",
        ));
    }

    let resolution = service
        .resolve_symbol(query)
        .map_err(|e| format!("Failed to resolve symbol: {}", e))?;

    match resolution {
        SymbolResolution::Ambiguous(candidates) => {
            let text = super::render::format_ambiguous_symbols(query, &candidates);
            Ok(ToolCallOutcome::ok(text))
        }
        SymbolResolution::NotFound => {
            Ok(ToolCallOutcome::err(format_symbol_not_found(query)))
        }
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

            let pack = retrieve_graph_context_with_options(
                &conn,
                &obj.qualified_name,
                &budget,
                &options,
            )
            .map_err(|e| format!("Failed to retrieve context: {}", e))?;

            let text = if format == "json" {
                serde_json::to_string_pretty(&pack)
                    .map_err(|e| format!("Failed to serialize context: {}", e))?
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
    let format_str = get_str_arg(args, "format").unwrap_or("markdown");
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

    let config = find_and_load_config(service.repo_root()).unwrap_or_default();
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
        let edges = load_callers(&conn, obj.id)
            .map_err(|e| format!("Failed to load callers: {}", e))?;
        Ok(render_caller_edges("Callers", &obj, &edges, service.repo_root()))
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
        let edges = load_callees(&conn, obj.id)
            .map_err(|e| format!("Failed to load callees: {}", e))?;
        Ok(render_call_edges("Callees", &obj, &edges, service.repo_root()))
    })?;

    let is_error = text.starts_with("Error:");
    Ok(ToolCallOutcome::text(text, is_error))
}

fn handle_rebuild_index(
    service: &GraphContextService,
    args: &serde_json::Value,
) -> Result<ToolCallOutcome, String> {
    let use_lsp = get_bool_arg(args, "use_lsp", true);
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
                "enum": ["text", "json"],
                "description": "Output format. Default: text."
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

