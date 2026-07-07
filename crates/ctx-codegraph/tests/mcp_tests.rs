use ctx_codegraph::mcp::run_mcp_server_with_io;
use std::fs;
use std::io::Cursor;
use tempfile::tempdir;

#[test]
fn test_mcp_server_flow() {
    let temp_dir = tempdir().unwrap();
    let root = temp_dir.path();

    // Create Cargo.toml
    let cargo_content = r#"
        [package]
        name = "temp_project"
        version = "0.1.0"
        edition = "2024"
    "#;
    fs::write(root.join("Cargo.toml"), cargo_content).unwrap();

    // Create src directory and src/lib.rs
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

    // Prepare JSON-RPC input messages
    let init_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "workspaceFolders": [
                {
                    "uri": format!("file://{}", root.display()),
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

    let input_str = format!(
        "{}\n{}\n{}\n",
        serde_json::to_string(&init_request).unwrap(),
        serde_json::to_string(&list_request).unwrap(),
        serde_json::to_string(&call_request).unwrap()
    );

    let input = Cursor::new(input_str);
    let mut output = Vec::new();

    // Run MCP server in-memory
    run_mcp_server_with_io(input, &mut output).unwrap();

    let output_str = String::from_utf8(output).unwrap();
    let lines: Vec<&str> = output_str.lines().collect();

    assert_eq!(lines.len(), 3);

    // 1. Verify initialize response
    let init_resp: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(init_resp.get("id").unwrap().as_i64().unwrap(), 1);
    assert!(init_resp.get("result").is_some());

    // 2. Verify tools/list response
    let list_resp: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(list_resp.get("id").unwrap().as_i64().unwrap(), 2);
    let tools = list_resp
        .get("result")
        .unwrap()
        .get("tools")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].get("name").unwrap().as_str().unwrap(), "get_graph_context");

    // 3. Verify tools/call response
    let call_resp: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(call_resp.get("id").unwrap().as_i64().unwrap(), 3);
    let result = call_resp.get("result").unwrap();
    assert_eq!(result.get("isError").unwrap().as_bool().unwrap(), false);
    
    let content = result.get("content").unwrap().as_array().unwrap();
    assert_eq!(content.len(), 1);
    assert_eq!(content[0].get("type").unwrap().as_str().unwrap(), "text");
    
    let text = content[0].get("text").unwrap().as_str().unwrap();
    assert!(text.contains("# Graph Context"));
    assert!(text.contains("Root: fn run_pipeline"));
    assert!(text.contains("lib::run_pipeline -> lib::load"));
    assert!(text.contains("lib::run_pipeline -> lib::process"));
}
