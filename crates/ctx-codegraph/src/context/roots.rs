use super::text::tokenize;
use crate::error::CodeGraphError;
use crate::model::{
    FileId, Language, LanguageObject, LanguageObjectKind, SourceRange, Symbol, SymbolId,
    SymbolKind, TextRange,
};

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
            file: std::path::PathBuf::from(file_path),
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