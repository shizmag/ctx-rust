use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::CodeGraphError;
use crate::model::{
    ContextFileSpan, GraphContextDiagnostic, GraphContextMode, GraphContextOptions,
    GraphContextResult, GraphEdge, LanguageObject, LanguageObjectKind, SourceRange, SymbolId,
    SymbolResolution,
};

/// Service for graph-based context selection.
/// Acts as the backend logic shared among CLI, TUI, and future MCP server implementations.
pub struct GraphContextService {
    repo_root: PathBuf,
    conn: Mutex<rusqlite::Connection>,
}

impl GraphContextService {
    pub fn new(repo_root: &Path, conn: rusqlite::Connection) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
            conn: Mutex::new(conn),
        }
    }

    pub fn load_or_build(repo_root: &Path) -> Result<Self, CodeGraphError> {
        let workspace_root = crate::storage::find_workspace_root(repo_root);
        let options = crate::index::BuildIndexOptions {
            use_lsp: false,
            max_depth: None,
            include_tests: true,
            change_detection: crate::model::FileChangeDetection::MtimeAndSize,
        };
        if !crate::storage::validate_index_db(&workspace_root, &options)? {
            let _ = crate::storage::rebuild_index_db(&workspace_root, options)?;
        }
        let conn = crate::storage::open_db(&workspace_root)?;
        Ok(Self {
            repo_root: workspace_root,
            conn: Mutex::new(conn),
        })
    }

    pub fn search_symbols(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<LanguageObject>, CodeGraphError> {
        let conn = self.conn.lock().unwrap();
        let candidates = crate::storage::find_symbols(&conn, query)?;
        let mut results = Vec::new();
        for sym in candidates.into_iter().take(limit) {
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

    pub fn resolve_symbol(&self, query: &str) -> Result<SymbolResolution, CodeGraphError> {
        let conn = self.conn.lock().unwrap();
        crate::storage::resolve_symbol(&conn, query)
    }

    pub fn build_context_for_symbol(
        &self,
        symbol_id: SymbolId,
        options: GraphContextOptions,
    ) -> Result<GraphContextResult, CodeGraphError> {
        let conn = self.conn.lock().unwrap();
        let index = crate::storage::load_index(&conn, &self.repo_root)?;

        // Find the root symbol
        let root_sym = index
            .symbols
            .iter()
            .find(|s| s.id == Some(symbol_id))
            .ok_or_else(|| CodeGraphError::SymbolNotFound(format!("{:?}", symbol_id)))?;

        let root = LanguageObject {
            id: root_sym.id.unwrap_or(SymbolId(0)),
            name: root_sym.name.clone(),
            qualified_name: root_sym.qualified_name.clone(),
            kind: LanguageObjectKind::from(root_sym.kind.clone()),
            file_path: root_sym.file.clone(),
            range: SourceRange::from(root_sym.range.clone()),
            signature: None,
            language: Some(root_sym.language.as_str().to_string()),
        };

        let mut visited = HashSet::new();
        let mut seen_edges = HashSet::new();
        let mut seen_diagnostics = HashSet::new();
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut diagnostics = Vec::new();

        let mut queue = VecDeque::new();
        queue.push_back((symbol_id, 0));
        visited.insert(symbol_id);

        while let Some((curr, depth)) = queue.pop_front() {
            let sym = match index.symbols.iter().find(|s| s.id == Some(curr)) {
                Some(s) => s,
                None => continue,
            };

            let is_root = curr == symbol_id;
            let should_include = !is_root || options.include_root;

            if should_include {
                if nodes.len() >= options.max_nodes {
                    let diag_msg = format!(
                        "Context truncated: max_nodes limit ({}) reached.",
                        options.max_nodes
                    );
                    if seen_diagnostics.insert(diag_msg.clone()) {
                        diagnostics.push(GraphContextDiagnostic {
                            severity: "warning".to_string(),
                            message: diag_msg,
                        });
                    }
                    break;
                }

                let obj = LanguageObject {
                    id: curr,
                    name: sym.name.clone(),
                    qualified_name: sym.qualified_name.clone(),
                    kind: LanguageObjectKind::from(sym.kind.clone()),
                    file_path: sym.file.clone(),
                    range: SourceRange::from(sym.range.clone()),
                    signature: None,
                    language: Some(sym.language.as_str().to_string()),
                };
                nodes.push(obj);
            }

            if depth >= options.max_depth {
                continue;
            }

            let traverse_forward = match options.mode {
                GraphContextMode::Callees
                | GraphContextMode::Dependencies
                | GraphContextMode::ForwardSlice
                | GraphContextMode::Neighborhood => true,
                _ => false,
            };

            let traverse_backward = match options.mode {
                GraphContextMode::Callers
                | GraphContextMode::Dependents
                | GraphContextMode::ReverseSlice
                | GraphContextMode::Neighborhood => true,
                _ => false,
            };

            if traverse_forward {
                for edge in &index.edges {
                    if edge.from == curr {
                        if let Some(to_id) = edge.to {
                            let edge_key = (edge.from, to_id, edge.raw_name.clone());
                            if seen_edges.insert(edge_key) {
                                edges.push(GraphEdge {
                                    from: edge.from,
                                    to: to_id,
                                    label: Some(edge.raw_name.clone()),
                                });
                            }

                            if !visited.contains(&to_id) {
                                visited.insert(to_id);
                                queue.push_back((to_id, depth + 1));
                            } else {
                                let diag_msg = format!(
                                    "Cycle or loop detected at symbol: {}",
                                    sym.qualified_name
                                );
                                if seen_diagnostics.insert(diag_msg.clone()) {
                                    diagnostics.push(GraphContextDiagnostic {
                                        severity: "info".to_string(),
                                        message: diag_msg,
                                    });
                                }
                            }
                        }
                    }
                }
            }

            if traverse_backward {
                for edge in &index.edges {
                    if edge.to == Some(curr) {
                        let from_id = edge.from;
                        let edge_key = (from_id, curr, edge.raw_name.clone());
                        if seen_edges.insert(edge_key) {
                            edges.push(GraphEdge {
                                from: from_id,
                                to: curr,
                                label: Some(edge.raw_name.clone()),
                            });
                        }

                        if !visited.contains(&from_id) {
                            visited.insert(from_id);
                            queue.push_back((from_id, depth + 1));
                        } else {
                            let diag_msg =
                                format!("Cycle or loop detected at symbol: {}", sym.qualified_name);
                            if seen_diagnostics.insert(diag_msg.clone()) {
                                diagnostics.push(GraphContextDiagnostic {
                                    severity: "info".to_string(),
                                    message: diag_msg,
                                });
                            }
                        }
                    }
                }
            }
        }

        let mut files = Vec::new();
        let mut seen_spans = HashSet::new();

        let root_span = ContextFileSpan {
            file_path: root.file_path.clone(),
            range: root.range.clone(),
        };
        seen_spans.insert((root_span.file_path.clone(), root_span.range.clone()));
        files.push(root_span);

        for node in &nodes {
            let span = ContextFileSpan {
                file_path: node.file_path.clone(),
                range: node.range.clone(),
            };
            if seen_spans.insert((span.file_path.clone(), span.range.clone())) {
                files.push(span);
            }
        }

        Ok(GraphContextResult {
            root,
            nodes,
            edges,
            files,
            diagnostics,
        })
    }
}
