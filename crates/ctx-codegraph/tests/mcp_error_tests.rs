use ctx_codegraph::index::BuildIndexOptions;
use ctx_mcp::run_mcp_server_with_io;
use ctx_codegraph::storage::rebuild_index_db;
use std::fs;
use std::io::Cursor;
use tempfile::tempdir;

fn setup_project_with_index() -> (tempfile::TempDir, String) {
    let temp_dir = tempdir().unwrap();
    let root = temp_dir.path();

    let cargo_content = r#"
        [package]
        name = "temp_project"
        version = "0.1.0"
        edition = "2024"
    "#;
    fs::write(root.join("Cargo.toml"), cargo_content).unwrap();

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let lib_code = r#"
        pub fn run_pipeline() {
            let value = load();
            process(value);
        }

        fn load() -> i32 {
            1
        }

        fn process(value: i32) {
            save(value);
        }

        fn save(_: i32) {}
    "#;
    fs::write(src_dir.join("lib.rs"), lib_code).unwrap();

    rebuild_index_db(root, BuildIndexOptions::default()).unwrap();

    let root_uri = format!("file://{}", root.display());
    (temp_dir, root_uri)
}

fn setup_ambiguous_foo_project() -> (tempfile::TempDir, String) {
    let temp_dir = tempdir().unwrap();
    let root = temp_dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"ambiguous_foo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    fs::write(
        src_dir.join("mod_a.rs"),
        "pub fn foo() -> i32 { 1 }\n",
    )
    .unwrap();
    fs::write(
        src_dir.join("mod_b.rs"),
        "pub fn foo() -> i32 { 2 }\n",
    )
    .unwrap();
    fs::write(
        src_dir.join("lib.rs"),
        "pub mod mod_a;\npub mod mod_b;\n",
    )
    .unwrap();

    rebuild_index_db(root, BuildIndexOptions::default()).unwrap();

    let root_uri = format!("file://{}", root.display());
    (temp_dir, root_uri)
}

fn run_mcp_requests(requests: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let input_str: String = requests
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    run_mcp_raw(&input_str)
}

fn run_mcp_raw(input: &str) -> Vec<serde_json::Value> {
    let input = Cursor::new(input);
    let mut output = Vec::new();
    run_mcp_server_with_io(input, &mut output).unwrap();

    let output_str = String::from_utf8(output).unwrap();
    output_str
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn init_request(root_uri: &str, id: i64) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "workspaceFolders": [{ "uri": root_uri, "name": "test" }]
        }
    })
}

fn tool_call_request(id: i64, name: &str, arguments: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": arguments
        }
    })
}

fn assert_json_rpc_error(resp: &serde_json::Value, code: i64, message_contains: &str) {
    let error = resp.get("error").expect("expected JSON-RPC error");
    assert_eq!(error.get("code").unwrap().as_i64().unwrap(), code);
    let message = error.get("message").unwrap().as_str().unwrap();
    assert!(
        message.contains(message_contains),
        "expected message containing {:?}, got {:?}",
        message_contains,
        message
    );
}

fn assert_tool_error(resp: &serde_json::Value, message_contains: &str) {
    let result = resp.get("result").expect("expected tool result");
    assert_eq!(
        result.get("isError").unwrap().as_bool().unwrap(),
        true,
        "expected isError=true"
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains(message_contains),
        "expected tool text containing {:?}, got {:?}",
        message_contains,
        text
    );
}

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
        "get_graph_context",
        serde_json::json!({ "query": "run_pipeline" }),
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
            "get_graph_context",
            serde_json::json!({ "query": "definitely_not_a_symbol_xyz" }),
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
        tool_call_request(2, "get_graph_context", serde_json::json!({ "query": "foo" })),
    ];

    let responses = run_mcp_requests(&requests);
    let result = responses[1].get("result").unwrap();
    assert_eq!(result.get("isError").unwrap().as_bool().unwrap(), false);

    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Multiple symbols found"));
    // sig-enhanced disambig now surfaces signatures + loc (dense)
    assert!(text.contains("foo") && (text.contains("mod_a") || text.contains("mod_a.rs")));
    assert!(text.contains("foo") && (text.contains("mod_b") || text.contains("mod_b.rs")));
}

#[test]
fn test_mcp_error_get_callers_not_found() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let requests = vec![
        init_request(&root_uri, 1),
        tool_call_request(
            2,
            "get_callers",
            serde_json::json!({ "query": "definitely_not_a_symbol_xyz" }),
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
            "get_affected_context",
            serde_json::json!({
                "query": "run_pipeline",
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
            "get_affected_context",
            serde_json::json!({
                "query": "run_pipeline",
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
        message.contains("Index not found") || message.contains("Failed to load graph context"),
        "expected graceful initialize error, got: {message}"
    );
}

#[test]
fn test_mcp_error_initialize_missing_cargo_toml() {
    let temp_dir = tempdir().unwrap();
    let root = temp_dir.path();
    // Directory exists but has no Cargo.toml and no index.
    let root_uri = format!("file://{}", root.display());

    let responses = run_mcp_requests(&[init_request(&root_uri, 1)]);
    assert!(responses[0].get("error").is_some());
    let message = responses[0]["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("Index not found") || message.contains("ctx graph build"),
        "expected index-not-found style error, got: {message}"
    );
}

#[test]
fn test_mcp_rebuild_index_after_init() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let requests = vec![
        init_request(&root_uri, 1),
        tool_call_request(2, "rebuild_index", serde_json::json!({ "use_lsp": false })),
        tool_call_request(
            3,
            "get_graph_context",
            serde_json::json!({ "query": "run_pipeline" }),
        ),
    ];

    let responses = run_mcp_requests(&requests);

    let rebuild_result = responses[1].get("result").unwrap();
    assert_eq!(
        rebuild_result.get("isError").unwrap().as_bool().unwrap(),
        false
    );
    let rebuild_text = rebuild_result["content"][0]["text"].as_str().unwrap();
    assert!(rebuild_text.contains("Index rebuilt successfully"));

    let context_result = responses[2].get("result").unwrap();
    assert_eq!(
        context_result.get("isError").unwrap().as_bool().unwrap(),
        false
    );
    let context_text = context_result["content"][0]["text"].as_str().unwrap();
    assert!(context_text.contains("run_pipeline"));
}