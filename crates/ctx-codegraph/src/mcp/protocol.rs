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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_file_uri_unix_absolute_path() {
        let path = parse_file_uri("file:///tmp/workspace").unwrap();
        assert_eq!(path, PathBuf::from("/tmp/workspace"));
    }

    #[test]
    fn parse_file_uri_windows_style_host() {
        let path = parse_file_uri("file://localhost/C:/Users/dev/project").unwrap();
        assert_eq!(path, PathBuf::from("/C:/Users/dev/project"));
    }

    #[test]
    fn parse_file_uri_percent_encoding() {
        let path = parse_file_uri("file:///tmp/my%20project/src%2Flib.rs").unwrap();
        assert_eq!(path, PathBuf::from("/tmp/my project/src/lib.rs"));
    }

    #[test]
    fn parse_file_uri_invalid_scheme_returns_none() {
        assert!(parse_file_uri("http://example.com").is_none());
        assert!(parse_file_uri("").is_none());
        assert!(parse_file_uri("/absolute/without/scheme").is_none());
    }

    #[test]
    fn parse_file_uri_malformed_percent_encoding_keeps_literal() {
        let path = parse_file_uri("file:///tmp/bad%ZZname").unwrap();
        assert_eq!(path, PathBuf::from("/tmp/bad%ZZname"));
    }

    #[test]
    fn get_workspace_path_prefers_workspace_folders() {
        let params = json!({
            "workspaceFolders": [{ "uri": "file:///ws/from/folders" }],
            "rootUri": "file:///ws/from/root_uri",
            "rootPath": "/ws/from/root_path"
        });
        assert_eq!(
            get_workspace_path(&params),
            PathBuf::from("/ws/from/folders")
        );
    }

    #[test]
    fn get_workspace_path_falls_back_to_root_uri() {
        let params = json!({
            "rootUri": "file:///ws/from/root_uri",
            "rootPath": "/ws/from/root_path"
        });
        assert_eq!(
            get_workspace_path(&params),
            PathBuf::from("/ws/from/root_uri")
        );
    }

    #[test]
    fn get_workspace_path_falls_back_to_root_path() {
        let params = json!({ "rootPath": "/ws/from/root_path" });
        assert_eq!(
            get_workspace_path(&params),
            PathBuf::from("/ws/from/root_path")
        );
    }

    #[test]
    fn get_workspace_path_uses_current_dir_when_params_missing() {
        let params = json!({});
        let expected = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        assert_eq!(get_workspace_path(&params), expected);
    }

    #[test]
    fn json_rpc_response_success_serializes_result_only() {
        let resp = JsonRpcResponse::success(json!(42), json!({ "ok": true }));
        let value = serde_json::to_value(&resp).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["id"], 42);
        assert_eq!(value["result"]["ok"], true);
        assert!(value.get("error").is_none());
    }

    #[test]
    fn json_rpc_response_error_serializes_error_only() {
        let resp = JsonRpcResponse::error(json!("req-1"), METHOD_NOT_FOUND, "not found");
        let value = serde_json::to_value(&resp).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["id"], "req-1");
        assert_eq!(value["error"]["code"], METHOD_NOT_FOUND);
        assert_eq!(value["error"]["message"], "not found");
        assert!(value.get("result").is_none());
    }

    #[test]
    fn text_tool_result_sets_is_error_flag() {
        let ok = text_tool_result("all good", false);
        assert_eq!(ok["isError"], false);
        assert_eq!(ok["content"][0]["type"], "text");
        assert_eq!(ok["content"][0]["text"], "all good");

        let err = text_tool_result("something failed", true);
        assert_eq!(err["isError"], true);
        assert_eq!(err["content"][0]["text"], "something failed");
    }

    #[test]
    fn depth_or_auto_parses_variants() {
        assert!(matches!(
            depth_or_auto(&json!({})).unwrap(),
            crate::DepthLimit::Auto
        ));
        assert!(matches!(
            depth_or_auto(&json!({ "depth": "auto" })).unwrap(),
            crate::DepthLimit::Auto
        ));
        assert!(matches!(
            depth_or_auto(&json!({ "depth": 3 })).unwrap(),
            crate::DepthLimit::Fixed(3)
        ));
        assert!(depth_or_auto(&json!({ "depth": "nope" })).is_err());
    }

    #[test]
    fn parse_graph_context_mode_maps_aliases() {
        use crate::model::GraphContextMode;
        assert_eq!(
            parse_graph_context_mode("callers"),
            GraphContextMode::Callers
        );
        assert_eq!(
            parse_graph_context_mode("forward_slice"),
            GraphContextMode::ForwardSlice
        );
        assert_eq!(
            parse_graph_context_mode("unknown-mode"),
            GraphContextMode::Neighborhood
        );
    }
}

