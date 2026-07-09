use super::text::{is_subsequence, tokenize};
use super::types::{ContextCandidate, ContextQuery};
use std::collections::{HashMap, HashSet};

pub trait TokenEstimator {
    fn estimate_tokens(&self, text: &str) -> usize;
}

pub struct ApproxTokenEstimator;
impl TokenEstimator for ApproxTokenEstimator {
    fn estimate_tokens(&self, text: &str) -> usize {
        if text.is_empty() {
            0
        } else {
            text.chars().count().div_ceil(4)
        }
    }
}

pub trait ContextRanker {
    fn rank(
        &self,
        query: &ContextQuery,
        candidates: Vec<ContextCandidate>,
    ) -> Vec<ContextCandidate>;
}

pub struct GraphRanker;
impl ContextRanker for GraphRanker {
    fn rank(
        &self,
        query: &ContextQuery,
        candidates: Vec<ContextCandidate>,
    ) -> Vec<ContextCandidate> {
        let mut scored = candidates;
        for c in &mut scored {
            let mut graph_score = 0.0;
            // 1. Distance weight
            match c.distance {
                0 => graph_score += 10.0,
                1 => graph_score += 6.0,
                2 => graph_score += 3.0,
                3 => graph_score += 1.0,
                _ => {}
            }
            // 2. Locality weight: same file (+2.0) or same folder (+1.0) as any root
            let same_file = query.roots.iter().any(|r| r.file_path == c.file_path);
            let same_dir = query
                .roots
                .iter()
                .any(|r| r.file_path.parent() == c.file_path.parent());
            if same_file {
                graph_score += 2.0;
            } else if same_dir {
                graph_score += 1.0;
            }

            // 3. Edge confidence weight
            if let Some(ref edge) = c.via_edge
                && let Some(ref conf) = edge.confidence {
                    match conf.as_str() {
                        "LspExact" | "Exact" => graph_score += 2.0,
                        "Syntax" | "Local" => graph_score += 1.2,
                        "Heuristic" | "NameOnly" | "Ambiguous" => graph_score += 0.5,
                        "Unresolved" => graph_score -= 1.0,
                        _ => {}
                    }
                }
            // 4. Test penalty
            let is_test = c.node.name.to_lowercase().contains("test")
                || c.node.qualified_name.to_lowercase().contains("test")
                || c.file_path
                    .to_string_lossy()
                    .to_lowercase()
                    .contains("test");
            if is_test && !query.include_tests {
                graph_score -= 2.0;
            }
            // 5. Vendor/generated penalty
            let path_str = c.file_path.to_string_lossy().to_lowercase();
            let is_vendor_or_gen = path_str.contains("vendor")
                || path_str.contains("generated")
                || path_str.contains("target")
                || path_str.contains("node_modules");
            if is_vendor_or_gen {
                graph_score -= 4.0;
            }

            c.graph_score = graph_score;
            c.combined_score = graph_score;
        }
        scored.sort_by(|a, b| {
            b.combined_score
                .partial_cmp(&a.combined_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    a.node
                        .qualified_name
                        .len()
                        .cmp(&b.node.qualified_name.len())
                })
        });
        scored
    }
}

pub struct LexicalRanker;
impl ContextRanker for LexicalRanker {
    fn rank(
        &self,
        query: &ContextQuery,
        candidates: Vec<ContextCandidate>,
    ) -> Vec<ContextCandidate> {
        let mut scored = candidates;
        let total_docs = scored.len();
        let query_terms = tokenize(&query.query_string);
        if query_terms.is_empty() {
            return scored;
        }

        let mut doc_freq = HashMap::new();
        for cand in &scored {
            let mut unique_terms = HashSet::new();
            for term in tokenize(&cand.node.name) {
                unique_terms.insert(term);
            }
            for term in tokenize(&cand.node.qualified_name) {
                unique_terms.insert(term);
            }
            for term in tokenize(&cand.file_path.to_string_lossy()) {
                unique_terms.insert(term);
            }
            for term in unique_terms {
                *doc_freq.entry(term).or_insert(0) += 1;
            }
        }

        for c in &mut scored {
            let name_terms = tokenize(&c.node.name);
            let qual_terms = tokenize(&c.node.qualified_name);
            let path_terms = tokenize(&c.file_path.to_string_lossy());

            let mut lex_score = 0.0;
            for q in &query_terms {
                let n = *doc_freq.get(q).unwrap_or(&0);
                let idf =
                    (((total_docs as f32 - n as f32 + 0.5) / (n as f32 + 0.5) + 1.0).ln()).max(0.1);

                let mut term_score: f32 = 0.0;
                for t in &name_terms {
                    if t == q {
                        term_score = term_score.max(3.0 * idf);
                    } else if t.starts_with(q) || q.starts_with(t) {
                        term_score = term_score.max(1.5 * idf);
                    } else if is_subsequence(q, t) {
                        term_score = term_score.max(0.5 * idf);
                    }
                }
                for t in &qual_terms {
                    if t == q {
                        term_score = term_score.max(2.0 * idf);
                    } else if t.starts_with(q) || q.starts_with(t) {
                        term_score = term_score.max(1.0 * idf);
                    } else if is_subsequence(q, t) {
                        term_score = term_score.max(0.3 * idf);
                    }
                }
                for t in &path_terms {
                    if t == q {
                        term_score = term_score.max(1.0 * idf);
                    } else if t.starts_with(q) || q.starts_with(t) {
                        term_score = term_score.max(0.5 * idf);
                    }
                }
                lex_score += term_score;
            }
            c.lexical_score = lex_score;
            c.combined_score = lex_score;
        }
        scored.sort_by(|a, b| {
            b.combined_score
                .partial_cmp(&a.combined_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    a.node
                        .qualified_name
                        .len()
                        .cmp(&b.node.qualified_name.len())
                })
        });
        scored
    }
}

pub struct HybridRanker {
    pub graph_weight: f32,
    pub lexical_weight: f32,
}
impl ContextRanker for HybridRanker {
    fn rank(
        &self,
        query: &ContextQuery,
        candidates: Vec<ContextCandidate>,
    ) -> Vec<ContextCandidate> {
        let mut scored = GraphRanker.rank(query, candidates);
        let total_docs = scored.len();
        let query_terms = tokenize(&query.query_string);
        if !query_terms.is_empty() {
            let mut doc_freq = HashMap::new();
            for cand in &scored {
                let mut unique_terms = HashSet::new();
                for term in tokenize(&cand.node.name) {
                    unique_terms.insert(term);
                }
                for term in tokenize(&cand.node.qualified_name) {
                    unique_terms.insert(term);
                }
                for term in tokenize(&cand.file_path.to_string_lossy()) {
                    unique_terms.insert(term);
                }
                for term in unique_terms {
                    *doc_freq.entry(term).or_insert(0) += 1;
                }
            }

            for c in &mut scored {
                let name_terms = tokenize(&c.node.name);
                let qual_terms = tokenize(&c.node.qualified_name);
                let path_terms = tokenize(&c.file_path.to_string_lossy());

                let mut lex_score = 0.0;
                for q in &query_terms {
                    let n = *doc_freq.get(q).unwrap_or(&0);
                    let idf = (((total_docs as f32 - n as f32 + 0.5) / (n as f32 + 0.5) + 1.0)
                        .ln())
                    .max(0.1);

                    let mut term_score: f32 = 0.0;
                    for t in &name_terms {
                        if t == q {
                            term_score = term_score.max(3.0 * idf);
                        } else if t.starts_with(q) || q.starts_with(t) {
                            term_score = term_score.max(1.5 * idf);
                        } else if is_subsequence(q, t) {
                            term_score = term_score.max(0.5 * idf);
                        }
                    }
                    for t in &qual_terms {
                        if t == q {
                            term_score = term_score.max(2.0 * idf);
                        } else if t.starts_with(q) || q.starts_with(t) {
                            term_score = term_score.max(1.0 * idf);
                        } else if is_subsequence(q, t) {
                            term_score = term_score.max(0.3 * idf);
                        }
                    }
                    for t in &path_terms {
                        if t == q {
                            term_score = term_score.max(1.0 * idf);
                        } else if t.starts_with(q) || q.starts_with(t) {
                            term_score = term_score.max(0.5 * idf);
                        }
                    }
                    lex_score += term_score;
                }
                c.lexical_score = lex_score;
                c.combined_score =
                    self.graph_weight * c.graph_score + self.lexical_weight * lex_score;
            }
        } else {
            for c in &mut scored {
                c.lexical_score = 0.0;
                c.combined_score = self.graph_weight * c.graph_score;
            }
        }

        scored.sort_by(|a, b| {
            b.combined_score
                .partial_cmp(&a.combined_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    a.node
                        .qualified_name
                        .len()
                        .cmp(&b.node.qualified_name.len())
                })
        });
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EdgeDirection, LanguageObject, LanguageObjectKind, SourceRange, SymbolId};
    use std::path::PathBuf;

    #[test]
    fn hybrid_ranker_combined_formula_via_empty_query_string() {
        let path = PathBuf::from("/proj/lib/foo.rs");
        let roots = vec![LanguageObject {
            id: SymbolId(0),
            name: "root".to_string(),
            qualified_name: "root".to_string(),
            kind: LanguageObjectKind::Function,
            file_path: PathBuf::from("/proj/src/root.rs"),
            range: SourceRange {
                start_line: 1,
                start_col: 1,
                end_line: 5,
                end_col: 1,
            },
            signature: None,
            language: Some("rust".to_string()),
        }];
        let query = ContextQuery {
            query_string: "foo".to_string(),
            roots,
            include_tests: true,
        };

        let candidates = vec![ContextCandidate {
            node: LanguageObject {
                id: SymbolId(1),
                name: "foo".to_string(),
                qualified_name: "foo".to_string(),
                kind: LanguageObjectKind::Function,
                file_path: path.clone(),
                range: SourceRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 5,
                    end_col: 1,
                },
                signature: None,
                language: Some("rust".to_string()),
            },
            distance: 1,
            direction: EdgeDirection::Outbound,
            via_edge: None,
            file_path: path,
            range: SourceRange {
                start_line: 1,
                start_col: 1,
                end_line: 5,
                end_col: 1,
            },
            graph_score: 0.0,
            lexical_score: 0.0,
            combined_score: 0.0,
            estimated_tokens: 0,
            reason: "test".to_string(),
        }];

        let hybrid = HybridRanker {
            graph_weight: 0.6,
            lexical_weight: 0.4,
        };
        let scored = hybrid.rank(&query, candidates);
        let expected = hybrid.graph_weight * scored[0].graph_score
            + hybrid.lexical_weight * scored[0].lexical_score;
        assert!((scored[0].combined_score - expected).abs() < 0.01);
    }
}