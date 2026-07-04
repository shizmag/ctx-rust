use crate::search::collect_matching_files;
use ctx_models::{NodeKind, TreeNode, get_relative_path};
use ratatui::widgets::ListState;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TuiScreen {
    TreePicker,
    SymbolSearch,
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
    use std::fs;

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
        let options = ctx_codegraph::BuildIndexOptions {
            use_rust_analyzer: false, // fast tree-sitter fallback
            max_depth: None,
            include_tests: true,
        };
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
}
