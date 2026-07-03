use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FileId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SymbolId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CallId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Language {
    Rust,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Impl,
    Struct,
    Enum,
    Trait,
    Module,
    Test,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Function => "Function",
            SymbolKind::Method => "Method",
            SymbolKind::Impl => "Impl",
            SymbolKind::Struct => "Struct",
            SymbolKind::Enum => "Enum",
            SymbolKind::Trait => "Trait",
            SymbolKind::Module => "Module",
            SymbolKind::Test => "Test",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Function" => Some(SymbolKind::Function),
            "Method" => Some(SymbolKind::Method),
            "Impl" => Some(SymbolKind::Impl),
            "Struct" => Some(SymbolKind::Struct),
            "Enum" => Some(SymbolKind::Enum),
            "Trait" => Some(SymbolKind::Trait),
            "Module" => Some(SymbolKind::Module),
            "Test" => Some(SymbolKind::Test),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ResolutionConfidence {
    Exact,
    Local,
    NameOnly,
    Ambiguous,
    Unresolved,
}

impl ResolutionConfidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResolutionConfidence::Exact => "Exact",
            ResolutionConfidence::Local => "Local",
            ResolutionConfidence::NameOnly => "NameOnly",
            ResolutionConfidence::Ambiguous => "Ambiguous",
            ResolutionConfidence::Unresolved => "Unresolved",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Exact" => Some(ResolutionConfidence::Exact),
            "Local" => Some(ResolutionConfidence::Local),
            "NameOnly" => Some(ResolutionConfidence::NameOnly),
            "Ambiguous" => Some(ResolutionConfidence::Ambiguous),
            "Unresolved" => Some(ResolutionConfidence::Unresolved),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TextRange {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SourceFile {
    pub id: Option<FileId>,
    pub path: PathBuf,
    pub language: Language,
    pub mtime_ms: Option<i64>,
    pub size_bytes: Option<i64>,
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Symbol {
    pub id: Option<SymbolId>,
    pub file_id: Option<FileId>,
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub language: Language,
    pub file: PathBuf,
    pub range: TextRange,
    pub body_range: Option<TextRange>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CallSite {
    pub id: Option<CallId>,
    pub file_id: Option<FileId>,
    pub from: Option<SymbolId>,
    pub from_temp_index: Option<usize>,
    pub raw_name: String,
    pub file: PathBuf,
    pub range: TextRange,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CallEdge {
    pub from: SymbolId,
    pub to: Option<SymbolId>,
    pub call_site_id: Option<CallId>,
    pub raw_name: String,
    pub call_range: TextRange,
    pub confidence: ResolutionConfidence,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CodeIndex {
    pub root: PathBuf,
    pub files: Vec<SourceFile>,
    pub symbols: Vec<Symbol>,
    pub call_sites: Vec<CallSite>,
    pub edges: Vec<CallEdge>,
}
