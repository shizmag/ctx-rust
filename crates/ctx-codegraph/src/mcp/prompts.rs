pub const EXPLORE_SYMBOL_PROMPT: &str = "explore-symbol";

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
                 2. Call `get_affected_context` with the resolved symbol for budget-aware LLM context.\n\
                 3. Call `get_callers` and `get_callees` for direct relationships.\n\
                 4. Call `get_graph_context` with mode `neighborhood` or `impact` for deeper graph traversal.\n\
                 5. If the index is missing, run `ctx graph build --with-lsp` or call `rebuild_index`.\n\n\
                 When a tool returns multiple symbol candidates, refine `query` with a qualified path and retry."
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
        _ => Err(format!("Unknown prompt: {}", name)),
    }
}