use crate::search::collect_matching_files;
use ctx_models::{NodeKind, TreeNode, get_relative_path};
use ratatui::widgets::ListState;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TuiScreen {
    TreePicker,
    SymbolSearch,
    GraphContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusedPanel {
    Left,
    Right,
}

pub(crate) struct TuiApp {
    pub(crate) path: PathBuf,
    pub(crate) scan_result: ctx_models::ScanResult,
    pub(crate) test_ctx: ctx_test::TestContext,
    pub(crate) expanded_dirs: HashSet<PathBuf>,
    pub(crate) checked_paths: HashSet<PathBuf>,
    pub(crate) visible_items: Vec<VisibleTuiNode>,
    pub(crate) list_state: ListState,
    pub(crate) message: Option<(String, std::time::Instant)>,
    pub(crate) search_active: bool,
    pub(crate) search_query: String,

    // Symbol Search and TUI screens
    pub(crate) screen: TuiScreen,
    pub(crate) focused_panel: FocusedPanel,
    pub(crate) symbol_search_query: String,
    pub(crate) symbol_search_results: Vec<ctx_codegraph::LanguageObject>,
    pub(crate) symbol_list_state: ListState,
    pub(crate) symbol_search_active: bool,
    pub(crate) selected_symbol: Option<ctx_codegraph::LanguageObject>,
    pub(crate) preview_scroll_offset: usize,
    pub(crate) graph_service: Option<ctx_codegraph::GraphContextService>,
    pub(crate) last_preview_total_lines: usize,
    pub(crate) last_preview_height: usize,

    // Graph Context Flow State
    pub(crate) graph_mode: ctx_codegraph::GraphContextMode,
    pub(crate) graph_depth: usize,
    pub(crate) graph_max_nodes: usize,
    pub(crate) graph_include_root: bool,
    pub(crate) graph_selected_option: usize, // 0: mode, 1: depth, 2: max_nodes, 3: include_root
    pub(crate) graph_preview: Option<Result<ctx_codegraph::GraphContextResult, String>>,
}

pub(crate) struct VisibleTuiNode {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) kind: NodeKind,
    pub(crate) is_expanded: bool,
    pub(crate) checked: bool,
    pub(crate) lines: usize,
    pub(crate) tokens: usize,
    pub(crate) bytes: u64,
    pub(crate) tests: usize,
    pub(crate) covered_lines: usize,
    pub(crate) coverable_lines: usize,
    pub(crate) tree_line_prefix: String,
    pub(crate) is_text: bool,
}

impl TuiApp {
    pub(crate) fn new(path: PathBuf) -> Result<Self, crate::error::TuiError> {
        let scan_result = ctx_core::scan(&path, ctx_models::ScanOptions::default())?;
        let test_ctx = ctx_test::TestContext::discover(&path);

        let mut expanded_dirs = HashSet::new();
        let checked_paths = HashSet::new();

        expanded_dirs.insert(scan_result.root.path.clone());

        let graph_service = ctx_codegraph::GraphContextService::load_or_build(&path).ok();

        let mut app = Self {
            path,
            scan_result,
            test_ctx,
            expanded_dirs,
            checked_paths,
            visible_items: Vec::new(),
            list_state: ListState::default(),
            message: None,
            search_active: false,
            search_query: String::new(),
            screen: TuiScreen::TreePicker,
            focused_panel: FocusedPanel::Left,
            symbol_search_query: String::new(),
            symbol_search_results: Vec::new(),
            symbol_list_state: ListState::default(),
            symbol_search_active: false,
            selected_symbol: None,
            preview_scroll_offset: 0,
            graph_service,
            last_preview_total_lines: 0,
            last_preview_height: 0,
            graph_mode: ctx_codegraph::GraphContextMode::Callers,
            graph_depth: 2,
            graph_max_nodes: 50,
            graph_include_root: true,
            graph_selected_option: 0,
            graph_preview: None,
        };

        app.update_visible_items();

        if !app.visible_items.is_empty() {
            app.list_state.select(Some(0));
        }

        Ok(app)
    }

    pub(crate) fn update_visible_items(&mut self) {
        if !self.search_query.is_empty() {
            let mut matches = Vec::new();
            collect_matching_files(&self.scan_result.root, &self.search_query, &mut matches);

            self.visible_items = matches
                .into_iter()
                .map(|node| {
                    let checked = self.checked_paths.contains(&node.path);
                    let rel_path = get_relative_path(&node.path, &self.path);
                    VisibleTuiNode {
                        path: node.path.clone(),
                        name: rel_path,
                        kind: node.kind,
                        is_expanded: false,
                        checked,
                        lines: node.stats.lines,
                        tokens: node.stats.tokens,
                        bytes: node.stats.bytes,
                        tests: node.stats.tests,
                        covered_lines: node.stats.covered_lines,
                        coverable_lines: node.stats.coverable_lines,
                        tree_line_prefix: String::new(),
                        is_text: node.stats.lines > 0 || node.stats.bytes == 0,
                    }
                })
                .collect();
        } else {
            let mut visible = Vec::new();
            traverse_build_visible(
                &self.scan_result.root,
                &self.expanded_dirs,
                &self.checked_paths,
                &mut visible,
            );
            self.visible_items = visible;
        }
    }

    pub(crate) fn rescan(&mut self) -> Result<(), crate::error::TuiError> {
        let scan_result = ctx_core::scan(&self.path, ctx_models::ScanOptions::default())?;
        self.scan_result = scan_result;

        let mut new_expanded = HashSet::new();
        let mut new_checked = HashSet::new();

        merge_tree_states(
            &self.scan_result.root,
            &self.expanded_dirs,
            &self.checked_paths,
            &mut new_expanded,
            &mut new_checked,
        );

        self.expanded_dirs = new_expanded;
        self.checked_paths = new_checked;

        self.update_visible_items();

        let selected = self.list_state.selected().unwrap_or(0);
        if self.visible_items.is_empty() {
            self.list_state.select(None);
        } else if selected >= self.visible_items.len() {
            self.list_state.select(Some(self.visible_items.len() - 1));
        } else {
            self.list_state.select(Some(selected));
        }

        self.message = Some((
            "Rescanned directory!".to_string(),
            std::time::Instant::now(),
        ));
        Ok(())
    }

    pub(crate) fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.update_visible_items();

        let selected = self.list_state.selected().unwrap_or(0);
        if self.visible_items.is_empty() {
            self.list_state.select(None);
        } else if selected >= self.visible_items.len() {
            self.list_state.select(Some(self.visible_items.len() - 1));
        } else {
            self.list_state.select(Some(selected));
        }
    }

    pub(crate) fn set_symbol_search_query(&mut self, query: String) {
        self.symbol_search_query = query;
        self.update_symbol_search();
    }

    pub(crate) fn update_symbol_search(&mut self) {
        if let Some(ref service) = self.graph_service {
            if !self.symbol_search_query.is_empty() {
                if let Ok(results) = service.search_symbols(&self.symbol_search_query, 100) {
                    self.symbol_search_results = results;
                } else {
                    self.symbol_search_results = Vec::new();
                }
            } else {
                self.symbol_search_results = Vec::new();
            }
        } else {
            self.symbol_search_results = Vec::new();
        }

        let selected = self.symbol_list_state.selected().unwrap_or(0);
        if self.symbol_search_results.is_empty() {
            self.symbol_list_state.select(None);
        } else if selected >= self.symbol_search_results.len() {
            self.symbol_list_state
                .select(Some(self.symbol_search_results.len() - 1));
        } else {
            self.symbol_list_state.select(Some(selected));
        }
        self.preview_scroll_offset = 0;
    }

    pub(crate) fn preview_scroll_up(&mut self) {
        self.preview_scroll_offset = self.preview_scroll_offset.saturating_sub(1);
    }

    pub(crate) fn preview_scroll_down(&mut self, total_lines: usize, visible_height: usize) {
        let max_offset = total_lines.saturating_sub(visible_height);
        if self.preview_scroll_offset < max_offset {
            self.preview_scroll_offset += 1;
        }
    }

    pub(crate) fn preview_page_up(&mut self, step: usize) {
        self.preview_scroll_offset = self.preview_scroll_offset.saturating_sub(step);
    }

    pub(crate) fn preview_page_down(
        &mut self,
        step: usize,
        total_lines: usize,
        visible_height: usize,
    ) {
        let max_offset = total_lines.saturating_sub(visible_height);
        self.preview_scroll_offset = std::cmp::min(self.preview_scroll_offset + step, max_offset);
    }

    pub(crate) fn update_graph_preview(&mut self) {
        let symbol = match &self.selected_symbol {
            Some(sym) => sym,
            None => {
                self.graph_preview = None;
                return;
            }
        };
        let service = match &self.graph_service {
            Some(srv) => srv,
            None => {
                self.graph_preview = Some(Err("Graph database not initialized".to_string()));
                return;
            }
        };
        let options = ctx_codegraph::GraphContextOptions {
            mode: self.graph_mode,
            max_depth: self.graph_depth,
            max_nodes: self.graph_max_nodes,
            include_root: self.graph_include_root,
        };
        match service.build_context_for_symbol(symbol.id, options) {
            Ok(res) => {
                self.graph_preview = Some(Ok(res));
            }
            Err(e) => {
                self.graph_preview = Some(Err(format!("Error building graph context: {}", e)));
            }
        }
    }

    pub(crate) fn render_tui_graph_preview(&self) -> Result<String, String> {
        let result = match &self.graph_preview {
            Some(Ok(res)) => res,
            Some(Err(err)) => return Err(err.clone()),
            None => return Err("No preview generated".to_string()),
        };

        let mut out = String::new();
        let root_kind = match result.root.kind {
            ctx_codegraph::LanguageObjectKind::Function => "fn",
            ctx_codegraph::LanguageObjectKind::Method => "fn",
            ctx_codegraph::LanguageObjectKind::Struct => "struct",
            ctx_codegraph::LanguageObjectKind::Enum => "enum",
            ctx_codegraph::LanguageObjectKind::Trait => "trait",
            ctx_codegraph::LanguageObjectKind::Impl => "impl",
            ctx_codegraph::LanguageObjectKind::Module => "mod",
            _ => "symbol",
        };
        let root_rel_path = result
            .root
            .file_path
            .strip_prefix(&self.path)
            .unwrap_or(&result.root.file_path);

        out.push_str("Root:\n");
        out.push_str(&format!(
            "  {} {} at {}:{}\n\n",
            root_kind,
            result.root.name,
            root_rel_path.display(),
            result.root.range.start_line
        ));

        let mode_str = match self.graph_mode {
            ctx_codegraph::GraphContextMode::Callers => "Callers",
            ctx_codegraph::GraphContextMode::Callees => "Callees",
            ctx_codegraph::GraphContextMode::Dependencies => "Dependencies",
            ctx_codegraph::GraphContextMode::Dependents => "Dependents",
            ctx_codegraph::GraphContextMode::ForwardSlice => "Forward slice",
            ctx_codegraph::GraphContextMode::ReverseSlice => "Reverse slice",
            ctx_codegraph::GraphContextMode::Forward => "Forward",
            ctx_codegraph::GraphContextMode::Reverse => "Reverse",
            ctx_codegraph::GraphContextMode::Neighborhood => "Neighborhood",
            ctx_codegraph::GraphContextMode::Impact => "Impact",
        };
        out.push_str("Mode:\n");
        out.push_str(&format!("  {}, depth {}\n\n", mode_str, self.graph_depth));

        let symbols_count = result.nodes.len();
        let files_count = result.files.len();
        let mut lines_count = 0;
        for file_span in &result.files {
            if let Ok(content) = std::fs::read_to_string(&file_span.file_path) {
                let lines: Vec<&str> = content.lines().collect();
                let end = std::cmp::min(file_span.range.end_line, lines.len());
                if file_span.range.start_line > 0 && file_span.range.start_line <= end {
                    lines_count += end - file_span.range.start_line + 1;
                }
            }
        }
        out.push_str("Included:\n");
        out.push_str(&format!("  {} symbols\n", symbols_count));
        out.push_str(&format!("  {} files\n", files_count));
        out.push_str(&format!("  {} lines\n", lines_count));

        Ok(out)
    }
}

pub(crate) fn traverse_build_visible(
    node: &TreeNode,
    expanded_dirs: &HashSet<PathBuf>,
    checked_paths: &HashSet<PathBuf>,
    visible: &mut Vec<VisibleTuiNode>,
) {
    ctx_models::walk_tree_lines(node, |line| {
        let is_dir = line.node.kind == NodeKind::Directory;
        let checked = checked_paths.contains(&line.node.path);
        let is_expanded = expanded_dirs.contains(&line.node.path);

        visible.push(VisibleTuiNode {
            path: line.node.path.clone(),
            name: line.node.name.clone(),
            kind: line.node.kind,
            is_expanded,
            checked,
            lines: line.node.stats.lines,
            tokens: line.node.stats.tokens,
            bytes: line.node.stats.bytes,
            tests: line.node.stats.tests,
            covered_lines: line.node.stats.covered_lines,
            coverable_lines: line.node.stats.coverable_lines,
            tree_line_prefix: line.prefix.clone(),
            is_text: line.node.stats.lines > 0 || line.node.stats.bytes == 0,
        });

        is_dir && is_expanded
    });
}

pub(crate) fn set_checked_recursive(
    node: &TreeNode,
    checked: bool,
    checked_paths: &mut HashSet<PathBuf>,
) {
    if checked {
        checked_paths.insert(node.path.clone());
    } else {
        checked_paths.remove(&node.path);
    }
    for child in &node.children {
        set_checked_recursive(child, checked, checked_paths);
    }
}

pub(crate) fn find_node<'a>(node: &'a TreeNode, path: &Path) -> Option<&'a TreeNode> {
    if node.path == path {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_node(child, path) {
            return Some(found);
        }
    }
    None
}

pub(crate) fn merge_tree_states(
    node: &TreeNode,
    old_expanded: &HashSet<PathBuf>,
    old_checked: &HashSet<PathBuf>,
    new_expanded: &mut HashSet<PathBuf>,
    new_checked: &mut HashSet<PathBuf>,
) {
    if node.kind == NodeKind::Directory && old_expanded.contains(&node.path) {
        new_expanded.insert(node.path.clone());
    }

    if old_checked.contains(&node.path) {
        new_checked.insert(node.path.clone());
    }

    for child in &node.children {
        merge_tree_states(child, old_expanded, old_checked, new_expanded, new_checked);
    }
}

pub(crate) fn collect_checked_files<'a>(
    node: &'a TreeNode,
    checked_paths: &HashSet<PathBuf>,
    files: &mut Vec<&'a TreeNode>,
) {
    if node.kind == NodeKind::File && checked_paths.contains(&node.path) {
        files.push(node);
    }
    for child in &node.children {
        collect_checked_files(child, checked_paths, files);
    }
}

pub(crate) fn count_all_files(node: &TreeNode) -> usize {
    if node.kind == NodeKind::File {
        1
    } else {
        node.children.iter().map(count_all_files).sum()
    }
}

pub(crate) fn sum_all_tokens(node: &TreeNode) -> usize {
    if node.kind == NodeKind::File {
        node.stats.tokens
    } else {
        node.children.iter().map(sum_all_tokens).sum()
    }
}

pub(crate) fn sum_all_bytes(node: &TreeNode) -> u64 {
    if node.kind == NodeKind::File {
        node.stats.bytes
    } else {
        node.children.iter().map(sum_all_bytes).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_codegraph::{BuildIndexOptions, IndexState, model::FileChangeDetection};
    use std::fs;
    use std::time::Duration;

    fn temp_tui_project_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ctx_tui_{}_{}",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn max_indexed_at_ms(root: &Path) -> Option<i64> {
        let conn = ctx_codegraph::open_db(root).ok()?;
        conn.query_row("SELECT MAX(indexed_at_ms) FROM files", [], |row| row.get(0))
            .ok()
    }

    fn default_index_options() -> BuildIndexOptions {
        BuildIndexOptions::default()
    }

    /// `ctx -i` initializes `TuiApp`, which calls `GraphContextService::load_or_build`.
    /// On a warm index with unchanged files, startup must not re-parse indexed files.
    #[test]
    fn test_tui_startup_skips_codegraph_rebuild_on_warm_cache() {
        let temp_dir = temp_tui_project_dir("warm_cache");
        fs::create_dir_all(&temp_dir).unwrap();
        fs::create_dir_all(temp_dir.join(".git")).unwrap();
        fs::write(temp_dir.join("lib.rs"), "fn cached_symbol() {}\n").unwrap();

        let options = default_index_options();
        let db_path = temp_dir.join(".ctx-codegraph/codegraph.sqlite");

        assert!(!db_path.exists(), "test project should start without an index");

        let app = TuiApp::new(temp_dir.clone()).unwrap();
        assert!(
            app.graph_service.is_some(),
            "interactive startup should initialize graph service"
        );
        assert!(db_path.exists(), "first interactive startup should build the index");

        let indexed_after_first = max_indexed_at_ms(&temp_dir)
            .expect("indexed files should have timestamps after first startup");
        assert!(
            ctx_codegraph::validate_index_db(&temp_dir, &options).unwrap(),
            "index should be valid after first startup"
        );

        std::thread::sleep(Duration::from_millis(10));

        let app2 = TuiApp::new(temp_dir.clone()).unwrap();
        assert!(app2.graph_service.is_some());

        let indexed_after_second = max_indexed_at_ms(&temp_dir).expect("index timestamps should remain readable");
        assert_eq!(
            indexed_after_first, indexed_after_second,
            "second ctx -i startup should not re-index unchanged files"
        );
        assert!(matches!(
            ctx_codegraph::get_index_state(&temp_dir, &options).unwrap(),
            IndexState::Ready
        ));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_tui_startup_reuses_prebuilt_codegraph_index() {
        let temp_dir = temp_tui_project_dir("prebuilt");
        fs::create_dir_all(&temp_dir).unwrap();
        fs::create_dir_all(temp_dir.join(".git")).unwrap();
        fs::write(temp_dir.join("lib.rs"), "fn prebuilt_symbol() {}\n").unwrap();

        let options = default_index_options();
        ctx_codegraph::rebuild_index_db(&temp_dir, options.clone()).unwrap();

        let indexed_before = max_indexed_at_ms(&temp_dir)
            .expect("prebuilt index should contain indexed files");
        assert!(ctx_codegraph::validate_index_db(&temp_dir, &options).unwrap());

        std::thread::sleep(Duration::from_millis(10));

        let app = TuiApp::new(temp_dir.clone()).unwrap();
        assert!(app.graph_service.is_some());

        let indexed_after = max_indexed_at_ms(&temp_dir).unwrap();
        assert_eq!(
            indexed_before, indexed_after,
            "ctx -i should reuse a prebuilt index without reparsing files"
        );
        assert!(matches!(
            ctx_codegraph::get_index_state(&temp_dir, &options).unwrap(),
            IndexState::Ready
        ));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_symbol_search_and_preview_logic() {
        let temp_dir = std::env::temp_dir().join(format!(
            "ctx_tui_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        // 1. Create a dummy rust file
        let file_path = temp_dir.join("lib.rs");
        fs::write(
            &file_path,
            "fn my_awesome_test_function() {\n    let x = 1;\n    let y = 2;\n}\n",
        )
        .unwrap();

        // 2. Build codegraph index so search works
        let options = ctx_codegraph::BuildIndexOptions::default();
        ctx_codegraph::rebuild_index_db(&temp_dir, options).unwrap();

        // 3. Initialize TuiApp
        let mut app = TuiApp::new(temp_dir.clone()).unwrap();
        assert!(app.graph_service.is_some());

        // Test 1: Search query updates candidates
        app.screen = TuiScreen::SymbolSearch;
        app.set_symbol_search_query("my_awesome".to_string());

        assert!(
            !app.symbol_search_results.is_empty(),
            "Candidates list should be updated and not empty"
        );
        let found = &app.symbol_search_results[0];
        assert_eq!(found.name, "my_awesome_test_function");
        assert_eq!(app.symbol_list_state.selected(), Some(0));

        // Test 2: Selection saves selected symbol (LanguageObject)
        app.selected_symbol = Some(found.clone());
        assert_eq!(
            app.selected_symbol.as_ref().unwrap().name,
            "my_awesome_test_function"
        );

        // Test 3: File preview scroll down/up
        app.preview_scroll_offset = 0;
        app.preview_scroll_down(100, 10);
        assert_eq!(app.preview_scroll_offset, 1);

        app.preview_scroll_up();
        assert_eq!(app.preview_scroll_offset, 0);

        app.preview_page_down(5, 100, 10);
        assert_eq!(app.preview_scroll_offset, 5);

        app.preview_page_up(2);
        assert_eq!(app.preview_scroll_offset, 3);

        // Test 4: Selected symbol remains associated with preview
        // Even after scrolling or query changes, we check if the selected symbol is retained
        app.set_symbol_search_query("another_query".to_string());
        assert!(app.symbol_search_results.is_empty());
        assert_eq!(
            app.selected_symbol.as_ref().unwrap().name,
            "my_awesome_test_function"
        );

        // Clean up
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_graph_context_flow_integration() {
        let temp_dir = std::env::temp_dir().join(format!(
            "ctx_tui_test_graph_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        // Create .git marker so find_workspace_root stops here
        fs::create_dir_all(temp_dir.join(".git")).unwrap();

        // 1. Create source file with 3 functions (lines must match symbol ranges below)
        let file_path = temp_dir.join("lib.rs");
        fs::write(
            &file_path,
            "fn a() {\n    b();\n}\nfn b() {\n    c();\n}\nfn c() {\n}\n",
        )
        .unwrap();

        // 2. Initialize TuiApp first (load_or_build may rebuild the index from tree-sitter
        //    which produces symbols but no call edges). We'll replace graph_service below.
        let mut app = TuiApp::new(temp_dir.clone()).unwrap();

        // 3. Manually populate the codegraph SQLite DB with mock symbols AND call edges.
        //    Clear whatever load_or_build wrote and insert our test fixture data.
        {
            let mut conn = ctx_codegraph::open_db(&temp_dir).unwrap();
            ctx_codegraph::storage::clear_index(&mut conn).unwrap();

            let mut index = ctx_codegraph::CodeIndex {
                root: temp_dir.clone(),
                files: vec![ctx_codegraph::FileSnapshot {
                    file_id: None,
                    rel_path: std::path::PathBuf::from("lib.rs"),
                    abs_path: temp_dir.join("lib.rs"),
                    language: ctx_codegraph::Language::rust(),
                    backend_id: ctx_codegraph::BackendId::new("rust-backend"),
                    size_bytes: 200,
                    mtime_ms: 100,
                    mtime_ns: None,
                    content_hash: Some("hash_test".to_string()),
                    parser_id: ctx_codegraph::ParserId::new("tree-sitter-rust"),
                    parser_version: "0.20.0".to_string(),
                    parser_config_hash: "".to_string(),
                    indexed_at_ms: None,
                    parse_status: ctx_codegraph::FileParseStatus::Success,
                }],
                symbols: vec![
                    ctx_codegraph::Symbol {
                        id: Some(ctx_codegraph::SymbolId(0)),
                        file_id: None,
                        name: "a".to_string(),
                        qualified_name: "a".to_string(),
                        kind: ctx_codegraph::SymbolKind::Function,
                        language: ctx_codegraph::Language::rust(),
                        file: std::path::PathBuf::from("lib.rs"),
                        range: ctx_codegraph::TextRange {
                            start_line: 1,
                            start_col: 1,
                            end_line: 3,
                            end_col: 1,
                        },
                        body_range: None,
                    },
                    ctx_codegraph::Symbol {
                        id: Some(ctx_codegraph::SymbolId(1)),
                        file_id: None,
                        name: "b".to_string(),
                        qualified_name: "b".to_string(),
                        kind: ctx_codegraph::SymbolKind::Function,
                        language: ctx_codegraph::Language::rust(),
                        file: std::path::PathBuf::from("lib.rs"),
                        range: ctx_codegraph::TextRange {
                            start_line: 4,
                            start_col: 1,
                            end_line: 6,
                            end_col: 1,
                        },
                        body_range: None,
                    },
                    ctx_codegraph::Symbol {
                        id: Some(ctx_codegraph::SymbolId(2)),
                        file_id: None,
                        name: "c".to_string(),
                        qualified_name: "c".to_string(),
                        kind: ctx_codegraph::SymbolKind::Function,
                        language: ctx_codegraph::Language::rust(),
                        file: std::path::PathBuf::from("lib.rs"),
                        range: ctx_codegraph::TextRange {
                            start_line: 7,
                            start_col: 1,
                            end_line: 8,
                            end_col: 1,
                        },
                        body_range: None,
                    },
                ],
                occurrences: vec![
                    ctx_codegraph::model::Occurrence {
                        id: Some(ctx_codegraph::model::OccurrenceId(0)),
                        file_id: None,
                        enclosing_symbol: Some(ctx_codegraph::SymbolId(0)),
                        enclosing_temp_index: None,
                        kind: ctx_codegraph::model::OccurrenceKind::Call,
                        raw_text: "b".to_string(),
                        file: std::path::PathBuf::from("lib.rs"),
                        range: ctx_codegraph::TextRange {
                            start_line: 2,
                            start_col: 5,
                            end_line: 2,
                            end_col: 8,
                        },
                        language: ctx_codegraph::model::LanguageId::rust(),
                        backend_id: ctx_codegraph::BackendId::new("rust-backend"),
                    },
                    ctx_codegraph::model::Occurrence {
                        id: Some(ctx_codegraph::model::OccurrenceId(1)),
                        file_id: None,
                        enclosing_symbol: Some(ctx_codegraph::SymbolId(1)),
                        enclosing_temp_index: None,
                        kind: ctx_codegraph::model::OccurrenceKind::Call,
                        raw_text: "c".to_string(),
                        file: std::path::PathBuf::from("lib.rs"),
                        range: ctx_codegraph::TextRange {
                            start_line: 5,
                            start_col: 5,
                            end_line: 5,
                            end_col: 8,
                        },
                        language: ctx_codegraph::model::LanguageId::rust(),
                        backend_id: ctx_codegraph::BackendId::new("rust-backend"),
                    },
                ],
                call_sites: vec![
                    ctx_codegraph::model::Occurrence {
                        id: Some(ctx_codegraph::model::OccurrenceId(0)),
                        file_id: None,
                        enclosing_symbol: Some(ctx_codegraph::SymbolId(0)),
                        enclosing_temp_index: None,
                        kind: ctx_codegraph::model::OccurrenceKind::Call,
                        raw_text: "b".to_string(),
                        file: std::path::PathBuf::from("lib.rs"),
                        range: ctx_codegraph::TextRange {
                            start_line: 2,
                            start_col: 5,
                            end_line: 2,
                            end_col: 8,
                        },
                        language: ctx_codegraph::model::LanguageId::rust(),
                        backend_id: ctx_codegraph::BackendId::new("rust-backend"),
                    },
                    ctx_codegraph::model::Occurrence {
                        id: Some(ctx_codegraph::model::OccurrenceId(1)),
                        file_id: None,
                        enclosing_symbol: Some(ctx_codegraph::SymbolId(1)),
                        enclosing_temp_index: None,
                        kind: ctx_codegraph::model::OccurrenceKind::Call,
                        raw_text: "c".to_string(),
                        file: std::path::PathBuf::from("lib.rs"),
                        range: ctx_codegraph::TextRange {
                            start_line: 5,
                            start_col: 5,
                            end_line: 5,
                            end_col: 8,
                        },
                        language: ctx_codegraph::model::LanguageId::rust(),
                        backend_id: ctx_codegraph::BackendId::new("rust-backend"),
                    },
                ],
                edges: vec![
                    ctx_codegraph::model::CallEdge {
                        id: None,
                        kind: ctx_codegraph::model::EdgeKind::Call,
                        from_file_id: None,
                        from_symbol_id: Some(ctx_codegraph::SymbolId(0)),
                        to_symbol_id: Some(ctx_codegraph::SymbolId(1)),
                        to_external: None,
                        occurrence_id: Some(ctx_codegraph::model::OccurrenceId(0)),
                        raw_text: Some("b".to_string()),
                        range: Some(ctx_codegraph::TextRange {
                            start_line: 2,
                            start_col: 5,
                            end_line: 2,
                            end_col: 8,
                        }),
                        confidence: ctx_codegraph::ResolutionConfidence::LspExact,
                        produced_by: None,
                    },
                    ctx_codegraph::model::CallEdge {
                        id: None,
                        kind: ctx_codegraph::model::EdgeKind::Call,
                        from_file_id: None,
                        from_symbol_id: Some(ctx_codegraph::SymbolId(1)),
                        to_symbol_id: Some(ctx_codegraph::SymbolId(2)),
                        to_external: None,
                        occurrence_id: Some(ctx_codegraph::model::OccurrenceId(1)),
                        raw_text: Some("c".to_string()),
                        range: Some(ctx_codegraph::TextRange {
                            start_line: 5,
                            start_col: 5,
                            end_line: 5,
                            end_col: 8,
                        }),
                        confidence: ctx_codegraph::ResolutionConfidence::LspExact,
                        produced_by: None,
                    },
                ],
            };
            ctx_codegraph::storage::save_index(&mut conn, &mut index).unwrap();
        }

        // 4. Inject a fresh GraphContextService that reads our mock data
        let conn = ctx_codegraph::open_db(&temp_dir).unwrap();
        app.graph_service = Some(ctx_codegraph::GraphContextService::new(&temp_dir, conn));

        // 5. Search and select symbol 'b' via the service
        let results = app
            .graph_service
            .as_ref()
            .unwrap()
            .search_symbols("b", 10)
            .unwrap();
        assert!(!results.is_empty());
        let sym_b = results.iter().find(|s| s.name == "b").unwrap().clone();
        app.selected_symbol = Some(sym_b.clone());

        // 5. Configure GraphContext flow state
        app.screen = TuiScreen::GraphContext;
        app.graph_mode = ctx_codegraph::GraphContextMode::Callers;
        app.graph_depth = 2;
        app.graph_max_nodes = 50;
        app.graph_include_root = true;

        // 6. Test: selected symbol + mode + depth produce a preview state
        app.update_graph_preview();
        assert!(app.graph_preview.is_some());
        let preview_result = app.graph_preview.as_ref().unwrap();
        assert!(
            preview_result.is_ok(),
            "Expected Ok, got: {:?}",
            preview_result
        );

        // 7. Test: preview shows root, mode, and counts
        let preview_text = app.render_tui_graph_preview().unwrap();
        assert!(
            preview_text.contains("Root:"),
            "missing Root in: {}",
            preview_text
        );
        assert!(
            preview_text.contains("fn b at lib.rs:4"),
            "missing 'fn b at lib.rs:4' in: {}",
            preview_text
        );
        assert!(
            preview_text.contains("Mode:\n  Callers, depth 2"),
            "missing mode in: {}",
            preview_text
        );
        assert!(preview_text.contains("Included:"));
        assert!(preview_text.contains("symbols"));
        assert!(preview_text.contains("files"));
        assert!(preview_text.contains("lines"));

        // 8. Test: rendered context for clipboard contains graph edges
        let rendered_ctx = crate::clipboard::render_graph_context_output(
            preview_result.as_ref().unwrap(),
            &app.path,
            app.graph_mode,
            app.graph_depth,
            app.graph_max_nodes,
        )
        .unwrap();
        assert!(rendered_ctx.contains("# Graph Context"));
        assert!(rendered_ctx.contains("Root: fn b"));
        assert!(rendered_ctx.contains("Mode: Callers"));
        assert!(rendered_ctx.contains("## Graph"));
        assert!(
            rendered_ctx.contains("a -> b"),
            "missing 'a -> b' in: {}",
            rendered_ctx
        );

        // 9. Test: nonexistent symbol shows error state
        app.selected_symbol = Some(ctx_codegraph::LanguageObject {
            id: ctx_codegraph::SymbolId(99999),
            name: "nonexistent".to_string(),
            qualified_name: "nonexistent".to_string(),
            kind: ctx_codegraph::LanguageObjectKind::Function,
            file_path: file_path.clone(),
            range: ctx_codegraph::SourceRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            },
            signature: None,
            language: Some("rust".to_string()),
        });
        app.update_graph_preview();
        assert!(app.graph_preview.is_some());
        assert!(app.graph_preview.as_ref().unwrap().is_err());
        let err_text = app.render_tui_graph_preview().unwrap_err();
        assert!(err_text.contains("Error building graph context"));

        // Clean up
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
