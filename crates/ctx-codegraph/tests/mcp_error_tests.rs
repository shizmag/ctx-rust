mod common;

use common::{
    assert_json_rpc_error, assert_tool_error, init_request, run_mcp_raw, run_mcp_requests,
    setup_ambiguous_foo_project, setup_project_with_index, tool_call_request,
};
use tempfile::tempdir;

// --- Protocol / JSON-RPC errors ---

#[test]
fn test_mcp_error_malformed_json() {
    let input = "{ not valid json }\n";
    let responses = run_mcp_raw(input);
    assert_eq!(responses.len(), 1);
    assert_json_rpc_error(&responses[0], -32700, "Parse error");
    assert!(responses[0].get("id").unwrap().is_null());
}

#[test]
fn test_mcp_error_unknown_method() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let requests = vec![
        init_request(&root_uri, 1),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "foo/bar"
        }),
    ];

    let responses = run_mcp_requests(&requests);
    assert_json_rpc_error(&responses[1], -32601, "Method not found: foo/bar");
}

#[test]
fn test_mcp_error_tools_call_before_initialize() {
    let requests = vec![tool_call_request(
        1,
        "retrieve_context",
        serde_json::json!({ "query": "run_pipeline", "strategy": "graph" }),
    )];

    let responses = run_mcp_requests(&requests);
    assert_json_rpc_error(&responses[0], -32000, "not initialized");
}

#[test]
fn test_mcp_error_unknown_tool_name() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let requests = vec![
        init_request(&root_uri, 1),
        tool_call_request(2, "nonexistent_tool", serde_json::json!({})),
    ];

    let responses = run_mcp_requests(&requests);
    assert_json_rpc_error(&responses[1], -32601, "Unknown tool: nonexistent_tool");
}

#[test]
fn test_mcp_error_empty_line_skipped() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let init = serde_json::to_string(&init_request(&root_uri, 1)).unwrap();
    let ping = serde_json::to_string(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "ping"
    }))
    .unwrap();

    let input = format!("\n\n{init}\n\n\n{ping}\n");
    let responses = run_mcp_raw(&input);
    assert_eq!(responses.len(), 2);
    assert!(responses[0].get("result").is_some());
    assert!(responses[1].get("result").is_some());
}

// --- Tool error paths ---

#[test]
fn test_mcp_error_symbol_not_found() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let requests = vec![
        init_request(&root_uri, 1),
        tool_call_request(
            2,
            "retrieve_context",
            serde_json::json!({
                "query": "definitely_not_a_symbol_xyz",
                "strategy": "graph"
            }),
        ),
    ];

    let responses = run_mcp_requests(&requests);
    assert_tool_error(&responses[1], "Symbol not found");
    assert_tool_error(&responses[1], "definitely_not_a_symbol_xyz");
}

#[test]
fn test_mcp_error_ambiguous_symbol() {
    let (_temp_dir, root_uri) = setup_ambiguous_foo_project();

    let requests = vec![
        init_request(&root_uri, 1),
        tool_call_request(
            2,
            "retrieve_context",
            serde_json::json!({ "query": "foo", "strategy": "graph", "format": "text" }),
        ),
    ];

    let responses = run_mcp_requests(&requests);
    let result = responses[1].get("result").unwrap();
    assert!(!result.get("isError").unwrap().as_bool().unwrap());

    let text = result["content"][0]["text"].as_str().unwrap();
    // Multiple exact name matches are returned as roots (agent should refine with list_symbols).
    assert!(text.contains("foo"));
    assert!(text.contains("mod_a") || text.contains("mod_a.rs"));
    assert!(text.contains("mod_b") || text.contains("mod_b.rs"));
}

#[test]
fn test_mcp_error_get_callers_not_found() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let requests = vec![
        init_request(&root_uri, 1),
        tool_call_request(
            2,
            "retrieve_context",
            serde_json::json!({
                "query": "definitely_not_a_symbol_xyz",
                "strategy": "graph",
                "graph_mode": "callers"
            }),
        ),
    ];

    let responses = run_mcp_requests(&requests);
    assert_tool_error(&responses[1], "Symbol not found");
}

#[test]
fn test_mcp_error_affected_context_invalid_depth() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let requests = vec![
        init_request(&root_uri, 1),
        tool_call_request(
            2,
            "retrieve_context",
            serde_json::json!({
                "query": "run_pipeline",
                "strategy": "graph",
                "depth": "not_a_number"
            }),
        ),
    ];

    let responses = run_mcp_requests(&requests);
    assert_json_rpc_error(
        &responses[1],
        -32601,
        "depth must be a non-negative integer or \"auto\"",
    );
}

#[test]
fn test_mcp_error_affected_context_invalid_format() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let requests = vec![
        init_request(&root_uri, 1),
        tool_call_request(
            2,
            "retrieve_context",
            serde_json::json!({
                "query": "run_pipeline",
                "strategy": "graph",
                "format": "xml"
            }),
        ),
    ];

    let responses = run_mcp_requests(&requests);
    assert_tool_error(&responses[1], "format must be 'text', 'json' or 'yaml'");
}

#[test]
fn test_mcp_error_resources_read_unknown_uri() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let requests = vec![
        init_request(&root_uri, 1),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "resources/read",
            "params": { "uri": "ctx://unknown/resource" }
        }),
    ];

    let responses = run_mcp_requests(&requests);
    assert_json_rpc_error(&responses[1], -32603, "Unknown resource: ctx://unknown/resource");
}

#[test]
fn test_mcp_error_prompts_get_unknown_prompt() {
    let requests = vec![serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "prompts/get",
        "params": { "name": "nonexistent_prompt" }
    })];

    let responses = run_mcp_requests(&requests);
    assert_json_rpc_error(&responses[0], -32601, "Unknown prompt: nonexistent_prompt");
}

#[test]
fn test_mcp_error_prompts_get_missing_symbol_argument() {
    let requests = vec![serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "prompts/get",
        "params": { "name": "explore-symbol" }
    })];

    let responses = run_mcp_requests(&requests);
    assert_json_rpc_error(&responses[0], -32601, "symbol argument is required");
}

// --- Initialize edge cases ---

#[test]
fn test_mcp_error_initialize_nonexistent_workspace() {
    let requests = vec![serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "workspaceFolders": [{
                "uri": "file:///this/path/does/not/exist/ctx_mcp_test",
                "name": "missing"
            }]
        }
    })];

    let responses = run_mcp_requests(&requests);
    assert!(responses[0].get("error").is_some());
    let message = responses[0]["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("does not exist") || message.contains("Workspace"),
        "expected workspace-does-not-exist error, got: {message}"
    );
}

#[test]
fn test_mcp_initialize_succeeds_without_index_for_file_tools() {
    let temp_dir = tempdir().unwrap();
    let root = temp_dir.path();
    // Directory exists but has no index (and no Cargo.toml). File tools should still be available.
    let root_uri = format!("file://{}", root.display());

    let responses = run_mcp_requests(&[init_request(&root_uri, 1)]);
    // Should succeed (not error) so read_file works even without graph index.
    assert!(responses[0].get("result").is_some(), "expected successful init for file tools");
    assert!(responses[0].get("error").is_none());
}

#[test]
fn test_mcp_rebuild_index_after_init() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let requests = vec![
        init_request(&root_uri, 1),
        tool_call_request(2, "rebuild_index", serde_json::json!({ "use_lsp": false })),
        tool_call_request(
            3,
            "retrieve_context",
            serde_json::json!({ "query": "run_pipeline", "strategy": "graph", "format": "text" }),
        ),
    ];

    let responses = run_mcp_requests(&requests);

    let rebuild_result = responses[1].get("result").unwrap();
    assert!(!rebuild_result.get("isError").unwrap().as_bool().unwrap());
    let rebuild_text = rebuild_result["content"][0]["text"].as_str().unwrap();
    assert!(rebuild_text.contains("Index rebuilt successfully"));

    let context_result = responses[2].get("result").unwrap();
    assert!(!context_result.get("isError").unwrap().as_bool().unwrap());
    let context_text = context_result["content"][0]["text"].as_str().unwrap();
    assert!(context_text.contains("run_pipeline"));
}