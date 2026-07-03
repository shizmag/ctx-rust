use std::collections::HashSet;
use std::path::{Path, PathBuf};
use ratatui::widgets::ListState;
use ctx_models::{NodeKind, TreeNode, get_relative_path};
use crate::search::collect_matching_files;

pub(crate) struct TuiApp {
    pub(crate) path: PathBuf,
    pub(crate) scan_result: ctx_models::ScanResult,
    pub(crate) expanded_dirs: HashSet<PathBuf>,
    pub(crate) checked_paths: HashSet<PathBuf>,
    pub(crate) visible_items: Vec<VisibleTuiNode>,
    pub(crate) list_state: ListState,
    pub(crate) message: Option<(String, std::time::Instant)>,
    pub(crate) search_active: bool,
    pub(crate) search_query: String,
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
    pub(crate) tree_line_prefix: String,
    pub(crate) is_text: bool,
}

impl TuiApp {
    pub(crate) fn new(path: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let scan_result = ctx_core::scan(&path, ctx_models::ScanOptions::default())?;
        
        let mut expanded_dirs = HashSet::new();
        let checked_paths = HashSet::new();
        
        expanded_dirs.insert(scan_result.root.path.clone());

        let mut app = Self {
            path,
            scan_result,
            expanded_dirs,
            checked_paths,
            visible_items: Vec::new(),
            list_state: ListState::default(),
            message: None,
            search_active: false,
            search_query: String::new(),
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

    pub(crate) fn rescan(&mut self) -> Result<(), Box<dyn std::error::Error>> {
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
            tree_line_prefix: line.prefix.clone(),
            is_text: line.node.stats.lines > 0 || line.node.stats.bytes == 0,
        });

        is_dir && is_expanded
    });
}

pub(crate) fn set_checked_recursive(node: &TreeNode, checked: bool, checked_paths: &mut HashSet<PathBuf>) {
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
