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

#[cfg(test)]
mod tests {
    use super::*;

    fn file_stats(lines: usize, bytes: u64) -> NodeStats {
        NodeStats {
            files: 1,
            dirs: 0,
            lines,
            bytes,
            tokens: lines,
            tests: 0,
            covered_lines: 0,
            coverable_lines: 0,
        }
    }

    #[test]
    fn new_creates_root_directory_node() {
        let root_path = PathBuf::from("/tmp/my-project");
        let builder = TreeBuilder::new(root_path.clone());

        assert_eq!(builder.root.name, "my-project");
        assert_eq!(builder.root.path, root_path);
        assert_eq!(builder.root.kind, NodeKind::Directory);
        assert!(builder.root.children.is_empty());
    }

    #[test]
    fn add_node_ignores_paths_outside_root() {
        let root_path = PathBuf::from("/tmp/project");
        let mut builder = TreeBuilder::new(root_path);

        builder.add_node(
            Path::new("/other/file.rs"),
            NodeKind::File,
            file_stats(1, 10),
        );

        assert!(builder.root.children.is_empty());
    }

    #[test]
    fn add_node_builds_nested_structure_and_aggregates_stats() {
        let root_path = PathBuf::from("/tmp/project");
        let mut builder = TreeBuilder::new(root_path.clone());

        builder.add_node(
            &root_path.join("src/main.rs"),
            NodeKind::File,
            file_stats(5, 50),
        );
        builder.add_node(
            &root_path.join("src/lib.rs"),
            NodeKind::File,
            file_stats(3, 30),
        );
        builder.add_node(&root_path.join("docs"), NodeKind::Directory, NodeStats {
            files: 0,
            dirs: 1,
            lines: 0,
            bytes: 0,
            tokens: 0,
            tests: 0,
            covered_lines: 0,
            coverable_lines: 0,
        });

        let root = builder.finish();

        assert_eq!(root.stats.files, 2);
        assert_eq!(root.stats.dirs, 3); // root + src + docs
        assert_eq!(root.stats.lines, 8);
        assert_eq!(root.stats.bytes, 80);

        let child_names: Vec<_> = root.children.iter().map(|node| node.name.as_str()).collect();
        assert_eq!(child_names, vec!["docs", "src"]);

        let src = root.children.iter().find(|node| node.name == "src").unwrap();
        assert_eq!(src.stats.files, 2);
        assert_eq!(src.stats.lines, 8);
        let src_child_names: Vec<_> = src.children.iter().map(|node| node.name.as_str()).collect();
        assert_eq!(src_child_names, vec!["lib.rs", "main.rs"]);
    }

    #[test]
    fn add_node_skips_duplicate_leaf_nodes() {
        let root_path = PathBuf::from("/tmp/project");
        let mut builder = TreeBuilder::new(root_path.clone());

        builder.add_node(
            &root_path.join("README.md"),
            NodeKind::File,
            file_stats(1, 10),
        );
        builder.add_node(
            &root_path.join("README.md"),
            NodeKind::File,
            file_stats(99, 999),
        );

        let root = builder.finish();

        assert_eq!(root.stats.files, 1);
        assert_eq!(root.stats.lines, 1);
        assert_eq!(root.stats.bytes, 10);
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].name, "README.md");
    }

    #[test]
    fn finish_sorts_directories_before_files() {
        let root_path = PathBuf::from("/tmp/project");
        let mut builder = TreeBuilder::new(root_path.clone());

        builder.add_node(
            &root_path.join("zebra.rs"),
            NodeKind::File,
            file_stats(1, 1),
        );
        builder.add_node(&root_path.join("alpha"), NodeKind::Directory, NodeStats {
            files: 0,
            dirs: 1,
            lines: 0,
            bytes: 0,
            tokens: 0,
            tests: 0,
            covered_lines: 0,
            coverable_lines: 0,
        });
        builder.add_node(&root_path.join("beta.txt"), NodeKind::File, file_stats(1, 1));

        let root = builder.finish();
        let names: Vec<_> = root.children.iter().map(|node| node.name.as_str()).collect();

        assert_eq!(names, vec!["alpha", "beta.txt", "zebra.rs"]);
    }
}
