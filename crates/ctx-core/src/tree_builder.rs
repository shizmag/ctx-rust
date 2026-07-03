use std::path::{Path, PathBuf};

use ctx_models::{NodeKind, NodeStats, TreeNode};

pub struct TreeBuilder {
    root_path: PathBuf,
    root: TreeNode,
}

impl TreeBuilder {
    pub fn new(root_path: PathBuf) -> Self {
        let name = root_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(".")
            .to_string();

        Self {
            root_path: root_path.clone(),
            root: TreeNode {
                name,
                path: root_path,
                kind: NodeKind::Directory,
                stats: NodeStats::default(),
                children: Vec::new(),
            },
        }
    }

    pub fn add_node(&mut self, path: &Path, kind: NodeKind, stats: NodeStats) {
        let Ok(relative_path) = path.strip_prefix(&self.root_path) else {
            return;
        };

        if relative_path.as_os_str().is_empty() {
            return;
        }

        let components: Vec<String> = relative_path
            .components()
            .map(|component| component.as_os_str().to_string_lossy().to_string())
            .collect();

        insert_node(&mut self.root, path.to_path_buf(), &components, kind, stats);
    }

    pub fn finish(mut self) -> TreeNode {
        recompute_stats(&mut self.root);
        sort_tree(&mut self.root);
        self.root
    }
}

fn insert_node(
    current: &mut TreeNode,
    full_path: PathBuf,
    components: &[String],
    kind: NodeKind,
    stats: NodeStats,
) {
    let Some((name, rest)) = components.split_first() else {
        return;
    };

    let is_leaf = rest.is_empty();

    if is_leaf {
        if current.children.iter().any(|child| child.name == *name) {
            return;
        }

        current.children.push(TreeNode {
            name: name.clone(),
            path: full_path,
            kind,
            stats,
            children: Vec::new(),
        });

        return;
    }

    let child_index = find_child_dir_index(current, name).unwrap_or_else(|| {
        current.children.push(TreeNode {
            name: name.clone(),
            path: current.path.join(name),
            kind: NodeKind::Directory,
            stats: NodeStats::default(),
            children: Vec::new(),
        });

        current.children.len() - 1
    });

    insert_node(
        &mut current.children[child_index],
        full_path,
        rest,
        kind,
        stats,
    );
}

fn find_child_dir_index(parent: &TreeNode, name: &str) -> Option<usize> {
    parent
        .children
        .iter()
        .position(|child| child.name == name && child.kind == NodeKind::Directory)
}

fn recompute_stats(node: &mut TreeNode) -> NodeStats {
    let mut stats = match node.kind {
        NodeKind::File => node.stats.clone(),
        NodeKind::Directory => NodeStats {
            files: 0,
            dirs: 1,
            lines: 0,
            bytes: 0,
            tokens: 0,
            tests: 0,
            covered_lines: 0,
            coverable_lines: 0,
        },
        NodeKind::Symlink | NodeKind::Other => node.stats.clone(),
    };

    for child in &mut node.children {
        let child_stats = recompute_stats(child);

        stats.files += child_stats.files;
        stats.dirs += child_stats.dirs;
        stats.lines += child_stats.lines;
        stats.bytes += child_stats.bytes;
        stats.tokens += child_stats.tokens;
        stats.tests += child_stats.tests;
        stats.covered_lines += child_stats.covered_lines;
        stats.coverable_lines += child_stats.coverable_lines;
    }

    node.stats = stats.clone();
    stats
}

fn sort_tree(node: &mut TreeNode) {
    node.children.sort_by(|a, b| {
        match (a.kind == NodeKind::Directory, b.kind == NodeKind::Directory) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        }
    });

    for child in &mut node.children {
        sort_tree(child);
    }
}
