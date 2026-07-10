use super::packing::{build_context_sections, pack_snippets};
use super::types::{
    ContextBudget, ContextCandidate, ContextPack, ContextPackingMode, DepthLimit, RankingMode,
};
use crate::ContextRetrievalOptions;
use crate::error::CodeGraphError;
use crate::model::{
    EdgeDirection, GraphContextDiagnostic, GraphContextMode, LanguageObject,
    LanguageObjectKind, SourceRange, extract_signature,
};
use crate::storage::{load_child_chunks, load_chunk, load_symbol, load_chunks_by_ids};
use ctx_codegraph_chunk::ChunkId;
use ctx_codegraph_search::{
    HybridQuery, HybridSearchBackend, HybridSearchOptions, HybridSearcher, SearchResult,
};
use crate::WorkspaceHybridBackend;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalStrategy {
    Hybrid,
    Graph,
    Lexical,
    Dense,
}

impl RetrievalStrategy {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "graph" => Self::Graph,
            "lexical" => Self::Lexical,
            "dense" => Self::Dense,
            _ => Self::Hybrid,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HybridRetrievalOptions {
    pub strategy: RetrievalStrategy,
    pub graph_options: ContextRetrievalOptions,
    pub hybrid_top_k: usize,
    pub rrf_k: usize,
    pub lexical_top_k: usize,
    pub dense_top_k: usize,
    pub enable_rerank: bool,
    pub rerank_top_k: usize,
    pub expansion_max_children: usize,
}

impl Default for HybridRetrievalOptions {
    fn default() -> Self {
        Self {
            strategy: RetrievalStrategy::Hybrid,
            graph_options: ContextRetrievalOptions {
                mode: GraphContextMode::Neighborhood,
                depth_limit: DepthLimit::Auto,
                max_nodes: 200,
                max_files: 50,
                ranking_mode: RankingMode::Hybrid,
                packing_mode: ContextPackingMode::Sandwich,
                with_snippets: true,
                context_lines: 3,
                include_tests: false,
                edge_kinds: Vec::new(),
                include_unresolved: false,
                explain_ranking: false,
            },
            hybrid_top_k: 30,
            rrf_k: 60,
            lexical_top_k: 50,
            dense_top_k: 50,
            enable_rerank: false,
            rerank_top_k: 20,
            expansion_max_children: 5,
        }
    }
}

pub fn retrieve_context_with_options(
    conn: &rusqlite::Connection,
    workspace: &Path,
    backend: &WorkspaceHybridBackend,
    query_str: &str,
    budget: &ContextBudget,
    options: &HybridRetrievalOptions,
) -> Result<ContextPack, CodeGraphError> {
    if options.strategy == RetrievalStrategy::Graph {
        return super::retrieval::retrieve_graph_context_with_options(
            conn,
            query_str,
            budget,
            &options.graph_options,
        );
    }

    let search_opts = HybridSearchOptions {
        rrf_k: options.rrf_k,
        lexical_top_k: options.lexical_top_k,
        dense_top_k: options.dense_top_k,
    };
    let searcher = HybridSearcher::new(backend, search_opts);
    let hybrid_query = HybridQuery {
        workspace_root: workspace,
        text: query_str,
        limit: options.hybrid_top_k,
    };

    let mut results = match options.strategy {
        RetrievalStrategy::Lexical => backend.search_lexical(hybrid_query).map_err(map_search_err)?,
        RetrievalStrategy::Dense => backend.search_dense(hybrid_query).map_err(map_search_err)?,
        RetrievalStrategy::Hybrid => searcher.search(hybrid_query).map_err(map_search_err)?,
        RetrievalStrategy::Graph => unreachable!(),
    };

    if options.enable_rerank {
        backend
            .rerank_results(conn, query_str, &mut results, options.rerank_top_k)
            .map_err(map_search_err)?;
    }

    pack_from_search_results(
        conn,
        workspace,
        query_str,
        budget,
        options,
        &results,
    )
}

fn map_search_err(e: ctx_codegraph_search::SearchError) -> CodeGraphError {
    CodeGraphError::Parse(e.to_string())
}

fn pack_from_search_results(
    conn: &rusqlite::Connection,
    workspace: &Path,
    query_str: &str,
    budget: &ContextBudget,
    options: &HybridRetrievalOptions,
    results: &[SearchResult],
) -> Result<ContextPack, CodeGraphError> {
    let _ = workspace;
    let mut diagnostics = Vec::new();
    let token_budget = budget.effective_budget().max(100);

    if results.is_empty() {
        diagnostics.push(GraphContextDiagnostic {
            severity: "warning".to_string(),
            message: "No hybrid search hits; try a different query or rebuild_index with embeddings."
                .to_string(),
        });
        return Ok(empty_pack(query_str, options, token_budget, diagnostics));
    }

    let chunk_ids: Vec<ChunkId> = results.iter().map(|r| r.chunk_id).collect();
    let chunks = load_chunks_by_ids(conn, &chunk_ids)?;
    let score_map: std::collections::HashMap<ChunkId, f32> = results
        .iter()
        .map(|r| (r.chunk_id, r.score))
        .collect();

    let mut candidates = Vec::new();
    let mut seen_symbols = HashSet::new();

    for chunk in &chunks {
        let relevance = score_map.get(&chunk.id.unwrap()).copied().unwrap_or(0.0);
        if let Some(symbol_id) = chunk.symbol_id {
            if !seen_symbols.insert(symbol_id) {
                continue;
            }
            if let Ok(sym) = load_symbol(conn, symbol_id) {
                let file_path = sym.file.clone();
                let range = SourceRange {
                    start_line: chunk.start_line,
                    start_col: 0,
                    end_line: chunk.end_line,
                    end_col: 0,
                };
                let node = LanguageObject {
                    id: symbol_id,
                    name: sym.name.clone(),
                    qualified_name: sym.qualified_name.clone(),
                    kind: LanguageObjectKind::from(sym.kind.clone()),
                    file_path: file_path.clone(),
                    range,
                    signature: extract_signature(&sym.file, &sym.range, sym.kind.clone()),
                    language: Some(sym.language.as_str().to_string()),
                };
                candidates.push(ContextCandidate {
                    node,
                    distance: 0,
                    direction: EdgeDirection::Outbound,
                    via_edge: None,
                    file_path,
                    range,
                    graph_score: relevance,
                    lexical_score: relevance,
                    combined_score: relevance,
                    estimated_tokens: chunk.token_count.max(1),
                    reason: format!("hybrid hit ({:?})", chunk.kind),
                });
            }
        }

        if let Some(parent_id) = chunk.parent_chunk_id
            && candidates.len() < options.graph_options.max_nodes
            && let Ok(Some(parent_chunk)) = load_chunk(conn, parent_id)
            && let Some(parent_sym) = parent_chunk.symbol_id
            && seen_symbols.insert(parent_sym)
            && let Ok(sym) = load_symbol(conn, parent_sym)
        {
            let file_path = sym.file.clone();
            let range = SourceRange {
                start_line: parent_chunk.start_line,
                start_col: 0,
                end_line: parent_chunk.end_line,
                end_col: 0,
            };
            let node = LanguageObject {
                id: parent_sym,
                name: sym.name.clone(),
                qualified_name: sym.qualified_name.clone(),
                kind: LanguageObjectKind::from(sym.kind.clone()),
                file_path: file_path.clone(),
                range,
                signature: extract_signature(&sym.file, &sym.range, sym.kind.clone()),
                language: Some(sym.language.as_str().to_string()),
            };
            candidates.push(ContextCandidate {
                node,
                distance: 1,
                direction: EdgeDirection::Outbound,
                via_edge: None,
                file_path,
                range,
                graph_score: relevance * 0.8,
                lexical_score: relevance * 0.8,
                combined_score: relevance * 0.8,
                estimated_tokens: parent_chunk.token_count.max(1),
                reason: "parent chunk expansion".to_string(),
            });
        }

        if options.expansion_max_children > 0
            && candidates.len() < options.graph_options.max_nodes
            && let Some(chunk_id) = chunk.id
        {
            let children =
                load_child_chunks(conn, chunk_id, options.expansion_max_children)?;
            for child_chunk in children {
                if candidates.len() >= options.graph_options.max_nodes {
                    break;
                }
                let Some(child_sym_id) = child_chunk.symbol_id else {
                    continue;
                };
                if !seen_symbols.insert(child_sym_id) {
                    continue;
                }
                if let Ok(sym) = load_symbol(conn, child_sym_id) {
                    let file_path = sym.file.clone();
                    let range = SourceRange {
                        start_line: child_chunk.start_line,
                        start_col: 0,
                        end_line: child_chunk.end_line,
                        end_col: 0,
                    };
                    let node = LanguageObject {
                        id: child_sym_id,
                        name: sym.name.clone(),
                        qualified_name: sym.qualified_name.clone(),
                        kind: LanguageObjectKind::from(sym.kind.clone()),
                        file_path: file_path.clone(),
                        range,
                        signature: extract_signature(&sym.file, &sym.range, sym.kind.clone()),
                        language: Some(sym.language.as_str().to_string()),
                    };
                    candidates.push(ContextCandidate {
                        node,
                        distance: 1,
                        direction: EdgeDirection::Outbound,
                        via_edge: None,
                        file_path,
                        range,
                        graph_score: relevance * 0.7,
                        lexical_score: relevance * 0.7,
                        combined_score: relevance * 0.7,
                        estimated_tokens: child_chunk.token_count.max(1),
                        reason: "child chunk expansion".to_string(),
                    });
                }
            }
        }
    }

    candidates.sort_by(|a, b| {
        b.combined_score
            .partial_cmp(&a.combined_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates.truncate(options.graph_options.max_nodes);

    let roots: Vec<LanguageObject> = candidates
        .iter()
        .take(3)
        .map(|c| c.node.clone())
        .collect();

    let roots_cand: Vec<ContextCandidate> = candidates.iter().take(3).cloned().collect();
    let neighbors_cand: Vec<ContextCandidate> = candidates.iter().skip(3).cloned().collect();

    let packed = pack_snippets(
        conn,
        &roots,
        roots_cand,
        neighbors_cand,
        token_budget,
        options.graph_options.max_nodes,
        options.graph_options.max_files,
        options.graph_options.with_snippets,
        options.graph_options.context_lines,
    );

    let built = build_context_sections(
        query_str,
        options.graph_options.mode,
        budget,
        &roots,
        &packed.included,
        &packed.omitted,
        &[],
        options.graph_options.packing_mode,
    );

    let nodes: Vec<LanguageObject> = packed
        .included
        .iter()
        .map(|(cand, _)| cand.node.clone())
        .collect();
    let snippets: Vec<super::types::ContextSnippet> = packed
        .included
        .into_iter()
        .map(|(_, snip)| snip)
        .collect();

    Ok(ContextPack {
        query: query_str.to_string(),
        mode: options.graph_options.mode,
        token_budget,
        requested_token_budget: Some(budget.token_budget),
        effective_token_budget: Some(token_budget),
        estimated_tokens: built.total_estimated_tokens,
        roots,
        nodes,
        edges: Vec::new(),
        snippets,
        sections: built.sections,
        omitted: packed.omitted,
        diagnostics,
    })
}

fn empty_pack(
    query_str: &str,
    options: &HybridRetrievalOptions,
    token_budget: usize,
    diagnostics: Vec<GraphContextDiagnostic>,
) -> ContextPack {
    ContextPack {
        query: query_str.to_string(),
        mode: options.graph_options.mode,
        token_budget,
        requested_token_budget: None,
        effective_token_budget: Some(token_budget),
        estimated_tokens: 0,
        roots: Vec::new(),
        nodes: Vec::new(),
        edges: Vec::new(),
        snippets: Vec::new(),
        sections: Vec::new(),
        omitted: Vec::new(),
        diagnostics,
    }
}