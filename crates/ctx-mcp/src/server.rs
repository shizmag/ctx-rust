use super::protocol::{
    INTERNAL_ERROR, METHOD_NOT_FOUND, PARSE_ERROR, SERVER_NOT_INITIALIZED, JsonRpcRequest,
    JsonRpcResponse, get_workspace_path,
};
use super::prompts;
use super::resources;
use super::tools;
use ctx_codegraph::error::CodeGraphError;
use ctx_codegraph::service::GraphContextService;
use std::io::{self, BufRead, Write};

pub fn run_mcp_server() -> Result<(), Box<dyn std::error::Error>> {
    run_mcp_server_with_io(io::stdin().lock(), io::stdout())
}

pub fn run_mcp_server_with_io<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut line = String::new();
    let mut service_opt: Option<GraphContextService> = None;
    let mut initialized = false;

    eprintln!("MCP Server started. Waiting for messages...");

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }

        let line_trimmed = line.trim();
        if line_trimmed.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(line_trimmed) {
            Ok(req) => req,
            Err(e) => {
                let resp = JsonRpcResponse::error(
                    serde_json::Value::Null,
                    PARSE_ERROR,
                    format!("Parse error: {}", e),
                );
                write_response(&mut writer, &resp)?;
                continue;
            }
        };

        if request.id.is_none() {
            match request.method.as_str() {
                "notifications/initialized" => {
                    eprintln!("Client initialized.");
                }
                other => {
                    eprintln!("Ignored notification: {}", other);
                }
            }
            continue;
        }

        let request_id = request.id.clone().unwrap_or(serde_json::Value::Null);
        let response = dispatch_request(
            &request,
            request_id,
            &mut service_opt,
            &mut initialized,
        );

        write_response(&mut writer, &response)?;
    }

    // Log usage summary to stderr on shutdown for collection (metrics for MCP vs other tools)
    eprintln!("\n{}", tools::usage_summary_text());
    eprintln!("MCP Server shutting down.");

    Ok(())
}

fn write_response<W: Write>(writer: &mut W, response: &JsonRpcResponse) -> io::Result<()> {
    let response_str = serde_json::to_string(response).map_err(io::Error::other)?;
    writeln!(writer, "{}", response_str)?;
    writer.flush()
}

fn dispatch_request(
    request: &JsonRpcRequest,
    request_id: serde_json::Value,
    service_opt: &mut Option<GraphContextService>,
    initialized: &mut bool,
) -> JsonRpcResponse {
    match request.method.as_str() {
        "initialize" => handle_initialize(request, request_id, service_opt, initialized),
        "ping" => JsonRpcResponse::success(request_id, serde_json::json!({})),
        "tools/list" => {
            if !*initialized {
                return not_initialized(request_id);
            }
            JsonRpcResponse::success(request_id, tools::list_tools())
        }
        "tools/call" => handle_tools_call(request, request_id, service_opt, initialized),
        "resources/list" => {
            if !*initialized {
                return not_initialized(request_id);
            }
            JsonRpcResponse::success(request_id, resources::list_resources())
        }
        "resources/read" => handle_resources_read(request, request_id, service_opt, initialized),
        "prompts/list" => JsonRpcResponse::success(request_id, prompts::list_prompts()),
        "prompts/get" => handle_prompts_get(request, request_id),
        _ => JsonRpcResponse::error(
            request_id,
            METHOD_NOT_FOUND,
            format!("Method not found: {}", request.method),
        ),
    }
}

fn handle_initialize(
    request: &JsonRpcRequest,
    request_id: serde_json::Value,
    service_opt: &mut Option<GraphContextService>,
    initialized: &mut bool,
) -> JsonRpcResponse {
    let params = request.params.as_ref().cloned().unwrap_or(serde_json::Value::Null);
    let ws_path = get_workspace_path(&params);
    eprintln!(
        "Initializing MCP Server for workspace: {}",
        ws_path.display()
    );

    match GraphContextService::load_only(&ws_path) {
        Ok(service) => {
            eprintln!("Index loaded from {}", service.repo_root().display());
            *service_opt = Some(service);
            *initialized = true;
            JsonRpcResponse::success(
                request_id,
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {},
                        "resources": {},
                        "prompts": {}
                    },
                    "serverInfo": {
                        "name": "ctx-codegraph-mcp",
                        "version": "0.1.0"
                    }
                }),
            )
        }
        Err(CodeGraphError::IndexNotFound(msg)) => {
            eprintln!("Index not found — run `ctx graph build --with-lsp`");
            JsonRpcResponse::error(request_id, INTERNAL_ERROR, msg)
        }
        Err(e) => JsonRpcResponse::error(
            request_id,
            INTERNAL_ERROR,
            format!("Failed to load graph context: {}", e),
        ),
    }
}

fn handle_tools_call(
    request: &JsonRpcRequest,
    request_id: serde_json::Value,
    service_opt: &mut Option<GraphContextService>,
    initialized: &bool,
) -> JsonRpcResponse {
    if !*initialized {
        return not_initialized(request_id);
    }

    let params = request.params.as_ref().cloned().unwrap_or(serde_json::Value::Null);
    let tool_name = params
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::Value::Object(Default::default()));

    let service = match service_opt.as_ref() {
        Some(s) => s,
        None => return not_initialized(request_id),
    };

    match tools::handle_tool_call(service, tool_name, &args) {
        Ok(outcome) => {
            if outcome.reload_service
                && let Some(current) = service_opt.as_ref()
            {
                match GraphContextService::load_only(current.repo_root()) {
                    Ok(reloaded) => {
                        eprintln!("Index reloaded after rebuild.");
                        *service_opt = Some(reloaded);
                    }
                    Err(e) => {
                        return JsonRpcResponse::error(
                            request_id,
                            INTERNAL_ERROR,
                            format!("Index rebuilt but failed to reload: {}", e),
                        );
                    }
                }
            }
            JsonRpcResponse::success(request_id, outcome.result)
        }
        Err(msg) => JsonRpcResponse::error(request_id, METHOD_NOT_FOUND, msg),
    }
}

fn handle_resources_read(
    request: &JsonRpcRequest,
    request_id: serde_json::Value,
    service_opt: &Option<GraphContextService>,
    initialized: &bool,
) -> JsonRpcResponse {
    if !*initialized {
        return not_initialized(request_id);
    }

    let params = request.params.as_ref().cloned().unwrap_or(serde_json::Value::Null);
    let uri = params.get("uri").and_then(|u| u.as_str()).unwrap_or("");

    let service = match service_opt.as_ref() {
        Some(s) => s,
        None => return not_initialized(request_id),
    };

    match resources::read_resource(service, uri) {
        Ok(text) => JsonRpcResponse::success(
            request_id,
            serde_json::json!({
                "contents": [
                    {
                        "uri": uri,
                        "mimeType": "text/markdown",
                        "text": text
                    }
                ]
            }),
        ),
        Err(msg) => JsonRpcResponse::error(request_id, INTERNAL_ERROR, msg),
    }
}

fn handle_prompts_get(request: &JsonRpcRequest, request_id: serde_json::Value) -> JsonRpcResponse {
    let params = request.params.as_ref().cloned().unwrap_or(serde_json::Value::Null);
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::Value::Object(Default::default()));

    match prompts::get_prompt(name, &args) {
        Ok(result) => JsonRpcResponse::success(request_id, result),
        Err(msg) => JsonRpcResponse::error(request_id, METHOD_NOT_FOUND, msg),
    }
}

fn not_initialized(request_id: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse::error(
        request_id,
        SERVER_NOT_INITIALIZED,
        "Server not initialized. Call initialize first.",
    )
}