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

fn run_mcp_requests(requests: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let input_str: String = requests
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    let input = Cursor::new(input_str);
    let mut output = Vec::new();
    run_mcp_server_with_io(input, &mut output).unwrap();

    let output_str = String::from_utf8(output).unwrap();
    output_str
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn test_mcp_server_flow() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let init_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "workspaceFolders": [
                {
                    "uri": root_uri,
                    "name": "test"
                }
            ]
        }
    });

    let list_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    });

    let call_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "get_graph_context",
            "arguments": {
                "query": "run_pipeline"
            }
        }
    });

    let responses = run_mcp_requests(&[init_request, list_request, call_request]);
    assert_eq!(responses.len(), 3);

    let init_resp = &responses[0];
    assert_eq!(init_resp.get("id").unwrap().as_i64().unwrap(), 1);
    assert!(init_resp.get("result").is_some());

    let list_resp = &responses[1];
    assert_eq!(list_resp.get("id").unwrap().as_i64().unwrap(), 2);
    let tools = list_resp
        .get("result")
        .unwrap()
        .get("tools")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(tools.len(), 9);

    let tool_names: Vec<&str> = tools
        .iter()
        .map(|t| t.get("name").unwrap().as_str().unwrap())
        .collect();
    assert!(tool_names.contains(&"get_graph_context"));
    assert!(tool_names.contains(&"get_affected_context"));
    assert!(tool_names.contains(&"rebuild_index"));
    assert!(tool_names.contains(&"read_file"));
    assert!(tool_names.contains(&"search_code"));

    let call_resp = &responses[2];
    assert_eq!(call_resp.get("id").unwrap().as_i64().unwrap(), 3);
    let result = call_resp.get("result").unwrap();
    assert_eq!(result.get("isError").unwrap().as_bool().unwrap(), false);

    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    assert!(text.contains("# Graph Context"));
    assert!(text.contains("Root: fn run_pipeline"));
    assert!(text.contains("lib::run_pipeline -> lib::load"));
    assert!(text.contains("lib::run_pipeline -> lib::process"));
}

#[test]
fn test_mcp_initialize_succeeds_without_index_file_tools_work() {
    let temp_dir = tempdir().unwrap();
    let root = temp_dir.path();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"no_index\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    let src = root.join("src");
    let _ = fs::create_dir(&src);
    fs::write(src.join("lib.rs"), "pub fn example() { let x = 42; /* searchme */ }\n").unwrap();

    let root_uri = format!("file://{}", root.display());
    let init_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "workspaceFolders": [{ "uri": root_uri, "name": "test" }]
        }
    });

    // Send init + list + file tool calls in single server run (each run_mcp_requests starts fresh server).
    let list_req = serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
    let read_req = serde_json::json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": { "name": "read_file", "arguments": { "path": "src/lib.rs", "max_lines": 5 } }
    });
    let search_req = serde_json::json!({
        "jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": { "name": "search_code", "arguments": { "query": "searchme", "path_filter": "src" } }
    });
    let responses = run_mcp_requests(&[init_request, list_req, read_req, search_req]);
    let resp = &responses[0];
    // Init now succeeds without index to enable read_file/search_code (key for agent adoption).
    assert!(resp.get("result").is_some(), "init must succeed for file tools");
    assert!(resp.get("error").is_none());

    // Verify new tools are listed and functional without index.
    let tools = responses[1]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"read_file"));
    assert!(names.contains(&"search_code"));

    let read_text = responses[2]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(read_text.contains("example()"));

    let search_text = responses[3]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(search_text.contains("searchme"));
}

#[test]
fn test_mcp_ping_and_resources() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let requests = vec![
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "workspaceFolders": [{ "uri": root_uri, "name": "test" }] }
        }),
        serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }),
        serde_json::json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/list" }),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "resources/read",
            "params": { "uri": "ctx://index/status" }
        }),
        serde_json::json!({ "jsonrpc": "2.0", "id": 5, "method": "prompts/list" }),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "prompts/get",
            "params": { "name": "explore-symbol", "arguments": { "symbol": "run_pipeline" } }
        }),
    ];

    let responses = run_mcp_requests(&requests);
    assert_eq!(responses.len(), 6);

    assert!(responses[1].get("result").unwrap().is_object());
    let resources = responses[2]
        .get("result")
        .unwrap()
        .get("resources")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(resources.len(), 3);  // includes ctx://stats/mcp for metrics collection

    let status_text = responses[3]
        .get("result")
        .unwrap()["contents"][0]["text"]
        .as_str()
        .unwrap();
    assert!(status_text.contains("Codegraph Index Status"));

    let prompts = responses[4]
        .get("result")
        .unwrap()
        .get("prompts")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(prompts[0]["name"], "explore-symbol");

    let prompt_text = responses[5]
        .get("result")
        .unwrap()["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(prompt_text.contains("get_affected_context"));
}

#[test]
fn test_mcp_list_symbols_and_callers() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let requests = vec![
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "workspaceFolders": [{ "uri": root_uri, "name": "test" }] }
        }),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "list_symbols",
                "arguments": { "query": "load", "limit": 10, "kind": "fn" }
            }
        }),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "get_callers",
                "arguments": { "query": "load" }
            }
        }),
    ];

    let responses = run_mcp_requests(&requests);

    let list_text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    // Updated for sig-enhanced denser list output (signatures surfaced)
    assert!(list_text.contains("fn load()") || list_text.contains("load"));

    let callers_text = responses[2]["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(callers_text.contains("Callers"));
    assert!(callers_text.contains("lib::run_pipeline"));
}

#[test]
fn test_mcp_notification_does_not_respond() {
    let (_temp_dir, root_uri) = setup_project_with_index();

    let input_str = format!(
        "{}\n{}\n",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "workspaceFolders": [{ "uri": root_uri, "name": "test" }] }
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
    let lines: Vec<&str> = output_str.lines().collect();
    assert_eq!(lines.len(), 1, "notifications must not produce a response");
}