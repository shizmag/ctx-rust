use ctx_codegraph::index::BuildIndexOptions;
use ctx_codegraph::mcp::run_mcp_server_with_io;
use ctx_codegraph::storage::rebuild_index_db;
use std::fs;
use std::io::Cursor;
use tempfile::tempdir;

const EXPECTED_TOOLS: &[&str] = &[
    "get_affected_context",
    "get_graph_context",
    "get_project_context",
    "list_symbols",
    "get_callers",
    "get_callees",
    "rebuild_index",
];

fn setup_project_with_index() -> (tempfile::TempDir, String) {
    let temp_dir = tempdir().unwrap();
    let root = temp_dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"coverage_project\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(
        src_dir.join("lib.rs"),
        r#"
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
        "#,
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
            "workspaceFolders": [{ "uri": root_uri, "name": "coverage" }]
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

#[test]
fn test_mcp_coverage_tools_list_returns_all_tools() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let responses = run_mcp_requests(&[
        init_request(&root_uri, 1),
        serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    ]);

    let tools = responses[1]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), EXPECTED_TOOLS.len());

    let names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for expected in EXPECTED_TOOLS {
        assert!(names.contains(expected), "missing tool: {expected}");
    }
}

#[test]
fn test_mcp_coverage_resources_list_and_read() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let responses = run_mcp_requests(&[
        init_request(&root_uri, 1),
        serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/list" }),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "resources/read",
            "params": { "uri": "ctx://index/status" }
        }),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "resources/read",
            "params": { "uri": "ctx://project/tree" }
        }),
    ]);

    let resources = responses[1]["result"]["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 2);
    let uris: Vec<&str> = resources
        .iter()
        .map(|r| r["uri"].as_str().unwrap())
        .collect();
    assert!(uris.contains(&"ctx://index/status"));
    assert!(uris.contains(&"ctx://project/tree"));

    let status_text = responses[2]["result"]["contents"][0]["text"]
        .as_str()
        .unwrap();
    assert!(status_text.contains("Codegraph Index Status"));
    assert!(status_text.contains("Symbols:"));

    let tree_text = responses[3]["result"]["contents"][0]["text"]
        .as_str()
        .unwrap();
    assert!(tree_text.contains("Project Tree Summary"));
    assert!(tree_text.contains("Cargo.toml"));
}

#[test]
fn test_mcp_coverage_prompts_list_and_get() {
    let responses = run_mcp_requests(&[
        serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "prompts/list" }),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "prompts/get",
            "params": {
                "name": "explore-symbol",
                "arguments": { "symbol": "run_pipeline" }
            }
        }),
    ]);

    let prompts = responses[0]["result"]["prompts"].as_array().unwrap();
    assert_eq!(prompts[0]["name"], "explore-symbol");

    let prompt_text = responses[1]["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(prompt_text.contains("run_pipeline"));
    assert!(prompt_text.contains("get_affected_context"));
    assert!(prompt_text.contains("get_callers"));
}

#[test]
fn test_mcp_coverage_tool_calls_happy_paths() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let responses = run_mcp_requests(&[
        init_request(&root_uri, 1),
        tool_call_request(
            2,
            "list_symbols",
            serde_json::json!({ "query": "load", "limit": 5 }),
        ),
        tool_call_request(3, "get_callers", serde_json::json!({ "query": "load" })),
        tool_call_request(4, "get_callees", serde_json::json!({ "query": "run_pipeline" })),
        tool_call_request(
            5,
            "get_affected_context",
            serde_json::json!({ "query": "run_pipeline" }),
        ),
        tool_call_request(
            6,
            "get_project_context",
            serde_json::json!({ "mode": "smart", "format": "markdown" }),
        ),
        tool_call_request(7, "rebuild_index", serde_json::json!({ "use_lsp": false })),
    ]);

    for (idx, name) in [
        "list_symbols",
        "get_callers",
        "get_callees",
        "get_affected_context",
        "get_project_context",
        "rebuild_index",
    ]
    .iter()
    .enumerate()
    {
        let result = &responses[idx + 1]["result"];
        assert_eq!(
            result["isError"].as_bool().unwrap(),
            false,
            "{name} should succeed"
        );
    }

    assert!(responses[1]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("lib::load"));
    assert!(responses[2]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Callers"));
    assert!(responses[3]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Callees"));
    assert!(responses[4]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("run_pipeline"));
    assert!(responses[5]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("coverage_project"));
    assert!(responses[6]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Index rebuilt successfully"));
}

#[test]
fn test_mcp_coverage_ping_returns_empty_result() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let responses = run_mcp_requests(&[
        init_request(&root_uri, 1),
        serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }),
    ]);

    assert_eq!(responses[1]["id"], 2);
    assert!(responses[1]["result"].is_object());
}

#[test]
fn test_mcp_coverage_initialized_notification_does_not_crash() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let input_str = format!(
        "{}\n{}\n",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "workspaceFolders": [{ "uri": root_uri, "name": "coverage" }] }
        }),
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        })
    );

    let input = Cursor::new(input_str);
    let mut output = Vec::new();
    run_mcp_server_with_io(input, &mut output).unwrap();

    let output_str = String::from_utf8(output).unwrap();
    let lines: Vec<&str> = output_str
        .lines()
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(lines.len(), 1, "notification must not produce a response");
}

#[test]
fn test_mcp_coverage_method_not_found_error() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let responses = run_mcp_requests(&[
        init_request(&root_uri, 1),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "does/not/exist"
        }),
    ]);

    let error = &responses[1]["error"];
    assert_eq!(error["code"].as_i64().unwrap(), -32601);
    assert!(error["message"]
        .as_str()
        .unwrap()
        .contains("Method not found"));
}