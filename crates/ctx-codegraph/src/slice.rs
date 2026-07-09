use crate::model::{CodeIndex, SymbolId, SymbolKind};
use std::collections::{HashSet, VecDeque};

pub struct SliceOptions {
    pub max_depth: usize,
    pub max_nodes: Option<usize>,
    pub include_tests: bool,
}

pub fn forward_slice(index: &CodeIndex, start: SymbolId, options: SliceOptions) -> Vec<SymbolId> {
    let mut visited = HashSet::new();
    let mut result = Vec::new();
    let mut queue = VecDeque::new();

    queue.push_back((start, 0));
    visited.insert(start);

    while let Some((curr, depth)) = queue.pop_front() {
        if let Some(sym) = index.symbols.iter().find(|s| s.id == Some(curr))
            && !options.include_tests && sym.kind == SymbolKind::Test {
                continue;
            }

        if let Some(limit) = options.max_nodes
            && result.len() >= limit {
                break;
            }

        result.push(curr);

        if depth >= options.max_depth {
            continue;
        }

        for edge in &index.edges {
            if edge.from_symbol_id == Some(curr)
                && let Some(to_id) = edge.to_symbol_id
                    && !visited.contains(&to_id) {
                        visited.insert(to_id);
                        queue.push_back((to_id, depth + 1));
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
        if let Some(sym) = index.symbols.iter().find(|s| s.id == Some(curr))
            && !options.include_tests && sym.kind == SymbolKind::Test {
                continue;
            }

        if let Some(limit) = options.max_nodes
            && result.len() >= limit {
                break;
            }

        result.push(curr);

        if depth >= options.max_depth {
            continue;
        }

        for edge in &index.edges {
            if edge.to_symbol_id == Some(curr)
                && let Some(from_id) = edge.from_symbol_id
                    && !visited.contains(&from_id) {
                        visited.insert(from_id);
                        queue.push_back((from_id, depth + 1));
                    }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CallEdge, LanguageId, ResolutionConfidence, Symbol, TextRange};
    use std::path::PathBuf;

    fn make_test_symbol(id: i64, name: &str, kind: SymbolKind) -> Symbol {
        Symbol {
            id: Some(SymbolId(id)),
            file_id: None,
            name: name.to_string(),
            qualified_name: name.to_string(),
            kind,
            language: LanguageId::rust(),
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
            id: None,
            kind: crate::model::EdgeKind::Call,
            from_file_id: None,
            from_symbol_id: Some(SymbolId(from)),
            to_symbol_id: Some(SymbolId(to)),
            to_external: None,
            occurrence_id: None,
            raw_text: Some("call".to_string()),
            range: Some(TextRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            }),
            confidence: ResolutionConfidence::Heuristic,
            produced_by: None,
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
            occurrences: vec![],
            call_sites: vec![],
            edges: vec![make_test_edge(0, 1), make_test_edge(1, 2)],
        };

        let slice = forward_slice(
            &index,
            SymbolId(0),
            SliceOptions {
                max_depth: 10,
                max_nodes: None,
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
            occurrences: vec![],
            call_sites: vec![],
            edges: vec![make_test_edge(0, 1), make_test_edge(1, 2)],
        };

        let slice = forward_slice(
            &index,
            SymbolId(0),
            SliceOptions {
                max_depth: 1,
                max_nodes: None,
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
            occurrences: vec![],
            call_sites: vec![],
            edges: vec![make_test_edge(0, 2), make_test_edge(1, 2)],
        };

        let slice = reverse_slice(
            &index,
            SymbolId(2),
            SliceOptions {
                max_depth: 10,
                max_nodes: None,
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
            occurrences: vec![],
            call_sites: vec![],
            edges: vec![make_test_edge(0, 1), make_test_edge(1, 0)],
        };

        let slice = forward_slice(
            &index,
            SymbolId(0),
            SliceOptions {
                max_depth: 10,
                max_nodes: None,
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
            occurrences: vec![],
            call_sites: vec![],
            edges: vec![make_test_edge(0, 1)],
        };

        let slice = forward_slice(
            &index,
            SymbolId(0),
            SliceOptions {
                max_depth: 10,
                max_nodes: None,
                include_tests: false,
            },
        );
        // b_test is skipped because include_tests is false
        assert_eq!(slice, vec![SymbolId(0)]);
    }

    #[test]
    fn test_fixture_graph_and_semantics() {
        // fn a() { b(); }
        // fn b() { c(); }
        // fn c() {}
        // fn d() { b(); }
        let index = CodeIndex {
            root: PathBuf::from("."),
            files: vec![],
            symbols: vec![
                make_test_symbol(0, "a", SymbolKind::Function),
                make_test_symbol(1, "b", SymbolKind::Function),
                make_test_symbol(2, "c", SymbolKind::Function),
                make_test_symbol(3, "d", SymbolKind::Function),
            ],
            occurrences: vec![],
            call_sites: vec![],
            edges: vec![
                make_test_edge(0, 1), // a -> b
                make_test_edge(1, 2), // b -> c
                make_test_edge(3, 1), // d -> b
            ],
        };

        // callees(a) содержит b
        let callees_a: Vec<SymbolId> = index
            .edges
            .iter()
            .filter(|e| e.from_symbol_id == Some(SymbolId(0)))
            .filter_map(|e| e.to_symbol_id)
            .collect();
        assert!(callees_a.contains(&SymbolId(1)));

        // callees(b) содержит c
        let callees_b: Vec<SymbolId> = index
            .edges
            .iter()
            .filter(|e| e.from_symbol_id == Some(SymbolId(1)))
            .filter_map(|e| e.to_symbol_id)
            .collect();
        assert!(callees_b.contains(&SymbolId(2)));

        // callers(b) содержит a и d
        let callers_b: Vec<SymbolId> = index
            .edges
            .iter()
            .filter(|e| e.to_symbol_id == Some(SymbolId(1)))
            .filter_map(|e| e.from_symbol_id)
            .collect();
        assert!(callers_b.contains(&SymbolId(0)));
        assert!(callers_b.contains(&SymbolId(3)));

        // forward traversal от a содержит b и c
        let forward = forward_slice(
            &index,
            SymbolId(0),
            SliceOptions {
                max_depth: 10,
                max_nodes: None,
                include_tests: true,
            },
        );
        assert!(forward.contains(&SymbolId(1)));
        assert!(forward.contains(&SymbolId(2)));

        // reverse traversal от c содержит b, a, d
        let reverse = reverse_slice(
            &index,
            SymbolId(2),
            SliceOptions {
                max_depth: 10,
                max_nodes: None,
                include_tests: true,
            },
        );
        assert!(reverse.contains(&SymbolId(1)));
        assert!(reverse.contains(&SymbolId(0)));
        assert!(reverse.contains(&SymbolId(3)));

        // self-cycle или обычный cycle не приводит к бесконечной рекурсии
        let index_cycle = CodeIndex {
            root: PathBuf::from("."),
            files: vec![],
            symbols: vec![
                make_test_symbol(0, "a", SymbolKind::Function),
                make_test_symbol(1, "b", SymbolKind::Function),
            ],
            occurrences: vec![],
            call_sites: vec![],
            edges: vec![
                make_test_edge(0, 0), // self-cycle: a -> a
                make_test_edge(0, 1), // a -> b
                make_test_edge(1, 0), // cycle: b -> a
            ],
        };
        let cycle_forward = forward_slice(
            &index_cycle,
            SymbolId(0),
            SliceOptions {
                max_depth: 10,
                max_nodes: None,
                include_tests: true,
            },
        );
        assert_eq!(cycle_forward.len(), 2); // only a and b

        // max_depth ограничивает обход
        let depth_forward = forward_slice(
            &index,
            SymbolId(0),
            SliceOptions {
                max_depth: 1,
                max_nodes: None,
                include_tests: true,
            },
        );
        assert!(depth_forward.contains(&SymbolId(1)));
        assert!(!depth_forward.contains(&SymbolId(2)));

        // max_nodes ограничивает результат
        let nodes_forward = forward_slice(
            &index,
            SymbolId(0),
            SliceOptions {
                max_depth: 10,
                max_nodes: Some(2),
                include_tests: true,
            },
        );
        assert_eq!(nodes_forward.len(), 2);
    }
}
