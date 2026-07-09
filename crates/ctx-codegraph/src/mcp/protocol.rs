use std::path::PathBuf;

pub const PARSE_ERROR: i32 = -32700;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INTERNAL_ERROR: i32 = -32603;
pub const SERVER_NOT_INITIALIZED: i32 = -32000;

#[derive(Debug, serde::Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, serde::Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, serde::Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: serde_json::Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

pub fn text_tool_result(text: impl Into<String>, is_error: bool) -> serde_json::Value {
    serde_json::json!({
        "content": [
            {
                "type": "text",
                "text": text.into()
            }
        ],
        "isError": is_error
    })
}

pub fn get_workspace_path(params: &serde_json::Value) -> PathBuf {
    if let Some(folders) = params.get("workspaceFolders").and_then(|f| f.as_array())
        && let Some(first) = folders.first()
        && let Some(uri) = first.get("uri").and_then(|u| u.as_str())
        && let Some(path) = parse_file_uri(uri)
    {
        return path;
    }
    if let Some(root_uri) = params.get("rootUri").and_then(|u| u.as_str())
        && let Some(path) = parse_file_uri(root_uri)
    {
        return path;
    }
    if let Some(root_path) = params.get("rootPath").and_then(|p| p.as_str()) {
        return PathBuf::from(root_path);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn parse_file_uri(uri: &str) -> Option<PathBuf> {
    if let Some(rest) = uri.strip_prefix("file://") {
        let path_str = if rest.starts_with('/') {
            rest
        } else if let Some(slash_idx) = rest.find('/') {
            &rest[slash_idx..]
        } else {
            rest
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
            if let Some(h1) = chars.next() {
                hex.push(h1);
            }
            if let Some(h2) = chars.next() {
                hex.push(h2);
            }
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

pub fn get_str_arg<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

pub fn get_usize_arg(args: &serde_json::Value, key: &str) -> Option<usize> {
    args.get(key).and_then(|v| {
        v.as_u64()
            .map(|n| n as usize)
            .or_else(|| v.as_i64().map(|n| n as usize))
    })
}

pub fn get_bool_arg(args: &serde_json::Value, key: &str, default: bool) -> bool {
    args.get(key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

pub fn get_string_array(args: &serde_json::Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn depth_or_auto(args: &serde_json::Value) -> Result<crate::DepthLimit, String> {
    match args.get("depth") {
        None => Ok(crate::DepthLimit::Auto),
        Some(v) if v.as_str() == Some("auto") => Ok(crate::DepthLimit::Auto),
        Some(v) => v
            .as_u64()
            .map(|d| crate::DepthLimit::Fixed(d as usize))
            .or_else(|| v.as_i64().map(|d| crate::DepthLimit::Fixed(d as usize)))
            .ok_or_else(|| "depth must be a non-negative integer or \"auto\"".to_string()),
    }
}

pub fn parse_graph_context_mode(mode_str: &str) -> crate::model::GraphContextMode {
    match mode_str {
        "callers" => crate::model::GraphContextMode::Callers,
        "callees" => crate::model::GraphContextMode::Callees,
        "dependencies" => crate::model::GraphContextMode::Dependencies,
        "dependents" => crate::model::GraphContextMode::Dependents,
        "forward-slice" | "forward_slice" => crate::model::GraphContextMode::ForwardSlice,
        "reverse-slice" | "reverse_slice" => crate::model::GraphContextMode::ReverseSlice,
        "forward" => crate::model::GraphContextMode::Forward,
        "reverse" => crate::model::GraphContextMode::Reverse,
        "impact" => crate::model::GraphContextMode::Impact,
        _ => crate::model::GraphContextMode::Neighborhood,
    }
}

pub fn parse_edge_kinds(values: &[String]) -> Vec<crate::model::EdgeKind> {
    values
        .iter()
        .filter_map(|k| {
            crate::model::EdgeKind::from_str(k).or_else(|| {
                let capitalized = if k.is_empty() {
                    k.clone()
                } else {
                    let mut chars = k.chars();
                    match chars.next() {
                        None => k.clone(),
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                };
                crate::model::EdgeKind::from_str(&capitalized)
            })
        })
        .collect()
}

