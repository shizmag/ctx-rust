use crate::model::{
    GraphContextDiagnostic, GraphContextEdge, GraphContextMode, LanguageObject, SourceRange,
    SymbolId,
};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DepthLimit {
    Fixed(usize),
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RankingMode {
    Graph,
    Lexical,
    Hybrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ContextPackingMode {
    Frontloaded,
    Sandwich,
    Balanced,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContextCandidate {
    pub node: LanguageObject,
    pub distance: usize,
    pub direction: crate::model::EdgeDirection,
    pub via_edge: Option<GraphContextEdge>,
    pub file_path: PathBuf,
    pub range: SourceRange,
    pub graph_score: f32,
    pub lexical_score: f32,
    pub combined_score: f32,
    pub estimated_tokens: usize,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OmittedContext {
    pub name: String,
    pub qualified_name: String,
    pub file_path: PathBuf,
    pub score: f32,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContextSnippet {
    pub file_path: PathBuf,
    pub range: SourceRange,
    pub symbol_id: Option<SymbolId>,
    pub text: String,
    pub estimated_tokens: usize,
    pub relevance: f32,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ContextSectionKind {
    Summary,
    Root,
    DirectRelationships,
    KeyNeighbors,
    Snippets,
    OmittedSummary,
    Diagnostics,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContextSection {
    pub kind: ContextSectionKind,
    pub text: String,
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContextPack {
    pub query: String,
    pub mode: GraphContextMode,
    pub token_budget: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_token_budget: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_token_budget: Option<usize>,
    pub estimated_tokens: usize,
    pub roots: Vec<LanguageObject>,
    pub nodes: Vec<LanguageObject>,
    pub edges: Vec<GraphContextEdge>,
    pub snippets: Vec<ContextSnippet>,
    pub sections: Vec<ContextSection>,
    pub omitted: Vec<OmittedContext>,
    pub diagnostics: Vec<GraphContextDiagnostic>,
}

pub struct ContextBudget {
    pub token_budget: usize,
    pub model_context_window: Option<usize>,
    pub reserve_output_tokens: usize,
    pub reserve_instruction_tokens: usize,
}

impl ContextBudget {
    pub fn effective_budget(&self) -> usize {
        let max_from_window = match self.model_context_window {
            Some(w) => {
                let reserved = self.reserve_output_tokens + self.reserve_instruction_tokens;
                if w > reserved {
                    w - reserved
                } else {
                    0
                }
            }
            None => usize::MAX,
        };
        self.token_budget.min(max_from_window)
    }
}

pub struct ContextQuery {
    pub query_string: String,
    pub roots: Vec<LanguageObject>,
    pub include_tests: bool,
}