use super::ranking::{ApproxTokenEstimator, TokenEstimator};
use super::text::extract_snippet;
use super::types::{
    ContextBudget, ContextCandidate, ContextPackingMode, ContextSection, ContextSectionKind,
    ContextSnippet, OmittedContext,
};
use crate::model::{GraphContextMode, LanguageObject, SourceRange};
use crate::storage::load_symbol;
use std::collections::HashSet;

pub(crate) struct PackedSnippets {
    pub included: Vec<(ContextCandidate, ContextSnippet)>,
    pub omitted: Vec<OmittedContext>,
}

pub(crate) fn pack_snippets(
    conn: &rusqlite::Connection,
    roots: &[LanguageObject],
    roots_cand: Vec<ContextCandidate>,
    neighbors_cand: Vec<ContextCandidate>,
    raw_budget: usize,
    max_nodes: usize,
    max_files: usize,
    with_snippets: bool,
    context_lines: usize,
) -> PackedSnippets {
    let mut included_snippets = Vec::new();
    let mut omitted_candidates = Vec::new();

    let mut current_budget = raw_budget;

    for mut root in roots_cand {
        let body_range = match load_symbol(conn, root.node.id) {
            Ok(sym) => sym.body_range.map(SourceRange::from),
            Err(_) => None,
        };

        let text = if with_snippets {
            extract_snippet(&root.file_path, root.range, body_range, true, context_lines)
                .unwrap_or_default()
        } else {
            "".to_string()
        };

        let tokens = ApproxTokenEstimator.estimate_tokens(&text);
        root.estimated_tokens = tokens;
        current_budget = current_budget.saturating_sub(tokens);

        let snippet = ContextSnippet {
            file_path: root.file_path.clone(),
            range: root.range,
            symbol_id: Some(root.node.id),
            text,
            estimated_tokens: tokens,
            relevance: root.combined_score,
            reason: root.reason.clone(),
        };
        included_snippets.push((root, snippet));
    }

    let mut included_files = HashSet::new();
    for r in roots {
        included_files.insert(r.file_path.clone());
    }

    for mut neighbor in neighbors_cand {
        let total_nodes = included_snippets.len();
        if total_nodes >= max_nodes {
            omitted_candidates.push(OmittedContext {
                name: neighbor.node.name.clone(),
                qualified_name: neighbor.node.qualified_name.clone(),
                file_path: neighbor.file_path.clone(),
                score: neighbor.combined_score,
                reason: "Truncated: max_nodes limit reached".to_string(),
            });
            continue;
        }

        if !included_files.contains(&neighbor.file_path) {
            if included_files.len() >= max_files {
                omitted_candidates.push(OmittedContext {
                    name: neighbor.node.name.clone(),
                    qualified_name: neighbor.node.qualified_name.clone(),
                    file_path: neighbor.file_path.clone(),
                    score: neighbor.combined_score,
                    reason: "Truncated: max_files limit reached".to_string(),
                });
                continue;
            }
        }

        let body_range = match load_symbol(conn, neighbor.node.id) {
            Ok(sym) => sym.body_range.map(SourceRange::from),
            Err(_) => None,
        };

        let text = if with_snippets {
            extract_snippet(
                &neighbor.file_path,
                neighbor.range,
                body_range,
                false,
                context_lines,
            )
            .unwrap_or_default()
        } else {
            "".to_string()
        };

        let tokens = ApproxTokenEstimator.estimate_tokens(&text);
        if tokens > current_budget {
            omitted_candidates.push(OmittedContext {
                name: neighbor.node.name.clone(),
                qualified_name: neighbor.node.qualified_name.clone(),
                file_path: neighbor.file_path.clone(),
                score: neighbor.combined_score,
                reason: "Truncated: token budget exceeded".to_string(),
            });
            continue;
        }

        neighbor.estimated_tokens = tokens;
        current_budget = current_budget.saturating_sub(tokens);
        included_files.insert(neighbor.file_path.clone());

        let snippet = ContextSnippet {
            file_path: neighbor.file_path.clone(),
            range: neighbor.range,
            symbol_id: Some(neighbor.node.id),
            text,
            estimated_tokens: tokens,
            relevance: neighbor.combined_score,
            reason: neighbor.reason.clone(),
        };
        included_snippets.push((neighbor, snippet));
    }

    PackedSnippets {
        included: included_snippets,
        omitted: omitted_candidates,
    }
}

pub(crate) struct BuiltSections {
    pub sections: Vec<ContextSection>,
    pub total_estimated_tokens: usize,
}

pub(crate) fn build_context_sections(
    query_str: &str,
    mode: GraphContextMode,
    budget: &ContextBudget,
    roots: &[LanguageObject],
    included_snippets: &[(ContextCandidate, ContextSnippet)],
    omitted_candidates: &[OmittedContext],
    relationship_lines: &[String],
    packing_mode: ContextPackingMode,
) -> BuiltSections {
    let mut sections = Vec::new();
    let mut total_estimated_tokens = 0;
    for (_, snip) in included_snippets {
        total_estimated_tokens += snip.estimated_tokens;
    }

    let mut summary_text = String::new();
    summary_text.push_str(&format!("Query: {}\n", query_str));
    summary_text.push_str(&format!("Mode: {:?}\n", mode));
    summary_text.push_str(&format!("Token budget: {}\n", budget.effective_budget()));
    summary_text.push_str(&format!("Estimated tokens: {}\n", total_estimated_tokens));
    summary_text.push_str(&format!("Roots: {}\n\n", roots.len()));
    let summary_tokens = ApproxTokenEstimator.estimate_tokens(&summary_text);
    sections.push(ContextSection {
        kind: ContextSectionKind::Summary,
        text: summary_text,
        estimated_tokens: summary_tokens,
    });
    total_estimated_tokens += summary_tokens;

    let mut root_text = String::new();
    for r in roots {
        root_text.push_str("Root\n");
        root_text.push_str(&format!("  {}\n", r.qualified_name));
        root_text.push_str(&format!(
            "  {}:{}-{}\n\n",
            r.file_path.display(),
            r.range.start_line,
            r.range.end_line
        ));
    }
    let root_tokens = ApproxTokenEstimator.estimate_tokens(&root_text);
    sections.push(ContextSection {
        kind: ContextSectionKind::Root,
        text: root_text,
        estimated_tokens: root_tokens,
    });
    total_estimated_tokens += root_tokens;

    let mut rel_text = String::new();
    if !relationship_lines.is_empty() {
        rel_text.push_str("Top relationships\n");
        for line in relationship_lines {
            rel_text.push_str(&format!("{}\n", line));
        }
        rel_text.push('\n');
    }
    let rel_tokens = ApproxTokenEstimator.estimate_tokens(&rel_text);
    sections.push(ContextSection {
        kind: ContextSectionKind::DirectRelationships,
        text: rel_text,
        estimated_tokens: rel_tokens,
    });
    total_estimated_tokens += rel_tokens;

    let mut files_text = String::new();
    files_text.push_str("Files to read\n");
    let mut files_list: Vec<String> = included_snippets
        .iter()
        .map(|(c, _)| c.file_path.to_string_lossy().to_string())
        .collect();
    files_list.sort();
    files_list.dedup();
    for (i, f) in files_list.iter().enumerate() {
        files_text.push_str(&format!("  {}. {}\n", i + 1, f));
    }
    files_text.push('\n');
    let files_tokens = ApproxTokenEstimator.estimate_tokens(&files_text);
    sections.push(ContextSection {
        kind: ContextSectionKind::KeyNeighbors,
        text: files_text,
        estimated_tokens: files_tokens,
    });
    total_estimated_tokens += files_tokens;

    let mut snippets_text = String::new();
    snippets_text.push_str("Context\n");

    let mut ordered_snippets = included_snippets.to_vec();
    match packing_mode {
        ContextPackingMode::Frontloaded => {
            ordered_snippets.sort_by(|a, b| {
                b.0.combined_score
                    .partial_cmp(&a.0.combined_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        ContextPackingMode::Sandwich => {
            // Keep default order (roots then neighbor scores) which naturally puts roots & highest score neighbors at top
        }
        ContextPackingMode::Balanced => {
            ordered_snippets.sort_by(|a, b| a.0.file_path.cmp(&b.0.file_path));
        }
    }

    for (cand, snip) in &ordered_snippets {
        snippets_text.push_str(&format!(
            "  --- {}:{}-{} {}\n",
            cand.file_path.display(),
            cand.range.start_line,
            cand.range.end_line,
            cand.node.qualified_name
        ));
        snippets_text.push_str(&snip.text);
        if !snip.text.ends_with('\n') && !snip.text.is_empty() {
            snippets_text.push('\n');
        }
        snippets_text.push('\n');
    }
    let snippets_tokens = ApproxTokenEstimator.estimate_tokens(&snippets_text);
    sections.push(ContextSection {
        kind: ContextSectionKind::Snippets,
        text: snippets_text,
        estimated_tokens: snippets_tokens,
    });

    let mut end_text = String::new();

    let mut sorted_recap = included_snippets.to_vec();
    sorted_recap.sort_by(|a, b| {
        b.0.combined_score
            .partial_cmp(&a.0.combined_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if !sorted_recap.is_empty() {
        end_text.push_str("Most important context recap\n");
        for (i, (cand, _)) in sorted_recap.iter().take(3).enumerate() {
            let label = match i {
                0 => "Start with",
                1 => "Then read",
                _ => "Then inspect",
            };
            end_text.push_str(&format!(
                "  {}. {} {}\n",
                i + 1,
                label,
                cand.node.qualified_name
            ));
        }
        end_text.push('\n');
    }

    if !omitted_candidates.is_empty() {
        end_text.push_str("Diagnostics\n");
        end_text.push_str(&format!(
            "  Context truncated: {} candidates omitted due to token budget.\n\n",
            omitted_candidates.len()
        ));
    }
    let end_tokens = ApproxTokenEstimator.estimate_tokens(&end_text);
    sections.push(ContextSection {
        kind: ContextSectionKind::OmittedSummary,
        text: end_text,
        estimated_tokens: end_tokens,
    });
    total_estimated_tokens += end_tokens;

    BuiltSections {
        sections,
        total_estimated_tokens,
    }
}