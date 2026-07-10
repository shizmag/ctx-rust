use ctx_codegraph_lang::model::{FileId, SymbolId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ChunkId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ChunkKind {
    ParentSummary,
    SymbolBody,
    SymbolDecl,
    Occurrence,
}

impl ChunkKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChunkKind::ParentSummary => "ParentSummary",
            ChunkKind::SymbolBody => "SymbolBody",
            ChunkKind::SymbolDecl => "SymbolDecl",
            ChunkKind::Occurrence => "Occurrence",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ParentSummary" => Some(ChunkKind::ParentSummary),
            "SymbolBody" => Some(ChunkKind::SymbolBody),
            "SymbolDecl" => Some(ChunkKind::SymbolDecl),
            "Occurrence" => Some(ChunkKind::Occurrence),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Chunk {
    pub id: Option<ChunkId>,
    pub symbol_id: Option<SymbolId>,
    pub parent_chunk_id: Option<ChunkId>,
    pub file_id: FileId,
    pub kind: ChunkKind,
    pub text_hash: String,
    pub token_count: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub qualified_name: String,
    pub text: Option<String>,
}