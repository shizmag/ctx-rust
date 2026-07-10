use crate::error::CodeGraphError;
use crate::BuildIndexOptions;
use crate::model::{Language, Occurrence, ResolutionConfidence, Symbol};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct BackendId(pub String);

impl BackendId {
    pub fn new(s: impl Into<String>) -> Self {
        BackendId(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ParserId(pub String);

impl ParserId {
    pub fn new(s: impl Into<String>) -> Self {
        ParserId(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ResolverId(pub String);

impl ResolverId {
    pub fn new(s: impl Into<String>) -> Self {
        ResolverId(s.into())
    }
}

macro_rules! impl_string_id_sql {
    ($id:ty) => {
        impl rusqlite::types::ToSql for $id {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                self.0.to_sql()
            }
        }

        impl rusqlite::types::FromSql for $id {
            fn column_result(
                value: rusqlite::types::ValueRef<'_>,
            ) -> rusqlite::types::FromSqlResult<Self> {
                let s = String::column_result(value)?;
                Ok(Self(s))
            }
        }
    };
}

impl_string_id_sql!(BackendId);
impl_string_id_sql!(ParserId);
impl_string_id_sql!(ResolverId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceMarker {
    File(&'static str),
    Directory(&'static str),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackendMetadata {
    pub backend_id: String,
    pub language: String,
    pub parser_id: String,
    pub parser_version: String,
    pub resolver_id: Option<String>,
    pub resolver_version: Option<String>,
    pub config_hash: String,
}

pub struct ParseInput<'a> {
    pub path: &'a Path,
}

#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub symbols: Vec<Symbol>,
    pub occurrences: Vec<Occurrence>,
}

pub struct ResolveInput<'a> {
    pub workspace_root: &'a Path,
    pub occurrence: &'a Occurrence,
    pub symbols: &'a [Symbol],
}

pub struct ResolveOutput {
    pub resolved_symbol_index: Option<usize>,
    pub confidence: ResolutionConfidence,
}

pub trait ParserBackend: Send + Sync {
    fn parser_id(&self) -> ParserId;
    fn parser_version(&self) -> String;
    fn parse_file(&self, input: ParseInput<'_>) -> Result<ParsedFile, CodeGraphError>;
}

pub trait ResolverBackend: Send + Sync {
    fn resolver_id(&self) -> ResolverId;
    fn resolver_version(&self) -> String;
    fn resolve(&self, input: ResolveInput<'_>) -> Result<ResolveOutput, CodeGraphError>;
}

pub trait LanguageBackend: Send + Sync {
    fn id(&self) -> BackendId;
    fn language(&self) -> Language;
    fn display_name(&self) -> &'static str;

    fn matches_path(&self, path: &Path) -> bool;
    fn parser(&self) -> &dyn ParserBackend;
    fn resolver(&self) -> Option<&dyn ResolverBackend>;

    fn workspace_markers(&self) -> &[WorkspaceMarker];
    fn metadata(&self, config: &BuildIndexOptions) -> BackendMetadata;
    fn config_fingerprint(&self, config: &BuildIndexOptions) -> String;
}
