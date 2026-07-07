use crate::model::{GraphContextMode, GraphContextOptions, GraphContextResult, LanguageObject, SymbolResolution};
use crate::service::GraphContextService;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Debug, serde::Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, serde::Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

fn kind_to_str(kind: crate::model::LanguageObjectKind) -> &'static str {
    match kind {
        crate::model::LanguageObjectKind::Function => "fn",
        crate::model::LanguageObjectKind::Method => "method",
        crate::model::LanguageObjectKind::Struct => "struct",
        crate::model::LanguageObjectKind::Enum => "enum",
        crate::model::LanguageObjectKind::Trait => "trait",
        crate::model::LanguageObjectKind::Impl => "impl",
        crate::model::LanguageObjectKind::Module => "mod",
        crate::model::LanguageObjectKind::Class => "class",
        crate::model::LanguageObjectKind::Interface => "interface",
        crate::model::LanguageObjectKind::TypeAlias => "type",
        crate::model::LanguageObjectKind::Constant => "const",
        crate::model::LanguageObjectKind::Variable => "var",
        crate::model::LanguageObjectKind::Unknown => "unknown",
    }
}

pub fn run_mcp_server() -> Result<(), Box<dyn std::error::Error>> {
    run_mcp_server_with_io(io::stdin().lock(), io::stdout())
}

pub fn run_mcp_server_with_io<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut line = String::new();

    let mut service_opt: Option<GraphContextService> = None;

    eprintln!("MCP Server started. Waiting for messages...");

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break; // EOF
        }

        let line_trimmed = line.trim();
        if line_trimmed.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(line_trimmed) {
            Ok(req) => req,
            Err(e) => {
                let err_resp = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: serde_json::Value::Null,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                    }),
                };
                let resp_str = serde_json::to_string(&err_resp)?;
                writeln!(writer, "{}", resp_str)?;
                writer.flush()?;
                continue;
            }
        };

        let request_id = match &request.id {
            Some(id) => id.clone(),
            None => serde_json::Value::Null,
        };

        // Handle request/notification
        let response = match request.method.as_str() {
            "initialize" => {
                let params = request.params.unwrap_or(serde_json::Value::Null);
                let ws_path = get_workspace_path(&params);
                eprintln!("Initializing MCP Server for workspace: {}", ws_path.display());
                match GraphContextService::load_or_build(&ws_path) {
                    Ok(service) => {
                        service_opt = Some(service);
                        JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: request_id,
                            result: Some(serde_json::json!({
                                "protocolVersion": "2024-11-05",
                                "capabilities": {
                                    "tools": {}
                                },
                                "serverInfo": {
                                    "name": "ctx-codegraph-mcp",
                                    "version": "0.1.0"
                                }
                            })),
                            error: None,
                        }
                    }
                    Err(e) => JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: request_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32603,
                            message: format!("Failed to build/load graph context: {}", e),
                        }),
                    },
                }
            }
            "notifications/initialized" => {
                // This is a notification, do not respond
                continue;
            }
            "tools/list" => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request_id,
                result: Some(serde_json::json!({
                    "tools": [
                        {
                            "name": "get_graph_context",
                            "description": "Expose symbol relationships and source code context (neighborhood, callers, callees, etc.) around a query symbol.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "query": {
                                        "type": "string",
                                        "description": "The symbol name or qualified path to resolve."
                                    },
                                    "mode": {
                                        "type": "string",
                                        "enum": ["neighborhood", "callers", "callees", "dependencies", "dependents", "impact"],
                                        "description": "The traversal mode. Default is neighborhood."
                                    },
                                    "depth": {
                                        "type": "integer",
                                        "description": "BFS traversal depth."
                                    }
                                },
                                "required": ["query"]
                            }
                        }
                    ]
                })),
                error: None,
            },
            "tools/call" => {
                let params = request.params.unwrap_or(serde_json::Value::Null);
                let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                if tool_name == "get_graph_context" {
                    if let Some(service) = &service_opt {
                        let args = params.get("arguments").unwrap_or(&serde_json::Value::Null);
                        let query = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
                        let mode_str = args.get("mode").and_then(|m| m.as_str()).unwrap_or("neighborhood");
                        let depth = args.get("depth").and_then(|d| d.as_u64()).map(|d| d as usize).unwrap_or(2);

                        let mode = match mode_str {
                            "callers" => GraphContextMode::Callers,
                            "callees" => GraphContextMode::Callees,
                            "dependencies" => GraphContextMode::Dependencies,
                            "dependents" => GraphContextMode::Dependents,
                            "impact" => GraphContextMode::Impact,
                            _ => GraphContextMode::Neighborhood,
                        };

                        match service.resolve_symbol(query) {
                            Ok(res) => match res {
                                SymbolResolution::Unique(obj) => {
                                    let options = GraphContextOptions {
                                        mode,
                                        max_depth: depth,
                                        max_nodes: 40,
                                        include_root: true,
                                    };
                                    match service.build_context_for_symbol(obj.id, options) {
                                        Ok(result) => {
                                            let markdown = render_context_to_markdown(&result, service.repo_root(), mode, depth, 40);
                                            JsonRpcResponse {
                                                jsonrpc: "2.0".to_string(),
                                                id: request_id,
                                                result: Some(serde_json::json!({
                                                    "content": [
                                                        {
                                                            "type": "text",
                                                            "text": markdown
                                                        }
                                                    ],
                                                    "isError": false
                                                })),
                                                error: None,
                                            }
                                        }
                                        Err(e) => JsonRpcResponse {
                                            jsonrpc: "2.0".to_string(),
                                            id: request_id,
                                            result: None,
                                            error: Some(JsonRpcError {
                                                code: -32603,
                                                message: format!("Failed to build context: {}", e),
                                            }),
                                        },
                                    }
                                }
                                SymbolResolution::Ambiguous(candidates) => {
                                    let mut msg = format!("Multiple symbols found matching query: '{}'. Please be more specific:\n", query);
                                    for c in candidates {
                                        let kind_str = kind_to_str(c.kind);
                                        msg.push_str(&format!("- {} {} in {}\n", kind_str, c.qualified_name, c.file_path.display()));
                                    }
                                    JsonRpcResponse {
                                        jsonrpc: "2.0".to_string(),
                                        id: request_id,
                                        result: Some(serde_json::json!({
                                            "content": [
                                                {
                                                    "type": "text",
                                                    "text": msg
                                                }
                                            ],
                                            "isError": false
                                        })),
                                        error: None,
                                    }
                                }
                                SymbolResolution::NotFound => {
                                    JsonRpcResponse {
                                        jsonrpc: "2.0".to_string(),
                                        id: request_id,
                                        result: Some(serde_json::json!({
                                            "content": [
                                                {
                                                    "type": "text",
                                                    "text": format!("Error: Symbol not found for query '{}'", query)
                                                }
                                            ],
                                            "isError": true
                                        })),
                                        error: None,
                                    }
                                }
                            },
                            Err(e) => JsonRpcResponse {
                                jsonrpc: "2.0".to_string(),
                                id: request_id,
                                result: None,
                                error: Some(JsonRpcError {
                                    code: -32603,
                                    message: format!("Failed to resolve symbol: {}", e),
                                }),
                            },
                        }
                    } else {
                        JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: request_id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32000,
                                message: "Server not initialized. Call initialize first.".to_string(),
                            }),
                        }
                    }
                } else {
                    JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: request_id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32601,
                            message: format!("Unknown tool: {}", tool_name),
                        }),
                    }
                }
            }
            _ => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", request.method),
                }),
            },
        };

        let response_str = serde_json::to_string(&response)?;
        writeln!(writer, "{}", response_str)?;
        writer.flush()?;
    }

    Ok(())
}

fn get_workspace_path(params: &serde_json::Value) -> PathBuf {
    if let Some(folders) = params.get("workspaceFolders").and_then(|f| f.as_array()) {
        if let Some(first) = folders.first() {
            if let Some(uri) = first.get("uri").and_then(|u| u.as_str()) {
                if let Some(path) = parse_file_uri(uri) {
                    return path;
                }
            }
        }
    }
    if let Some(root_uri) = params.get("rootUri").and_then(|u| u.as_str()) {
        if let Some(path) = parse_file_uri(root_uri) {
            return path;
        }
    }
    if let Some(root_path) = params.get("rootPath").and_then(|p| p.as_str()) {
        return PathBuf::from(root_path);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn parse_file_uri(uri: &str) -> Option<PathBuf> {
    if let Some(rest) = uri.strip_prefix("file://") {
        let path_str = if rest.starts_with('/') {
            rest
        } else {
            if let Some(slash_idx) = rest.find('/') {
                &rest[slash_idx..]
            } else {
                rest
            }
        };
        let decoded = url_decode(path_str);
        return Some(PathBuf::from(decoded));
    }
    None
}

fn url_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let mut hex = String::new();
            if let Some(h1) = chars.next() { hex.push(h1); }
            if let Some(h2) = chars.next() { hex.push(h2); }
            if let Ok(val) = u8::from_str_radix(&hex, 16) {
                result.push(val as char);
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn render_context_to_markdown(
    result: &GraphContextResult,
    root_path: &Path,
    mode: GraphContextMode,
    depth: usize,
    max_nodes: usize,
) -> String {
    let mut out = String::new();

    // Header
    out.push_str("# Graph Context\n\n");
    let root_kind = kind_to_str(result.root.kind);
    let root_rel_path = result
        .root
        .file_path
        .strip_prefix(root_path)
        .unwrap_or(&result.root.file_path);
    out.push_str(&format!("Root: {} {}\n", root_kind, result.root.name));
    out.push_str(&format!(
        "Path: {}:{}-{}\n",
        root_rel_path.display(),
        result.root.range.start_line,
        result.root.range.end_line
    ));
    out.push_str(&format!("Mode: {:?}\n", mode));
    out.push_str(&format!("Depth: {}\n", depth));
    out.push_str(&format!("Max nodes: {}\n\n", max_nodes));

    // Graph
    out.push_str("## Graph\n\n");
    let mut symbol_names = std::collections::HashMap::new();
    symbol_names.insert(result.root.id, result.root.qualified_name.clone());
    for node in &result.nodes {
        symbol_names.insert(node.id, node.qualified_name.clone());
    }

    let mut edge_lines = Vec::new();
    for edge in &result.edges {
        let from_name = symbol_names
            .get(&edge.from)
            .cloned()
            .unwrap_or_else(|| format!("unknown_{:?}", edge.from));
        let to_name = symbol_names
            .get(&edge.to)
            .cloned()
            .unwrap_or_else(|| format!("unknown_{:?}", edge.to));
        edge_lines.push(format!("{} -> {}", from_name, to_name));
    }
    edge_lines.sort();
    for line in edge_lines {
        out.push_str(&line);
        out.push('\n');
    }
    out.push('\n');

    // Included Symbols
    out.push_str("## Included Symbols\n\n");
    let mut symbols_list = Vec::new();

    let format_symbol = |obj: &LanguageObject| -> String {
        let kind = kind_to_str(obj.kind);
        let rel_path = obj
            .file_path
            .strip_prefix(root_path)
            .unwrap_or(&obj.file_path);
        format!(
            "- {} {} — {}:{}-{}",
            kind,
            obj.name,
            rel_path.display(),
            obj.range.start_line,
            obj.range.end_line
        )
    };

    symbols_list.push(format_symbol(&result.root));
    for node in &result.nodes {
        symbols_list.push(format_symbol(node));
    }
    symbols_list.sort();

    for sym_line in symbols_list {
        out.push_str(&sym_line);
        out.push('\n');
    }
    out.push('\n');

    // Files
    out.push_str("## Files\n\n");

    let mut sorted_files = result.files.clone();
    sorted_files.sort_by(|a, b| match a.file_path.cmp(&b.file_path) {
        std::cmp::Ordering::Equal => a.range.start_line.cmp(&b.range.start_line),
        other => other,
    });

    for file_span in sorted_files {
        let rel_path = file_span
            .file_path
            .strip_prefix(root_path)
            .unwrap_or(&file_span.file_path);
        out.push_str(&format!(
            "### {}:{}-{}\n\n",
            rel_path.display(),
            file_span.range.start_line,
            file_span.range.end_line
        ));

        let content = match std::fs::read_to_string(&file_span.file_path) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                if file_span.range.start_line == 0 || file_span.range.start_line > lines.len() {
                    "".to_string()
                } else {
                    let end = std::cmp::min(file_span.range.end_line, lines.len());
                    if file_span.range.start_line > end {
                        "".to_string()
                    } else {
                        let mut result = String::new();
                        for i in (file_span.range.start_line - 1)..end {
                            result.push_str(lines[i]);
                            result.push('\n');
                        }
                        result
                    }
                }
            }
            Err(e) => format!("Error reading file: {}\n", e),
        };

        let lang = match file_span.file_path.extension().and_then(|ext| ext.to_str()) {
            Some("rs") => "rust",
            Some("py") => "python",
            Some("js") => "javascript",
            Some("ts") => "typescript",
            Some("tsx") => "tsx",
            Some("jsx") => "jsx",
            _ => "",
        };
        out.push_str(&format!("```{}\n", lang));
        out.push_str(&content);
        if !content.ends_with('\n') && !content.is_empty() {
            out.push('\n');
        }
        out.push_str("```\n\n");
    }

    out
}
