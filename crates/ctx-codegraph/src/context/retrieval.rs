use super::packing::{build_context_sections, pack_snippets};
use super::ranking::{
    ApproxTokenEstimator, ContextRanker, GraphRanker, HybridRanker, LexicalRanker, TokenEstimator,
};
use super::roots::resolve_roots;
use super::text::extract_snippet;
use super::types::{
    ContextBudget, ContextCandidate, ContextPack, ContextPackingMode, ContextQuery, DepthLimit,
    RankingMode,
};
use crate::error::CodeGraphError;
use crate::model::{
    EdgeDirection, EdgeKind, GraphContextDiagnostic, GraphContextEdge, GraphContextMode,
    LanguageObject, LanguageObjectKind, ResolutionConfidence, ResolvedEdgeTarget, SourceRange,
    SymbolId, extract_signature,
};
use crate::storage::{load_edges_for_symbol, load_edges_from, load_edges_to, load_symbol};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct ContextRetrievalOptions {
    pub mode: GraphContextMode,
    pub depth_limit: DepthLimit,
    pub max_nodes: usize,
    pub max_files: usize,
    pub ranking_mode: RankingMode,
    pub packing_mode: ContextPackingMode,
    pub with_snippets: bool,
    pub context_lines: usize,
    pub include_tests: bool,
    pub edge_kinds: Vec<EdgeKind>,
    pub include_unresolved: bool,
    pub explain_ranking: bool,
}

pub fn retrieve_graph_context_with_options(
    conn: &rusqlite::Connection,
    query_str: &str,
    budget: &ContextBudget,
    options: &ContextRetrievalOptions,
) -> Result<ContextPack, CodeGraphError> {
    let ContextRetrievalOptions {
        mode,
        depth_limit,
        max_nodes,
        max_files,
        ranking_mode,
        packing_mode,
        with_snippets,
        context_lines,
        include_tests,
        edge_kinds,
        include_unresolved,
        explain_ranking,
    } = options;

    let mut diagnostics = Vec::new();
    let mut requested_token_budget = None;
    let mut effective_token_budget = None;
    let mut raw_budget = budget.effective_budget();

    if budget.token_budget < 100 {
        diagnostics.push(GraphContextDiagnostic {
            severity: "warning".to_string(),
            message: format!(
                "Requested token budget {} is below minimum 100; using 100.",
                budget.token_budget
            ),
        });
        raw_budget = 100;
        requested_token_budget = Some(budget.token_budget);
        effective_token_budget = Some(100);
    }

    let roots = resolve_roots(conn, query_str, 5)?;
    if roots.is_empty() {
        diagnostics.push(GraphContextDiagnostic {
            severity: "error".to_string(),
            message: format!("Symbol not found: {}", query_str),
        });
        return Ok(ContextPack {
            query: query_str.to_string(),
            mode: *mode,
            token_budget: raw_budget,
            requested_token_budget,
            effective_token_budget,
            estimated_tokens: 0,
            roots: Vec::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            snippets: Vec::new(),
            sections: Vec::new(),
            omitted: Vec::new(),
            diagnostics,
        });
    }

    let kinds = if edge_kinds.is_empty() {
        match mode {
            GraphContextMode::Callers | GraphContextMode::Callees => {
                vec![EdgeKind::Call, EdgeKind::Reference]
            }
            GraphContextMode::Dependencies | GraphContextMode::Dependents => {
                vec![
                    EdgeKind::Import,
                    EdgeKind::Call,
                    EdgeKind::TypeUse,
                    EdgeKind::Reference,
                ]
            }
            _ => {
                vec![
                    EdgeKind::Call,
                    EdgeKind::Reference,
                    EdgeKind::Import,
                    EdgeKind::Export,
                    EdgeKind::TypeUse,
                    EdgeKind::Inherits,
                    EdgeKind::Implements,
                    EdgeKind::DataFlow,
                    EdgeKind::Contains,
                ]
            }
        }
    } else {
        edge_kinds.clone()
    };

    let mut visited = HashSet::new();
    let mut candidates = Vec::new();
    let mut current_layer = Vec::new();
    for r in &roots {
        visited.insert(r.id);
        let cand = ContextCandidate {
            node: r.clone(),
            distance: 0,
            direction: EdgeDirection::Outbound,
            via_edge: None,
            file_path: r.file_path.clone(),
            range: r.range,
            graph_score: 0.0,
            lexical_score: 0.0,
            combined_score: 0.0,
            estimated_tokens: 0,
            reason: "Root symbol".to_string(),
        };
        current_layer.push(cand);
    }
    candidates.extend(current_layer.clone());

    let mut remaining_budget = raw_budget;

    let auto_depth_min = 1;
    let auto_depth_max = 3;
    let frontier_limit_per_depth = 50;
    let marginal_score_threshold = 0.12;

    let is_auto = match depth_limit {
        DepthLimit::Auto => true,
        DepthLimit::Fixed(_) => false,
    };
    let max_depth = match depth_limit {
        DepthLimit::Fixed(d) => *d,
        DepthLimit::Auto => auto_depth_max,
    };

    let mut unresolved_filtered_count = 0;
    let mut depth = 0;
    while depth < max_depth {
        let mut next_layer_candidates = Vec::new();

        for parent in &current_layer {
            let mut traverse_directions = Vec::new();
            match mode {
                GraphContextMode::Callers
                | GraphContextMode::Dependents
                | GraphContextMode::ReverseSlice
                | GraphContextMode::Reverse => {
                    traverse_directions.push(EdgeDirection::Inbound);
                }
                GraphContextMode::Callees
                | GraphContextMode::Dependencies
                | GraphContextMode::ForwardSlice
                | GraphContextMode::Forward => {
                    traverse_directions.push(EdgeDirection::Outbound);
                }
                GraphContextMode::Neighborhood => {
                    traverse_directions.push(EdgeDirection::Inbound);
                    traverse_directions.push(EdgeDirection::Outbound);
                }
                GraphContextMode::Impact => {
                    traverse_directions.push(EdgeDirection::Inbound);
                    if depth < 1 {
                        traverse_directions.push(EdgeDirection::Outbound);
                    }
                }
            }

            for dir in traverse_directions {
                let edges = load_edges_for_symbol(conn, parent.node.id, dir, &kinds)?;
                for (edge, target) in edges {
                    if !include_unresolved && edge.confidence == ResolutionConfidence::Unresolved {
                        unresolved_filtered_count += 1;
                        continue;
                    }
                    let target_sym = match target {
                        ResolvedEdgeTarget::Symbol(s) => s,
                        _ => continue,
                    };
                    let target_id = target_sym.id.unwrap();
                    if visited.insert(target_id) {
                        let node = LanguageObject {
                            id: target_id,
                            name: target_sym.name.clone(),
                            qualified_name: target_sym.qualified_name.clone(),
                            kind: LanguageObjectKind::from(target_sym.kind.clone()),
                            file_path: target_sym.file.clone(),
                            range: SourceRange::from(target_sym.range.clone()),
                            signature: extract_signature(&target_sym.file, &target_sym.range, target_sym.kind.clone()),
                            language: Some(target_sym.language.as_str().to_string()),
                        };

                        let via_context_edge = GraphContextEdge {
                            from: edge.from_symbol_id.unwrap_or(SymbolId(0)),
                            to: edge.to_symbol_id.unwrap_or(SymbolId(0)),
                            label: edge.raw_text.clone(),
                            confidence: Some(edge.confidence.as_str().to_string()),
                        };

                        let dir_str = match dir {
                            EdgeDirection::Inbound => "inbound",
                            EdgeDirection::Outbound => "outbound",
                        };
                        let reason = format!("{} relationship to {}", dir_str, parent.node.name);

                        let cand = ContextCandidate {
                            node,
                            distance: depth + 1,
                            direction: dir,
                            via_edge: Some(via_context_edge),
                            file_path: target_sym.file.clone(),
                            range: SourceRange::from(target_sym.range),
                            graph_score: 0.0,
                            lexical_score: 0.0,
                            combined_score: 0.0,
                            estimated_tokens: 0,
                            reason,
                        };
                        next_layer_candidates.push(cand);
                    }
                }
            }
        }

        if next_layer_candidates.is_empty() {
            break;
        }

        let query_obj = ContextQuery {
            query_string: query_str.to_string(),
            roots: roots.clone(),
            include_tests: *include_tests,
        };

        let ranker: Box<dyn ContextRanker> = match ranking_mode {
            RankingMode::Graph => Box::new(GraphRanker),
            RankingMode::Lexical => Box::new(LexicalRanker),
            RankingMode::Hybrid => Box::new(HybridRanker {
                graph_weight: 1.0,
                lexical_weight: 1.0,
            }),
        };

        let mut ranked_layer = ranker.rank(&query_obj, next_layer_candidates);

        if is_auto {
            ranked_layer.retain(|cand| cand.combined_score >= marginal_score_threshold);
            ranked_layer.truncate(frontier_limit_per_depth);
            if ranked_layer.is_empty() {
                break;
            }

            let mut estimated_cost = 0;
            for c in &ranked_layer {
                let range = c.range;
                let body_range = match load_symbol(conn, c.node.id) {
                    Ok(sym) => sym.body_range.map(SourceRange::from),
                    Err(_) => None,
                };
                if let Ok(snippet_text) =
                    extract_snippet(&c.file_path, range, body_range, false, *context_lines)
                {
                    estimated_cost += ApproxTokenEstimator.estimate_tokens(&snippet_text);
                } else {
                    estimated_cost += 100;
                }
            }

            if estimated_cost > remaining_budget && depth >= auto_depth_min {
                break;
            }

            remaining_budget = remaining_budget.saturating_sub(estimated_cost);
        }

        candidates.extend(ranked_layer.clone());
        current_layer = ranked_layer;
        depth += 1;
    }

    if unresolved_filtered_count > 0 {
        diagnostics.push(GraphContextDiagnostic {
            severity: "info".to_string(),
            message: format!(
                "Filtered {} unresolved edges because include_unresolved=false.",
                unresolved_filtered_count
            ),
        });
    }

    let query_obj = ContextQuery {
        query_string: query_str.to_string(),
        roots: roots.clone(),
        include_tests: *include_tests,
    };
    let ranker: Box<dyn ContextRanker> = match ranking_mode {
        RankingMode::Graph => Box::new(GraphRanker),
        RankingMode::Lexical => Box::new(LexicalRanker),
        RankingMode::Hybrid => Box::new(HybridRanker {
            graph_weight: 1.0,
            lexical_weight: 1.0,
        }),
    };
    let mut final_ranked = ranker.rank(&query_obj, candidates);

    if *explain_ranking {
        for c in &mut final_ranked {
            c.reason = format!(
                "{} (graph: {:.1}, lexical: {:.1})",
                c.reason, c.graph_score, c.lexical_score
            );
        }
    }

    let (roots_cand, mut neighbors_cand): (Vec<ContextCandidate>, Vec<ContextCandidate>) =
        final_ranked
            .into_iter()
            .partition(|c| roots.iter().any(|r| r.id == c.node.id));

    neighbors_cand.sort_by(|a, b| {
        b.combined_score
            .partial_cmp(&a.combined_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let packed = pack_snippets(
        conn,
        &roots,
        roots_cand,
        neighbors_cand,
        raw_budget,
        *max_nodes,
        *max_files,
        *with_snippets,
        *context_lines,
    );

    if !packed.omitted.is_empty() {
        let count = packed.omitted.len();
        diagnostics.push(GraphContextDiagnostic {
            severity: "warning".to_string(),
            message: format!(
                "Context truncated: {} candidates omitted due to token budget.",
                count
            ),
        });
    }

    let mut relationship_lines = Vec::new();
    for r in &roots {
        let outbound = load_edges_from(conn, r.id, &kinds)?;
        for (edge, target) in outbound {
            let target_name = target.map(|t| t.qualified_name).unwrap_or_else(|| {
                edge.to_external
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string())
            });
            relationship_lines.push(format!(
                "  [out {:?}] {} -> {}",
                edge.kind, r.qualified_name, target_name
            ));
        }
        let inbound = load_edges_to(conn, r.id, &kinds)?;
        for (edge, source) in inbound {
            relationship_lines.push(format!(
                "  [in {:?}] {} -> {}",
                edge.kind, source.qualified_name, r.qualified_name
            ));
        }
    }
    relationship_lines.sort();
    relationship_lines.dedup();
    relationship_lines.truncate(10);

    let built = build_context_sections(
        query_str,
        *mode,
        budget,
        &roots,
        &packed.included,
        &packed.omitted,
        &relationship_lines,
        *packing_mode,
    );

    let nodes: Vec<LanguageObject> = packed
        .included
        .iter()
        .map(|(cand, _)| cand.node.clone())
        .collect();
    let edges: Vec<GraphContextEdge> = packed
        .included
        .iter()
        .filter_map(|(cand, _)| cand.via_edge.clone())
        .collect();
    let snippets: Vec<super::types::ContextSnippet> = packed
        .included
        .into_iter()
        .map(|(_, snip)| snip)
        .collect();

    Ok(ContextPack {
        query: query_str.to_string(),
        mode: *mode,
        token_budget: raw_budget,
        requested_token_budget,
        effective_token_budget,
        estimated_tokens: built.total_estimated_tokens,
        roots,
        nodes,
        edges,
        snippets,
        sections: built.sections,
        omitted: packed.omitted,
        diagnostics,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn retrieve_graph_context(
    conn: &rusqlite::Connection,
    query_str: &str,
    mode: GraphContextMode,
    depth_limit: DepthLimit,
    max_nodes: usize,
    max_files: usize,
    ranking_mode: RankingMode,
    packing_mode: ContextPackingMode,
    with_snippets: bool,
    context_lines: usize,
    budget: &ContextBudget,
    include_tests: bool,
    edge_kinds: &[EdgeKind],
    include_unresolved: bool,
    explain_ranking: bool,
) -> Result<ContextPack, CodeGraphError> {
    retrieve_graph_context_with_options(
        conn,
        query_str,
        budget,
        &ContextRetrievalOptions {
            mode,
            depth_limit,
            max_nodes,
            max_files,
            ranking_mode,
            packing_mode,
            with_snippets,
            context_lines,
            include_tests,
            edge_kinds: edge_kinds.to_vec(),
            include_unresolved,
            explain_ranking,
        },
    )
}