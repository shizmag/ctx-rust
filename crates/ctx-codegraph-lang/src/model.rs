use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FileId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SymbolId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct OccurrenceId(pub i64);

pub type CallId = OccurrenceId;

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
    Class,
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
            SymbolKind::Class => "Class",
            SymbolKind::Enum => "Enum",
            SymbolKind::Trait => "Trait",
            SymbolKind::Module => "Module",
            SymbolKind::Test => "Test",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Function" => Some(SymbolKind::Function),
            "Method" => Some(SymbolKind::Method),
            "Impl" => Some(SymbolKind::Impl),
            "Struct" => Some(SymbolKind::Struct),
            "Class" => Some(SymbolKind::Class),
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

    #[allow(clippy::should_implement_trait)]
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum OccurrenceKind {
    Call,
    Reference,
    Import,
    Export,
    TypeUse,
    DefinitionUse,
    VariableRead,
    VariableWrite,
    MacroInvocation,
    Unknown,
}

impl OccurrenceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            OccurrenceKind::Call => "Call",
            OccurrenceKind::Reference => "Reference",
            OccurrenceKind::Import => "Import",
            OccurrenceKind::Export => "Export",
            OccurrenceKind::TypeUse => "TypeUse",
            OccurrenceKind::DefinitionUse => "DefinitionUse",
            OccurrenceKind::VariableRead => "VariableRead",
            OccurrenceKind::VariableWrite => "VariableWrite",
            OccurrenceKind::MacroInvocation => "MacroInvocation",
            OccurrenceKind::Unknown => "Unknown",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Call" => Some(OccurrenceKind::Call),
            "Reference" => Some(OccurrenceKind::Reference),
            "Import" => Some(OccurrenceKind::Import),
            "Export" => Some(OccurrenceKind::Export),
            "TypeUse" => Some(OccurrenceKind::TypeUse),
            "DefinitionUse" => Some(OccurrenceKind::DefinitionUse),
            "VariableRead" => Some(OccurrenceKind::VariableRead),
            "VariableWrite" => Some(OccurrenceKind::VariableWrite),
            "MacroInvocation" => Some(OccurrenceKind::MacroInvocation),
            "Unknown" => Some(OccurrenceKind::Unknown),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Occurrence {
    pub id: Option<OccurrenceId>,
    pub file_id: Option<FileId>,
    pub enclosing_symbol: Option<SymbolId>,
    pub enclosing_temp_index: Option<usize>,
    pub kind: OccurrenceKind,
    pub raw_text: String,
    pub file: PathBuf,
    pub range: TextRange,
    pub language: LanguageId,
    pub backend_id: BackendId,
}

pub type CallSite = Occurrence;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EdgeKind {
    Call,
    Reference,
    Import,
    Export,
    TypeUse,
    Inherits,
    Implements,
    DataFlow,
    Contains,
    Unknown,
}

impl EdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeKind::Call => "Call",
            EdgeKind::Reference => "Reference",
            EdgeKind::Import => "Import",
            EdgeKind::Export => "Export",
            EdgeKind::TypeUse => "TypeUse",
            EdgeKind::Inherits => "Inherits",
            EdgeKind::Implements => "Implements",
            EdgeKind::DataFlow => "DataFlow",
            EdgeKind::Contains => "Contains",
            EdgeKind::Unknown => "Unknown",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Call" => Some(EdgeKind::Call),
            "Reference" => Some(EdgeKind::Reference),
            "Import" => Some(EdgeKind::Import),
            "Export" => Some(EdgeKind::Export),
            "TypeUse" => Some(EdgeKind::TypeUse),
            "Inherits" => Some(EdgeKind::Inherits),
            "Implements" => Some(EdgeKind::Implements),
            "DataFlow" => Some(EdgeKind::DataFlow),
            "Contains" => Some(EdgeKind::Contains),
            "Unknown" => Some(EdgeKind::Unknown),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EdgeId(pub i64);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphEdge {
    pub id: Option<EdgeId>,
    pub kind: EdgeKind,
    pub from_file_id: Option<FileId>,
    pub from_symbol_id: Option<SymbolId>,
    pub to_symbol_id: Option<SymbolId>,
    pub to_external: Option<String>,
    pub occurrence_id: Option<OccurrenceId>,
    pub raw_text: Option<String>,
    pub range: Option<TextRange>,
    pub confidence: ResolutionConfidence,
    pub produced_by: Option<ResolverId>,
}

pub type CallEdge = GraphEdge;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CodeIndex {
    pub root: PathBuf,
    pub files: Vec<FileSnapshot>,
    pub symbols: Vec<Symbol>,
    pub occurrences: Vec<Occurrence>,
    pub edges: Vec<GraphEdge>,
    pub call_sites: Vec<Occurrence>,
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
            SymbolKind::Class => LanguageObjectKind::Class,
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

/// Light signature extraction for function/method symbols.
/// Reads the source at the symbol's start range and captures a compact decl line.
pub fn extract_signature(file: &Path, range: &TextRange, kind: SymbolKind) -> Option<String> {
    if !matches!(kind, SymbolKind::Function | SymbolKind::Method) {
        return None;
    }
    let content = std::fs::read_to_string(file).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    let start_idx = range.start_line.saturating_sub(1);
    if start_idx >= lines.len() {
        return None;
    }
    // Capture up to 3 lines or until we hit body opener
    let mut sig_lines = Vec::new();
    for i in start_idx..(start_idx + 3).min(lines.len()) {
        let l = lines[i];
        sig_lines.push(l);
        if l.contains('{') || l.trim_end().ends_with(';') || l.trim_end().ends_with("->") {
            break;
        }
    }
    let joined = sig_lines.join(" ").trim().to_string();
    // Trim to before body if present
    let end = joined.find(" {").or_else(|| joined.find('{')).unwrap_or(joined.len());
    let mut sig = joined[..end].trim().to_string();
    // For methods, keep receiver etc. Normalize pub if present.
    if sig.is_empty() {
        return None;
    }
    // Ensure starts with fn or pub fn
    if !sig.starts_with("fn ") && !sig.contains(" fn ") {
        // try to find fn decl
        if let Some(pos) = sig.find("fn ") {
            sig = sig[pos..].to_string();
        }
    }
    Some(sig)
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SymbolResolution {
    Unique(LanguageObject),
    Ambiguous(Vec<LanguageObject>),
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EdgeDirection {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ResolvedEdgeTarget {
    Symbol(Symbol),
    External(String),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GraphContextMode {
    Callers,
    Callees,
    Dependencies,
    Dependents,
    ForwardSlice,
    ReverseSlice,
    Forward,
    Reverse,
    Neighborhood,
    Impact,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphContextOptions {
    pub mode: GraphContextMode,
    pub max_depth: usize,
    pub max_nodes: usize,
    pub include_root: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GraphContextEdge {
    pub from: SymbolId,
    pub to: SymbolId,
    pub label: Option<String>,
    pub confidence: Option<String>,
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
    pub edges: Vec<GraphContextEdge>,
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

    #[allow(clippy::should_implement_trait)]
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

// Generic Occurrence and GraphEdge model types are defined above.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_kind_as_str_roundtrip() {
        let variants = [
            SymbolKind::Function,
            SymbolKind::Method,
            SymbolKind::Impl,
            SymbolKind::Struct,
            SymbolKind::Class,
            SymbolKind::Enum,
            SymbolKind::Trait,
            SymbolKind::Module,
            SymbolKind::Test,
        ];
        for kind in variants {
            let s = kind.as_str();
            assert_eq!(SymbolKind::from_str(s), Some(kind));
        }
        assert_eq!(SymbolKind::from_str("invalid"), None);
    }

    #[test]
    fn occurrence_kind_as_str_roundtrip() {
        let variants = [
            OccurrenceKind::Call,
            OccurrenceKind::Reference,
            OccurrenceKind::Import,
            OccurrenceKind::Export,
            OccurrenceKind::TypeUse,
            OccurrenceKind::DefinitionUse,
            OccurrenceKind::VariableRead,
            OccurrenceKind::VariableWrite,
            OccurrenceKind::MacroInvocation,
            OccurrenceKind::Unknown,
        ];
        for kind in variants {
            let s = kind.as_str();
            assert_eq!(OccurrenceKind::from_str(s), Some(kind));
        }
        assert_eq!(OccurrenceKind::from_str("bogus"), None);
    }

    #[test]
    fn edge_kind_as_str_roundtrip() {
        let variants = [
            EdgeKind::Call,
            EdgeKind::Reference,
            EdgeKind::Import,
            EdgeKind::Export,
            EdgeKind::TypeUse,
            EdgeKind::Inherits,
            EdgeKind::Implements,
            EdgeKind::DataFlow,
            EdgeKind::Contains,
            EdgeKind::Unknown,
        ];
        for kind in variants {
            let s = kind.as_str();
            assert_eq!(EdgeKind::from_str(s), Some(kind));
        }
        assert_eq!(EdgeKind::from_str("nope"), None);
    }

    #[test]
    fn resolution_confidence_as_str_roundtrip_and_aliases() {
        let canonical = [
            (ResolutionConfidence::Syntax, "Syntax"),
            (ResolutionConfidence::Heuristic, "Heuristic"),
            (ResolutionConfidence::LspExact, "LspExact"),
            (ResolutionConfidence::Unresolved, "Unresolved"),
        ];
        for (kind, expected) in canonical {
            assert_eq!(kind.as_str(), expected);
            assert_eq!(ResolutionConfidence::from_str(expected), Some(kind));
        }

        assert_eq!(
            ResolutionConfidence::from_str("Local"),
            Some(ResolutionConfidence::Syntax)
        );
        assert_eq!(
            ResolutionConfidence::from_str("NameOnly"),
            Some(ResolutionConfidence::Heuristic)
        );
        assert_eq!(
            ResolutionConfidence::from_str("Ambiguous"),
            Some(ResolutionConfidence::Heuristic)
        );
        assert_eq!(
            ResolutionConfidence::from_str("Exact"),
            Some(ResolutionConfidence::LspExact)
        );
        assert_eq!(ResolutionConfidence::from_str("unknown"), None);
    }

    #[test]
    fn resolution_confidence_display() {
        assert_eq!(format!("{}", ResolutionConfidence::Syntax), "Syntax");
        assert_eq!(format!("{}", ResolutionConfidence::LspExact), "LspExact");
    }

    #[test]
    fn file_parse_status_as_str_roundtrip() {
        assert_eq!(FileParseStatus::Success.as_str(), "Success");
        assert_eq!(FileParseStatus::Failed.as_str(), "Failed");
        assert_eq!(
            FileParseStatus::from_str("Success"),
            Some(FileParseStatus::Success)
        );
        assert_eq!(
            FileParseStatus::from_str("Failed"),
            Some(FileParseStatus::Failed)
        );
        assert_eq!(FileParseStatus::from_str("Pending"), None);
    }

    #[test]
    fn language_id_new_lowercase_and_display() {
        let id = LanguageId::new("RuSt");
        assert_eq!(id.as_str(), "rust");
        assert_eq!(id.to_string(), "rust");
        assert_eq!(format!("{id}"), "rust");

        let rust = LanguageId::rust();
        assert_eq!(rust.as_str(), "rust");
    }

    #[test]
    fn language_constructor_and_type_alias() {
        let lang: Language = Language("Python".to_string());
        assert_eq!(lang.as_str(), "Python");
    }

    #[test]
    fn text_range_to_source_range() {
        let text = TextRange {
            start_line: 2,
            start_col: 5,
            end_line: 10,
            end_col: 20,
        };
        let source: SourceRange = text.clone().into();
        assert_eq!(source.start_line, 2);
        assert_eq!(source.start_col, 5);
        assert_eq!(source.end_line, 10);
        assert_eq!(source.end_col, 20);
        assert_eq!(source, SourceRange::from(text));
    }

    #[test]
    fn symbol_kind_to_language_object_kind() {
        let mappings = [
            (SymbolKind::Function, LanguageObjectKind::Function),
            (SymbolKind::Method, LanguageObjectKind::Method),
            (SymbolKind::Struct, LanguageObjectKind::Struct),
            (SymbolKind::Class, LanguageObjectKind::Class),
            (SymbolKind::Enum, LanguageObjectKind::Enum),
            (SymbolKind::Trait, LanguageObjectKind::Trait),
            (SymbolKind::Impl, LanguageObjectKind::Impl),
            (SymbolKind::Module, LanguageObjectKind::Module),
            (SymbolKind::Test, LanguageObjectKind::Function),
        ];
        for (symbol_kind, object_kind) in mappings {
            assert_eq!(LanguageObjectKind::from(symbol_kind), object_kind);
        }
    }

    #[test]
    fn rebuild_reason_as_str_all_variants() {
        let reasons = [
            RebuildReason::MissingDatabase,
            RebuildReason::CorruptDatabase,
            RebuildReason::SchemaVersionChanged,
            RebuildReason::IndexerVersionChanged,
            RebuildReason::BackendSetChanged,
            RebuildReason::BackendVersionChanged,
            RebuildReason::ParserVersionChanged,
            RebuildReason::ParserConfigChanged,
            RebuildReason::ResolverVersionChanged,
            RebuildReason::ResolverConfigChanged,
            RebuildReason::DiscoveryConfigChanged,
            RebuildReason::ChangeDetectionStrategyChanged,
            RebuildReason::PreviousRunIncomplete,
            RebuildReason::PreviousRunFailed,
        ];
        for reason in reasons {
            let s = reason.as_str();
            assert!(!s.is_empty());
            assert_eq!(s.chars().next().unwrap().is_uppercase(), true);
        }
        assert_eq!(RebuildReason::MissingDatabase.as_str(), "MissingDatabase");
        assert_eq!(
            RebuildReason::ChangeDetectionStrategyChanged.as_str(),
            "ChangeDetectionStrategyChanged"
        );
        assert_eq!(
            RebuildReason::PreviousRunFailed.as_str(),
            "PreviousRunFailed"
        );
    }
}
