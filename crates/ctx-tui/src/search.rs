use ctx_models::TreeNode;

pub(crate) fn collect_matching_files<'a>(
    node: &'a TreeNode,
    query: &str,
    matches: &mut Vec<&'a TreeNode>,
) {
    let query_lower = query.to_lowercase();
    collect_matching_files_impl(node, &query_lower, matches);
}

fn collect_matching_files_impl<'a>(
    node: &'a TreeNode,
    query_lower: &str,
    matches: &mut Vec<&'a TreeNode>,
) {
    if node.kind == ctx_models::NodeKind::File {
        let name_matches = node.name.to_lowercase().contains(query_lower);
        let mut content_matches = false;
        if !name_matches && node.stats.lines > 0 && node.stats.bytes <= 512 * 1024 {
            if let ctx_models::FileContentResult::Text(content) =
                ctx_models::read_file_content(&node.path, 512 * 1024)
            {
                if content.to_lowercase().contains(query_lower) {
                    content_matches = true;
                }
            }
        }
        if name_matches || content_matches {
            matches.push(node);
        }
    }
    for child in &node.children {
        collect_matching_files_impl(child, query_lower, matches);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_models::{NodeKind, NodeStats, TreeNode};
    use std::path::PathBuf;

    #[test]
    fn test_collect_matching_files() {
        let root = TreeNode {
            name: "root".to_string(),
            path: PathBuf::from("."),
            kind: NodeKind::Directory,
            stats: NodeStats::default(),
            children: vec![
                TreeNode {
                    name: "foo.txt".to_string(),
                    path: PathBuf::from("foo.txt"),
                    kind: NodeKind::File,
                    stats: NodeStats {
                        lines: 10,
                        bytes: 100,
                        ..Default::default()
                    },
                    children: vec![],
                },
                TreeNode {
                    name: "bar.rs".to_string(),
                    path: PathBuf::from("bar.rs"),
                    kind: NodeKind::File,
                    stats: NodeStats {
                        lines: 5,
                        bytes: 50,
                        ..Default::default()
                    },
                    children: vec![],
                },
            ],
        };

        // Search by name
        let mut matches = Vec::new();
        collect_matching_files(&root, "foo", &mut matches);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "foo.txt");

        // Case insensitivity
        let mut matches = Vec::new();
        collect_matching_files(&root, "FOO", &mut matches);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "foo.txt");

        // Search for non-existent
        let mut matches = Vec::new();
        collect_matching_files(&root, "baz", &mut matches);
        assert!(matches.is_empty());

        // Search by content
        let temp_file_path = PathBuf::from("test_content_match.txt");
        std::fs::write(&temp_file_path, "Hello search world!").unwrap();

        let root_content = TreeNode {
            name: "root".to_string(),
            path: PathBuf::from("."),
            kind: NodeKind::Directory,
            stats: NodeStats::default(),
            children: vec![TreeNode {
                name: "test_content_match.txt".to_string(),
                path: temp_file_path.clone(),
                kind: NodeKind::File,
                stats: NodeStats {
                    lines: 1,
                    bytes: 20,
                    ..Default::default()
                },
                children: vec![],
            }],
        };

        let mut matches = Vec::new();
        collect_matching_files(&root_content, "search world", &mut matches);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "test_content_match.txt");

        let _ = std::fs::remove_file(temp_file_path);
    }
}
