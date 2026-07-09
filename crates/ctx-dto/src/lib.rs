// ctx-dto: Data Transfer Objects for structured outputs (esp. yaml/json for AI agents).
// Coordinated with settings default_format (e.g. yaml) and MCP render/tools.
// Minimal implementations to support current integration (stubs for render.rs mappings).
// Full DTOs can be expanded without changing callers.

use serde::{Deserialize, Serialize};

pub fn serialize_json<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|e| e.to_string())
}

pub fn serialize_yaml<T: Serialize>(value: &T) -> Result<String, String> {
    serde_yaml::to_string(value).map_err(|e| e.to_string())
}

/// Helper to produce serde_json::Value directly from DTOs (for structuredContent in MCP).
pub fn to_value<T: Serialize>(value: &T) -> Result<serde_json::Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SymbolDto {
    pub kind: String,
    pub name: String,
    pub qualified_name: String,
    pub signature: Option<String>,
    pub path: String,
    pub lines: String,
}

impl SymbolDto {
    pub fn new(
        kind: String,
        name: String,
        qualified_name: String,
        signature: Option<String>,
        path: String,
        lines: String,
    ) -> Self {
        Self {
            kind,
            name,
            qualified_name,
            signature,
            path,
            lines,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AmbiguousResultDto {
    pub query: String,
    pub candidates: Vec<SymbolDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EdgeDto {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiagnosticDto {
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphContextOutputDto {
    pub root: SymbolDto,
    pub nodes: Vec<SymbolDto>,
    pub edges: Vec<EdgeDto>,
    pub mode: String,
    pub depth: usize,
    pub max_nodes: usize,
    pub max_files: usize,
    pub diagnostics: Vec<DiagnosticDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AffectedContextOutputDto {
    pub query: String,
    pub mode: String,
    pub token_budget: usize,
    pub estimated_tokens: usize,
    pub roots: Vec<SymbolDto>,
    pub nodes: Vec<SymbolDto>,
    pub diagnostics: Vec<DiagnosticDto>,
}