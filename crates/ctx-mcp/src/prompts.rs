pub const EXPLORE_SYMBOL_PROMPT: &str = "explore-symbol";
pub const ANALYZE_IMPACT_PROMPT: &str = "analyze-impact";
pub const TRACE_CALLERS_PROMPT: &str = "trace-callers";
pub const GET_CONTEXT_FOR_TASK_PROMPT: &str = "get-context-for-task";

pub fn list_prompts() -> serde_json::Value {
    serde_json::json!({
        "prompts": [
            {
                "name": EXPLORE_SYMBOL_PROMPT,
                "description": "Explore a symbol using ctx codegraph tools",
                "arguments": [
                    {
                        "name": "symbol",
                        "description": "Symbol name or qualified path to explore",
                        "required": true
                    }
                ]
            },
            {
                "name": ANALYZE_IMPACT_PROMPT,
                "description": "Analyze change impact for a symbol or area using graph and affected context tools",
                "arguments": [
                    {
                        "name": "symbol",
                        "description": "Symbol or area to analyze for impact",
                        "required": true
                    }
                ]
            },
            {
                "name": TRACE_CALLERS_PROMPT,
                "description": "Trace callers (and potentially transitive) for a symbol to understand usage",
                "arguments": [
                    {
                        "name": "symbol",
                        "description": "Symbol whose callers to trace",
                        "required": true
                    }
                ]
            },
            {
                "name": GET_CONTEXT_FOR_TASK_PROMPT,
                "description": "Get packed context suitable for performing a described coding task",
                "arguments": [
                    {
                        "name": "task",
                        "description": "Short description of the task or change",
                        "required": true
                    },
                    {
                        "name": "focus_symbol",
                        "description": "Optional primary symbol to start from",
                        "required": false
                    }
                ]
            }
        ]
    })
}

pub fn get_prompt(name: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    match name {
        EXPLORE_SYMBOL_PROMPT => {
            let symbol = args
                .get("symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "symbol argument is required".to_string())?;

            let text = format!(
                "Explore the symbol `{symbol}` in this codebase using ctx MCP tools.\n\n\
                 Suggested workflow:\n\
                 1. Call `list_symbols` with query `{symbol}` if the name may be ambiguous.\n\
                 2. Call `retrieve_context` with the resolved symbol (format=yaml) for budget-aware LLM context.\n\
                 3. For direct relationships use `retrieve_context` with strategy=graph, graph_mode=callers or graph_mode=callees.\n\
                 4. For deeper graph traversal use `retrieve_context` with strategy=graph, graph_mode=neighborhood or graph_mode=impact.\n\
                 5. Use `read_file` proactively for exact full/partial file source when packed snippets are not enough.\n\
                 6. For text search in comments/strings/configs use `retrieve_context` with strategy=hybrid or strategy=lexical.\n\
                 7. If the index is missing, call `rebuild_index` (or `ctx graph build --with-lsp`).\n\n\
                 When results are ambiguous, refine `query` with a qualified path from list_symbols and retry."
            );

            Ok(serde_json::json!({
                "description": format!("Explore symbol: {}", symbol),
                "messages": [
                    {
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": text
                        }
                    }
                ]
            }))
        }
        ANALYZE_IMPACT_PROMPT => {
            let symbol = args
                .get("symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "symbol argument is required".to_string())?;

            let text = format!(
                "Analyze the impact of changes to `{symbol}` using ctx MCP tools.\n\n\
                 Suggested multi-step workflow:\n\
                 1. Use `list_symbols` (query=`{symbol}`) or get precise name.\n\
                 2. Call `retrieve_context` (query=`{symbol}`, format=`yaml`, strategy=`graph`, graph_mode=`impact` or `neighborhood`) for ranked packed context + diagnostics.\n\
                 3. Call `retrieve_context` (query=`{symbol}`, strategy=`graph`, graph_mode=`dependents` or `callers`) for relationship context.\n\
                 4. If needed call `retrieve_context` again with broader depth or strategy=`hybrid`.\n\n\
                 Report: roots affected, estimated tokens, key files, potential breakage points. Use yaml for structured results."
            );

            Ok(serde_json::json!({
                "description": format!("Analyze impact: {}", symbol),
                "messages": [
                    {
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": text
                        }
                    }
                ]
            }))
        }
        TRACE_CALLERS_PROMPT => {
            let symbol = args
                .get("symbol")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "symbol argument is required".to_string())?;

            let text = format!(
                "Trace callers of `{symbol}` (and usage) using ctx tools.\n\n\
                 Workflow:\n\
                 1. `list_symbols` query=`{symbol}` to disambiguate.\n\
                 2. `retrieve_context` query=`{symbol}`, strategy=`graph`, graph_mode=`callers` for direct callers.\n\
                 3. For transitive callers increase depth or repeat retrieve_context on interesting symbols.\n\
                 4. `retrieve_context` query=`{symbol}`, strategy=`graph`, graph_mode=`callers`, format=`yaml` for packed view.\n\
                 Summarize call chain and entry points."
            );

            Ok(serde_json::json!({
                "description": format!("Trace callers: {}", symbol),
                "messages": [
                    {
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": text
                        }
                    }
                ]
            }))
        }
        GET_CONTEXT_FOR_TASK_PROMPT => {
            let task = args
                .get("task")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "task argument is required".to_string())?;
            let focus = args
                .get("focus_symbol")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let focus_line = if focus.is_empty() {
                String::new()
            } else {
                format!(" Primary focus symbol: `{}`.\n", focus)
            };

            let text = format!(
                "Gather the right context to perform this task: `{task}`.{focus_line}\n\n\
                 Multi-step plan:\n\
                 1. If focus given, start with `list_symbols` + `retrieve_context` (format=yaml, suitable token_budget).\n\
                 2. Use `retrieve_context` with strategy=graph and graph_mode=callers/callees/neighborhood to pull related symbols.\n\
                 3. If broad change, use `retrieve_context` with strategy=graph, graph_mode=`impact`.\n\
                 4. For text search use `retrieve_context` with strategy=hybrid or lexical.\n\
                 5. Call `get_project_context` only if you need surrounding file structure.\n\
                 Prefer yaml outputs. Stop when you have sufficient nodes/files under budget for the task."
            );

            Ok(serde_json::json!({
                "description": format!("Context for task: {}", task),
                "messages": [
                    {
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": text
                        }
                    }
                ]
            }))
        }
        _ => Err(format!("Unknown prompt: {}", name)),
    }
}
