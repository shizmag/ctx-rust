use ctx_codegraph::storage::rebuild_index_db;
use ctx_codegraph_store::test_fixtures::{no_search_options, with_isolated_global_config};
use ctx_mcp::run_mcp_server_with_io;
use std::fs;
use std::io::Cursor;
use tempfile::tempdir;

fn setup_named_project_with_index(package_name: &str) -> (tempfile::TempDir, String) {
    let temp_dir = tempdir().unwrap();
    let root = temp_dir.path();

    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
        ),
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

    with_isolated_global_config(|| {
        rebuild_index_db(root, no_search_options()).unwrap();
    });

    let root_uri = format!("file://{}", root.display());
    (temp_dir, root_uri)
}

pub fn setup_project_with_index() -> (tempfile::TempDir, String) {
    setup_named_project_with_index("temp_project")
}

pub fn setup_coverage_project_with_index() -> (tempfile::TempDir, String) {
    setup_named_project_with_index("coverage_project")
}

pub fn setup_ambiguous_foo_project() -> (tempfile::TempDir, String) {
    let temp_dir = tempdir().unwrap();
    let root = temp_dir.path();

    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"ambiguous_foo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    fs::write(src_dir.join("mod_a.rs"), "pub fn foo() -> i32 { 1 }\n").unwrap();
    fs::write(src_dir.join("mod_b.rs"), "pub fn foo() -> i32 { 2 }\n").unwrap();
    fs::write(src_dir.join("lib.rs"), "pub mod mod_a;\npub mod mod_b;\n").unwrap();

    with_isolated_global_config(|| {
        rebuild_index_db(root, no_search_options()).unwrap();
    });

    let root_uri = format!("file://{}", root.display());
    (temp_dir, root_uri)
}

pub fn run_mcp_requests(requests: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let input_str: String = requests
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    run_mcp_raw(&input_str)
}

pub fn run_mcp_raw(input: &str) -> Vec<serde_json::Value> {
    let mut responses = Vec::new();
    with_isolated_global_config(|| {
        let input = Cursor::new(input);
        let mut output = Vec::new();
        run_mcp_server_with_io(input, &mut output).unwrap();

        let output_str = String::from_utf8(output).unwrap();
        responses = output_str
            .lines()
            .filter(|l| !l.is_empty())
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    });
    responses
}

pub fn init_request(root_uri: &str, id: i64) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "workspaceFolders": [{ "uri": root_uri, "name": "test" }]
        }
    })
}

pub fn tool_call_request(id: i64, name: &str, arguments: serde_json::Value) -> serde_json::Value {
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

pub fn assert_json_rpc_error(resp: &serde_json::Value, code: i64, message_contains: &str) {
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

pub fn assert_tool_error(resp: &serde_json::Value, message_contains: &str) {
    let result = resp.get("result").expect("expected tool result");
    assert!(
        result.get("isError").unwrap().as_bool().unwrap(),
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