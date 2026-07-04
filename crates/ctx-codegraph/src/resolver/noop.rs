use crate::model::{ResolutionConfidence, Symbol, SymbolKind};

pub fn parse_raw_name(raw_name: &str) -> &str {
    if let Some(idx) = raw_name.rfind("::") {
        &raw_name[idx + 2..]
    } else if let Some(idx) = raw_name.rfind('.') {
        &raw_name[idx + 1..]
    } else {
        raw_name
    }
}

pub fn resolve_name_only(
    raw_name: &str,
    symbols: &[Symbol],
) -> (Option<usize>, ResolutionConfidence) {
    let target_name = parse_raw_name(raw_name);
    let is_method_call = raw_name.contains('.') && !raw_name.contains("::");

    let candidates: Vec<usize> = symbols
        .iter()
        .enumerate()
        .filter(|(_, sym)| {
            if sym.name != target_name {
                return false;
            }
            if is_method_call {
                sym.kind == SymbolKind::Method
            } else {
                sym.kind == SymbolKind::Function
                    || sym.kind == SymbolKind::Method
                    || sym.kind == SymbolKind::Test
            }
        })
        .map(|(i, _)| i)
        .collect();

    if candidates.len() == 1 {
        (Some(candidates[0]), ResolutionConfidence::NameOnly)
    } else if candidates.len() > 1 {
        (None, ResolutionConfidence::Ambiguous)
    } else {
        (None, ResolutionConfidence::Unresolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Language, TextRange};
    use std::path::PathBuf;

    fn make_test_symbol(name: &str, kind: SymbolKind) -> Symbol {
        Symbol {
            id: None,
            file_id: None,
            name: name.to_string(),
            qualified_name: format!("mod::{}", name),
            kind,
            language: Language::Rust,
            file: PathBuf::from("src/lib.rs"),
            range: TextRange {
                start_line: 1,
                start_col: 1,
                end_line: 2,
                end_col: 1,
            },
            body_range: None,
        }
    }

    #[test]
    fn test_resolves_unique_function_by_name() {
        let symbols = vec![
            make_test_symbol("run_pipeline", SymbolKind::Function),
            make_test_symbol("load", SymbolKind::Function),
        ];

        let (res_idx, res_conf) = resolve_name_only("load", &symbols);
        assert_eq!(res_idx, Some(1));
        assert_eq!(res_conf, ResolutionConfidence::NameOnly);
    }

    #[test]
    fn test_resolves_unique_associated_path_by_suffix() {
        let symbols = vec![
            make_test_symbol("run_pipeline", SymbolKind::Function),
            make_test_symbol("load", SymbolKind::Function),
        ];

        let (res_idx, res_conf) = resolve_name_only("crate::pipeline::load", &symbols);
        assert_eq!(res_idx, Some(1));
        assert_eq!(res_conf, ResolutionConfidence::NameOnly);
    }

    #[test]
    fn test_resolves_method_like_call_by_last_segment() {
        let symbols = vec![
            make_test_symbol("run", SymbolKind::Method),
            make_test_symbol("load", SymbolKind::Method),
        ];

        let (res_idx, res_conf) = resolve_name_only("self.load", &symbols);
        assert_eq!(res_idx, Some(1));
        assert_eq!(res_conf, ResolutionConfidence::NameOnly);

        let (res_idx_2, res_conf_2) = resolve_name_only("pipeline.load", &symbols);
        assert_eq!(res_idx_2, Some(1));
        assert_eq!(res_conf_2, ResolutionConfidence::NameOnly);
    }

    #[test]
    fn test_ambiguous_symbol() {
        let symbols = vec![
            make_test_symbol("load", SymbolKind::Function),
            make_test_symbol("load", SymbolKind::Method),
        ];

        let (res_idx, res_conf) = resolve_name_only("load", &symbols);
        assert_eq!(res_idx, None);
        assert_eq!(res_conf, ResolutionConfidence::Ambiguous);
    }

    #[test]
    fn test_unresolved_symbol() {
        let symbols = vec![make_test_symbol("load", SymbolKind::Function)];

        let (res_idx, res_conf) = resolve_name_only("missing", &symbols);
        assert_eq!(res_idx, None);
        assert_eq!(res_conf, ResolutionConfidence::Unresolved);
    }
}
