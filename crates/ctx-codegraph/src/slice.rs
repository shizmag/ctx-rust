use crate::model::{CodeIndex, SymbolId, SymbolKind};
use std::collections::{HashSet, VecDeque};

pub struct SliceOptions {
    pub max_depth: usize,
    pub include_tests: bool,
}

pub fn forward_slice(index: &CodeIndex, start: SymbolId, options: SliceOptions) -> Vec<SymbolId> {
    let mut visited = HashSet::new();
    let mut result = Vec::new();
    let mut queue = VecDeque::new();

    queue.push_back((start, 0));
    visited.insert(start);

    while let Some((curr, depth)) = queue.pop_front() {
        if let Some(sym) = index.symbols.iter().find(|s| s.id == Some(curr)) {
            if !options.include_tests && sym.kind == SymbolKind::Test {
                continue;
            }
        }

        result.push(curr);

        if depth >= options.max_depth {
            continue;
        }

        for edge in &index.edges {
            if edge.from == curr {
                if let Some(to_id) = edge.to {
                    if !visited.contains(&to_id) {
                        visited.insert(to_id);
                        queue.push_back((to_id, depth + 1));
                    }
                }
            }
        }
    }

    result
}

pub fn reverse_slice(index: &CodeIndex, target: SymbolId, options: SliceOptions) -> Vec<SymbolId> {
    let mut visited = HashSet::new();
    let mut result = Vec::new();
    let mut queue = VecDeque::new();

    queue.push_back((target, 0));
    visited.insert(target);

    while let Some((curr, depth)) = queue.pop_front() {
        if let Some(sym) = index.symbols.iter().find(|s| s.id == Some(curr)) {
            if !options.include_tests && sym.kind == SymbolKind::Test {
                continue;
            }
        }

        result.push(curr);

        if depth >= options.max_depth {
            continue;
        }

        for edge in &index.edges {
            if edge.to == Some(curr) {
                let from_id = edge.from;
                if !visited.contains(&from_id) {
                    visited.insert(from_id);
                    queue.push_back((from_id, depth + 1));
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CallEdge, Language, ResolutionConfidence, Symbol, TextRange};
    use std::path::PathBuf;

    fn make_test_symbol(id: i64, name: &str, kind: SymbolKind) -> Symbol {
        Symbol {
            id: Some(SymbolId(id)),
            file_id: None,
            name: name.to_string(),
            qualified_name: name.to_string(),
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

    fn make_test_edge(from: i64, to: i64) -> CallEdge {
        CallEdge {
            from: SymbolId(from),
            to: Some(SymbolId(to)),
            call_site_id: None,
            raw_name: "call".to_string(),
            call_range: TextRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            },
            confidence: ResolutionConfidence::NameOnly,
        }
    }

    #[test]
    fn test_forward_slice_chain() {
        let index = CodeIndex {
            root: PathBuf::from("."),
            files: vec![],
            symbols: vec![
                make_test_symbol(0, "a", SymbolKind::Function),
                make_test_symbol(1, "b", SymbolKind::Function),
                make_test_symbol(2, "c", SymbolKind::Function),
            ],
            call_sites: vec![],
            edges: vec![make_test_edge(0, 1), make_test_edge(1, 2)],
        };

        let slice = forward_slice(
            &index,
            SymbolId(0),
            SliceOptions {
                max_depth: 10,
                include_tests: true,
            },
        );
        // Stable BFS order: a, then its callee b, then b's callee c
        assert_eq!(slice, vec![SymbolId(0), SymbolId(1), SymbolId(2)]);
    }

    #[test]
    fn test_forward_slice_respects_max_depth() {
        let index = CodeIndex {
            root: PathBuf::from("."),
            files: vec![],
            symbols: vec![
                make_test_symbol(0, "a", SymbolKind::Function),
                make_test_symbol(1, "b", SymbolKind::Function),
                make_test_symbol(2, "c", SymbolKind::Function),
            ],
            call_sites: vec![],
            edges: vec![make_test_edge(0, 1), make_test_edge(1, 2)],
        };

        let slice = forward_slice(
            &index,
            SymbolId(0),
            SliceOptions {
                max_depth: 1,
                include_tests: true,
            },
        );
        // BFS order, up to depth 1: a (depth 0), b (depth 1)
        assert_eq!(slice, vec![SymbolId(0), SymbolId(1)]);
    }

    #[test]
    fn test_reverse_slice() {
        let index = CodeIndex {
            root: PathBuf::from("."),
            files: vec![],
            symbols: vec![
                make_test_symbol(0, "a", SymbolKind::Function),
                make_test_symbol(1, "b", SymbolKind::Function),
                make_test_symbol(2, "c", SymbolKind::Function),
            ],
            call_sites: vec![],
            edges: vec![make_test_edge(0, 2), make_test_edge(1, 2)],
        };

        let slice = reverse_slice(
            &index,
            SymbolId(2),
            SliceOptions {
                max_depth: 10,
                include_tests: true,
            },
        );
        // Target c (depth 0), then callers a and b
        assert_eq!(slice.len(), 3);
        assert_eq!(slice[0], SymbolId(2));
        assert!(slice.contains(&SymbolId(0)));
        assert!(slice.contains(&SymbolId(1)));
    }

    #[test]
    fn test_handles_cycles() {
        let index = CodeIndex {
            root: PathBuf::from("."),
            files: vec![],
            symbols: vec![
                make_test_symbol(0, "a", SymbolKind::Function),
                make_test_symbol(1, "b", SymbolKind::Function),
            ],
            call_sites: vec![],
            edges: vec![make_test_edge(0, 1), make_test_edge(1, 0)],
        };

        let slice = forward_slice(
            &index,
            SymbolId(0),
            SliceOptions {
                max_depth: 10,
                include_tests: true,
            },
        );
        // Should terminate, containing a and b exactly once
        assert_eq!(slice, vec![SymbolId(0), SymbolId(1)]);
    }

    #[test]
    fn test_can_exclude_tests() {
        let index = CodeIndex {
            root: PathBuf::from("."),
            files: vec![],
            symbols: vec![
                make_test_symbol(0, "a", SymbolKind::Function),
                make_test_symbol(1, "b_test", SymbolKind::Test),
            ],
            call_sites: vec![],
            edges: vec![make_test_edge(0, 1)],
        };

        let slice = forward_slice(
            &index,
            SymbolId(0),
            SliceOptions {
                max_depth: 10,
                include_tests: false,
            },
        );
        // b_test is skipped because include_tests is false
        assert_eq!(slice, vec![SymbolId(0)]);
    }
}
