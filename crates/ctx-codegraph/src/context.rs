use crate::error::CodeGraphError;
use crate::model::{
    EdgeDirection, EdgeKind, FileId, GraphContextDiagnostic, GraphContextEdge, GraphContextMode,
    Language, LanguageObject, LanguageObjectKind, ResolutionConfidence, ResolvedEdgeTarget,
    SourceRange, Symbol, SymbolId, SymbolKind, TextRange,
};
use crate::storage::{load_edges_for_symbol, load_edges_from, load_edges_to, load_symbol};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
    pub direction: EdgeDirection,
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
                if w > reserved { w - reserved } else { 0 }
            }
            None => usize::MAX,
        };
        self.token_budget.min(max_from_window)
    }
}

pub trait TokenEstimator {
    fn estimate_tokens(&self, text: &str) -> usize;
}

pub struct ApproxTokenEstimator;
impl TokenEstimator for ApproxTokenEstimator {
    fn estimate_tokens(&self, text: &str) -> usize {
        if text.is_empty() {
            0
        } else {
            (text.chars().count() + 3) / 4
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

pub struct ContextQuery {
    pub query_string: String,
    pub roots: Vec<LanguageObject>,
    pub include_tests: bool,
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
            if let Some(ref edge) = c.via_edge {
                if let Some(ref conf) = edge.confidence {
                    match conf.as_str() {
                        "LspExact" | "Exact" => graph_score += 2.0,
                        "Syntax" | "Local" => graph_score += 1.2,
                        "Heuristic" | "NameOnly" | "Ambiguous" => graph_score += 0.5,
                        "Unresolved" => graph_score -= 1.0,
                        _ => {}
                    }
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

pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len() {
        let c = chars[i];
        if c == ':' || c == '.' || c == '-' || c == '_' || c == '/' || c == '\\' {
            if !current.is_empty() {
                tokens.push(current.to_lowercase());
                current.clear();
            }
        } else if c.is_uppercase() {
            let prev_is_lower = i > 0 && chars[i - 1].is_lowercase();
            let next_is_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();
            if prev_is_lower || next_is_lower {
                if !current.is_empty() {
                    tokens.push(current.to_lowercase());
                    current.clear();
                }
            }
            current.push(c);
        } else if c.is_whitespace() {
            if !current.is_empty() {
                tokens.push(current.to_lowercase());
                current.clear();
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(current.to_lowercase());
    }

    let lower = text.to_lowercase();
    if !tokens.contains(&lower) {
        tokens.push(lower);
    }

    tokens
}

pub fn is_subsequence(sub: &str, full: &str) -> bool {
    let mut sub_chars = sub.chars();
    let mut current_sub = sub_chars.next();
    if current_sub.is_none() {
        return true;
    }
    for c in full.chars() {
        if Some(c) == current_sub {
            current_sub = sub_chars.next();
            if current_sub.is_none() {
                return true;
            }
        }
    }
    false
}

pub fn extract_snippet(
    file_path: &Path,
    range: SourceRange,
    body_range: Option<SourceRange>,
    is_root: bool,
    context_lines: usize,
) -> Result<String, std::io::Error> {
    let content = std::fs::read_to_string(file_path)?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Ok("".to_string());
    }

    let mut start_line = range.start_line;
    let mut end_line = range.end_line;

    let limit = if is_root { 160 } else { 80 };

    if let Some(br) = body_range {
        let body_len = br.end_line.saturating_sub(br.start_line) + 1;
        if body_len <= limit {
            start_line = br.start_line.saturating_sub(context_lines).max(1);
            end_line = (br.end_line + context_lines).min(lines.len());
        } else {
            let top_limit = 15;
            let bottom_limit = 15;

            let top_end = br.start_line + top_limit;
            let bottom_start = br.end_line.saturating_sub(bottom_limit);

            let mut snippet = String::new();
            let start = range.start_line.saturating_sub(context_lines).max(1);
            let end_top = top_end.min(lines.len());
            for i in (start - 1)..end_top {
                snippet.push_str(lines[i]);
                snippet.push('\n');
            }

            let omitted = bottom_start.saturating_sub(end_top);
            if omitted > 0 {
                snippet.push_str(&format!("// ... {} lines omitted ...\n", omitted));
            }

            let start_bot = bottom_start.max(end_top + 1);
            let end_bot = (br.end_line + context_lines).min(lines.len());
            for i in (start_bot - 1)..end_bot {
                snippet.push_str(lines[i]);
                snippet.push('\n');
            }
            return Ok(snippet);
        }
    } else {
        start_line = start_line.saturating_sub(context_lines).max(1);
        end_line = (end_line + context_lines).min(lines.len());
    }

    if start_line > lines.len() {
        return Ok("".to_string());
    }
    let end = std::cmp::min(end_line, lines.len());
    if start_line > end {
        return Ok("".to_string());
    }

    let mut snippet = String::new();
    for i in (start_line - 1)..end {
        snippet.push_str(lines[i]);
        snippet.push('\n');
    }

    Ok(snippet)
}

pub fn resolve_roots(
    conn: &rusqlite::Connection,
    query: &str,
    max_roots: usize,
) -> Result<Vec<LanguageObject>, CodeGraphError> {
    let mut stmt = conn.prepare(
        "
        SELECT s.id, s.file_id, s.name, s.qualified_name, s.kind, s.language,
               s.start_line, s.start_col, s.end_line, s.end_col,
               s.body_start_line, s.body_start_col, s.body_end_line, s.body_end_col,
               f.path
        FROM symbols s
        JOIN files f ON s.file_id = f.id
    ",
    )?;
    let mut all_rows = stmt.query([])?;
    let mut all_symbols = Vec::new();
    while let Some(row) = all_rows.next()? {
        let id: i64 = row.get(0)?;
        let file_id: i64 = row.get(1)?;
        let name: String = row.get(2)?;
        let qualified_name: String = row.get(3)?;
        let kind_str: String = row.get(4)?;
        let lang_str: String = row.get(5)?;
        let start_line: usize = row.get(6)?;
        let start_col: usize = row.get(7)?;
        let end_line: usize = row.get(8)?;
        let end_col: usize = row.get(9)?;
        let body_start_line: Option<usize> = row.get(10)?;
        let body_start_col: Option<usize> = row.get(11)?;
        let body_end_line: Option<usize> = row.get(12)?;
        let body_end_col: Option<usize> = row.get(13)?;
        let file_path: String = row.get(14)?;

        let body_range = if let (Some(sl), Some(sc), Some(el), Some(ec)) =
            (body_start_line, body_start_col, body_end_line, body_end_col)
        {
            Some(TextRange {
                start_line: sl,
                start_col: sc,
                end_line: el,
                end_col: ec,
            })
        } else {
            None
        };

        all_symbols.push(Symbol {
            id: Some(SymbolId(id)),
            file_id: Some(FileId(file_id)),
            name,
            qualified_name,
            kind: SymbolKind::from_str(&kind_str).unwrap_or(SymbolKind::Function),
            language: Language(lang_str),
            file: PathBuf::from(file_path),
            range: TextRange {
                start_line,
                start_col,
                end_line,
                end_col,
            },
            body_range,
        });
    }

    #[derive(Debug, Clone)]
    struct RootCandidate {
        symbol: Symbol,
        score: f32,
        match_type: usize,
    }

    let query_lower = query.to_lowercase();
    let query_terms = tokenize(query);
    let mut scored_candidates = Vec::new();

    for sym in all_symbols {
        let name_lower = sym.name.to_lowercase();
        let qual_lower = sym.qualified_name.to_lowercase();
        let file_str = sym.file.to_string_lossy();
        let file_lower = file_str.to_lowercase();
        let file_stem = sym
            .file
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        let mut match_type = 6;
        let mut score = 0.0;

        if sym.qualified_name == query {
            match_type = 0;
            score = 100.0;
        } else if sym.name == query {
            match_type = 1;
            score = 90.0;
        } else if qual_lower == query_lower {
            match_type = 2;
            score = 80.0;
        } else if name_lower == query_lower {
            match_type = 2;
            score = 75.0;
        } else if file_str == query || file_lower == query_lower {
            match_type = 3;
            score = 70.0;
        } else if file_stem == query_lower {
            match_type = 4;
            score = 60.0;
        } else if qual_lower.contains(&query_lower) || name_lower.contains(&query_lower) {
            match_type = 5;
            score = 40.0;
        } else if file_lower.contains(&query_lower) {
            match_type = 5;
            score = 30.0;
        } else {
            let sym_terms = tokenize(&sym.qualified_name);
            let mut matched_terms = 0;
            for q in &query_terms {
                if sym_terms.contains(q) {
                    matched_terms += 1;
                }
            }
            if matched_terms > 0 {
                match_type = 6;
                score = 10.0 * (matched_terms as f32);
            }
        }

        if score > 0.0 {
            let is_test = name_lower.contains("test")
                || qual_lower.contains("test")
                || file_lower.contains("test");
            if is_test {
                score -= 5.0;
            }

            let is_external = file_lower.contains("vendor")
                || file_lower.contains("node_modules")
                || file_lower.contains("generated")
                || file_lower.contains("target");
            if is_external {
                score -= 10.0;
            }

            scored_candidates.push(RootCandidate {
                symbol: sym,
                score,
                match_type,
            });
        }
    }

    scored_candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.symbol
                    .qualified_name
                    .len()
                    .cmp(&b.symbol.qualified_name.len())
            })
    });

    let mut exact_matches = Vec::new();
    for c in &scored_candidates {
        if c.match_type <= 2 {
            exact_matches.push(c.symbol.clone());
        }
    }

    let final_roots = if !exact_matches.is_empty() {
        exact_matches.into_iter().take(max_roots).collect()
    } else if !scored_candidates.is_empty() {
        vec![scored_candidates[0].symbol.clone()]
    } else {
        Vec::new()
    };

    let mut results = Vec::new();
    for sym in final_roots {
        let id = sym.id.unwrap_or(SymbolId(0));
        let name = sym.name;
        let qualified_name = sym.qualified_name;
        let kind = LanguageObjectKind::from(sym.kind);
        let file_path = sym.file;
        let range = SourceRange::from(sym.range);
        let language = Some(sym.language.as_str().to_string());

        results.push(LanguageObject {
            id,
            name,
            qualified_name,
            kind,
            file_path,
            range,
            signature: None,
            language,
        });
    }

    Ok(results)
}

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
            mode,
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
        edge_kinds.to_vec()
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
        DepthLimit::Fixed(d) => d,
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
                            kind: LanguageObjectKind::from(target_sym.kind),
                            file_path: target_sym.file.clone(),
                            range: SourceRange::from(target_sym.range.clone()),
                            signature: None,
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
            include_tests,
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
                    extract_snippet(&c.file_path, range, body_range, false, context_lines)
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
        include_tests,
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

    if explain_ranking {
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
    for r in &roots {
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

    if !omitted_candidates.is_empty() {
        let count = omitted_candidates.len();
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

    let mut sections = Vec::new();
    let mut total_estimated_tokens = 0;
    for (_, snip) in &included_snippets {
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
    for r in &roots {
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
        for line in &relationship_lines {
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

    let mut ordered_snippets = included_snippets.clone();
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

    let mut sorted_recap = included_snippets.clone();
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

    let nodes: Vec<LanguageObject> = included_snippets
        .iter()
        .map(|(cand, _)| cand.node.clone())
        .collect();
    let edges: Vec<GraphContextEdge> = included_snippets
        .iter()
        .filter_map(|(cand, _)| cand.via_edge.clone())
        .collect();
    let snippets: Vec<ContextSnippet> = included_snippets
        .into_iter()
        .map(|(_, snip)| snip)
        .collect();

    Ok(ContextPack {
        query: query_str.to_string(),
        mode,
        token_budget: raw_budget,
        requested_token_budget,
        effective_token_budget,
        estimated_tokens: total_estimated_tokens,
        roots,
        nodes,
        edges,
        snippets,
        sections,
        omitted: omitted_candidates,
        diagnostics,
    })
}
