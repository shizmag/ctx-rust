use ctx_codegraph_lang::model::{
    LanguageId, ResolutionConfidence, Symbol, SymbolKind, TextRange,
};
use ctx_codegraph_lang::noop::resolve_name_only;
use std::path::{Path, PathBuf};

#[test]
fn test_name_only_resolution_and_ambiguity() {
    let symbols = vec![
        Symbol { nesting_depth: 0, lines_of_code: 0, complexity_proxy: 0, param_count: 0, parent_symbol_id: None, fan_in: 0, fan_out: 0, coupling: 0.0, cohesion: 0.0,
            id: None,
            file_id: None,
            name: "foo".to_string(),
            qualified_name: "mod::foo".to_string(),
            kind: SymbolKind::Function,
            language: LanguageId::rust(),
            file: PathBuf::from("src/lib.rs"),
            range: TextRange {
                start_line: 1,
                start_col: 1,
                end_line: 5,
                end_col: 1,
            },
            body_range: None,
        },
        Symbol { nesting_depth: 0, lines_of_code: 0, complexity_proxy: 0, param_count: 0, parent_symbol_id: None, fan_in: 0, fan_out: 0, coupling: 0.0, cohesion: 0.0,
            id: None,
            file_id: None,
            name: "bar".to_string(),
            qualified_name: "mod1::bar".to_string(),
            kind: SymbolKind::Function,
            language: LanguageId::rust(),
            file: PathBuf::from("src/lib.rs"),
            range: TextRange {
                start_line: 6,
                start_col: 1,
                end_line: 10,
                end_col: 1,
            },
            body_range: None,
        },
        Symbol { nesting_depth: 0, lines_of_code: 0, complexity_proxy: 0, param_count: 0, parent_symbol_id: None, fan_in: 0, fan_out: 0, coupling: 0.0, cohesion: 0.0,
            id: None,
            file_id: None,
            name: "bar".to_string(),
            qualified_name: "mod2::bar".to_string(),
            kind: SymbolKind::Function,
            language: LanguageId::rust(),
            file: PathBuf::from("src/lib.rs"),
            range: TextRange {
                start_line: 11,
                start_col: 1,
                end_line: 15,
                end_col: 1,
            },
            body_range: None,
        },
    ];

    let (res_idx, res_conf) =
        resolve_name_only("foo", &symbols, Path::new("src/lib.rs"));
    assert_eq!(res_idx, Some(0));
    assert_eq!(res_conf, ResolutionConfidence::Syntax);

    let (res_idx_ambig, res_conf_ambig) =
        resolve_name_only("bar", &symbols, Path::new("src/lib.rs"));
    assert_eq!(res_idx_ambig, None);
    assert_eq!(res_conf_ambig, ResolutionConfidence::Unresolved);

    let (res_idx_unres, res_conf_unres) =
        resolve_name_only("baz", &symbols, Path::new("src/lib.rs"));
    assert_eq!(res_idx_unres, None);
    assert_eq!(res_conf_unres, ResolutionConfidence::Unresolved);
}