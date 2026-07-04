use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FileId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SymbolId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CallId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct LanguageId(pub String);

impl LanguageId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().to_ascii_lowercase())
    }

    pub fn rust() -> Self {
        Self("rust".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for LanguageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub type Language = LanguageId;

#[allow(non_snake_case)]
pub fn Language(s: String) -> LanguageId {
    LanguageId(s)
}

impl rusqlite::types::ToSql for LanguageId {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        self.0.to_sql()
    }
}

impl rusqlite::types::FromSql for LanguageId {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        Ok(LanguageId(s))
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ResolutionConfidence {
    Syntax,
    Heuristic,
    LspExact,
    Unresolved,
}

impl ResolutionConfidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResolutionConfidence::Syntax => "Syntax",
            ResolutionConfidence::Heuristic => "Heuristic",
            ResolutionConfidence::LspExact => "LspExact",
            ResolutionConfidence::Unresolved => "Unresolved",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Syntax" | "Local" => Some(ResolutionConfidence::Syntax),
            "Heuristic" | "NameOnly" | "Ambiguous" => Some(ResolutionConfidence::Heuristic),
            "LspExact" | "Exact" => Some(ResolutionConfidence::LspExact),
            "Unresolved" => Some(ResolutionConfidence::Unresolved),
            _ => None,
        }
    }
}

impl std::fmt::Display for ResolutionConfidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
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
    pub files: Vec<FileSnapshot>,
    pub symbols: Vec<Symbol>,
    pub call_sites: Vec<CallSite>,
    pub edges: Vec<CallEdge>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SourceRange {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl From<TextRange> for SourceRange {
    fn from(r: TextRange) -> Self {
        Self {
            start_line: r.start_line,
            start_col: r.start_col,
            end_line: r.end_line,
            end_col: r.end_col,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum LanguageObjectKind {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Impl,
    Module,
    Class,
    Interface,
    TypeAlias,
    Constant,
    Variable,
    Unknown,
}

impl From<SymbolKind> for LanguageObjectKind {
    fn from(kind: SymbolKind) -> Self {
        match kind {
            SymbolKind::Function => LanguageObjectKind::Function,
            SymbolKind::Method => LanguageObjectKind::Method,
            SymbolKind::Struct => LanguageObjectKind::Struct,
            SymbolKind::Enum => LanguageObjectKind::Enum,
            SymbolKind::Trait => LanguageObjectKind::Trait,
            SymbolKind::Impl => LanguageObjectKind::Impl,
            SymbolKind::Module => LanguageObjectKind::Module,
            SymbolKind::Test => LanguageObjectKind::Function,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LanguageObject {
    pub id: SymbolId,
    pub name: String,
    pub qualified_name: String,
    pub kind: LanguageObjectKind,
    pub file_path: PathBuf,
    pub range: SourceRange,
    pub signature: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SymbolResolution {
    Unique(LanguageObject),
    Ambiguous(Vec<LanguageObject>),
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GraphContextMode {
    Callers,
    Callees,
    Dependencies,
    Dependents,
    ForwardSlice,
    ReverseSlice,
    Neighborhood,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphContextOptions {
    pub mode: GraphContextMode,
    pub max_depth: usize,
    pub max_nodes: usize,
    pub include_root: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GraphEdge {
    pub from: SymbolId,
    pub to: SymbolId,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContextFileSpan {
    pub file_path: PathBuf,
    pub range: SourceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GraphContextDiagnostic {
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphContextResult {
    pub root: LanguageObject,
    pub nodes: Vec<LanguageObject>,
    pub edges: Vec<GraphEdge>,
    pub files: Vec<ContextFileSpan>,
    pub diagnostics: Vec<GraphContextDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RebuildReason {
    MissingDatabase,
    CorruptDatabase,
    SchemaVersionChanged,
    IndexerVersionChanged,
    BackendSetChanged,
    BackendVersionChanged,
    ParserVersionChanged,
    ParserConfigChanged,
    ResolverVersionChanged,
    ResolverConfigChanged,
    DiscoveryConfigChanged,
    ChangeDetectionStrategyChanged,
    PreviousRunIncomplete,
    PreviousRunFailed,
}

impl RebuildReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            RebuildReason::MissingDatabase => "MissingDatabase",
            RebuildReason::CorruptDatabase => "CorruptDatabase",
            RebuildReason::SchemaVersionChanged => "SchemaVersionChanged",
            RebuildReason::IndexerVersionChanged => "IndexerVersionChanged",
            RebuildReason::BackendSetChanged => "BackendSetChanged",
            RebuildReason::BackendVersionChanged => "BackendVersionChanged",
            RebuildReason::ParserVersionChanged => "ParserVersionChanged",
            RebuildReason::ParserConfigChanged => "ParserConfigChanged",
            RebuildReason::ResolverVersionChanged => "ResolverVersionChanged",
            RebuildReason::ResolverConfigChanged => "ResolverConfigChanged",
            RebuildReason::DiscoveryConfigChanged => "DiscoveryConfigChanged",
            RebuildReason::ChangeDetectionStrategyChanged => "ChangeDetectionStrategyChanged",
            RebuildReason::PreviousRunIncomplete => "PreviousRunIncomplete",
            RebuildReason::PreviousRunFailed => "PreviousRunFailed",
        }
    }
}

pub type BackendId = String;
pub type ParserId = String;
pub type ResolverId = String;
pub type OccurrenceId = CallId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FileParseStatus {
    Success,
    Failed,
}

impl FileParseStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileParseStatus::Success => "Success",
            FileParseStatus::Failed => "Failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Success" => Some(FileParseStatus::Success),
            "Failed" => Some(FileParseStatus::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileSnapshot {
    pub file_id: Option<FileId>,
    pub rel_path: PathBuf,
    pub abs_path: PathBuf,

    pub language: LanguageId,
    pub backend_id: BackendId,

    pub size_bytes: u64,
    pub mtime_ms: i64,
    pub mtime_ns: Option<i128>,
    pub content_hash: Option<String>,

    pub parser_id: ParserId,
    pub parser_version: String,
    pub parser_config_hash: String,

    pub indexed_at_ms: Option<i64>,
    pub parse_status: FileParseStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FileChangeDetection {
    MtimeAndSize,
    ContentHash,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IndexDiff {
    pub added: Vec<FileSnapshot>,
    pub modified: Vec<FileSnapshot>,
    pub deleted: Vec<PathBuf>,
    pub unchanged: Vec<FileSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IndexState {
    Missing,
    Ready,
    NeedsIncrementalUpdate(IndexDiff),
    NeedsFullRebuild(RebuildReason),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuildReport {
    pub full_rebuild: bool,
    pub full_rebuild_reason: Option<RebuildReason>,
    pub added_files: usize,
    pub modified_files: usize,
    pub deleted_files: usize,
    pub unchanged_files: usize,
    pub parsed_files: usize,
    pub reused_files: usize,
    pub symbols_written: usize,
    pub call_sites_written: usize,
    pub edges_written: usize,
    pub lsp_edges_exact: usize,
    pub syntax_edges: usize,
    pub heuristic_edges: usize,
    pub unresolved_edges: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EdgeKind {
    Call,
    Reference,
    Import,
    Export,
    TypeUse,
    DataFlow,
}

impl EdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeKind::Call => "Call",
            EdgeKind::Reference => "Reference",
            EdgeKind::Import => "Import",
            EdgeKind::Export => "Export",
            EdgeKind::TypeUse => "TypeUse",
            EdgeKind::DataFlow => "DataFlow",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Call" => Some(EdgeKind::Call),
            "Reference" => Some(EdgeKind::Reference),
            "Import" => Some(EdgeKind::Import),
            "Export" => Some(EdgeKind::Export),
            "TypeUse" => Some(EdgeKind::TypeUse),
            "DataFlow" => Some(EdgeKind::DataFlow),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EdgeId(pub i64);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphEdgeRecord {
    pub id: Option<EdgeId>,
    pub kind: EdgeKind,
    pub from_file_id: FileId,
    pub from_symbol_id: Option<SymbolId>,
    pub to_symbol_id: Option<SymbolId>,
    pub to_external: Option<String>,
    pub occurrence_id: Option<OccurrenceId>,
    pub range: TextRange,
    pub label: Option<String>,
    pub confidence: ResolutionConfidence,
    pub produced_by: ResolverId,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AffectedSet {
    pub files: std::collections::HashSet<FileId>,
    pub symbols: std::collections::HashSet<SymbolId>,
    pub occurrences: std::collections::HashSet<OccurrenceId>,
    pub edge_kinds: std::collections::HashSet<EdgeKind>,
    pub resolvers: std::collections::HashSet<ResolverId>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UpdatePlan {
    Ready,
    PartialRebuild(IndexDiff),
    EdgeOnlyRebuild(AffectedSet),
    FullRebuild(RebuildReason),
}
