use std::path::Path;

pub fn load_gitignore(root_path: &Path, exclude_patterns: &[String]) -> Option<ignore::gitignore::Gitignore> {
    let gitignore_path = root_path.join(".gitignore");
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root_path);

    for pattern in exclude_patterns {
        let _ = builder.add_line(None, pattern);
    }

    if !gitignore_path.exists() {
        if !exclude_patterns.is_empty() {
            return builder.build().ok();
        }
        return None;
    }

    let content = match std::fs::read_to_string(&gitignore_path) {
        Ok(c) => c,
        Err(_) => {
            if !exclude_patterns.is_empty() {
                return builder.build().ok();
            }
            return None;
        }
    };

    let mut current_block: Vec<String> = Vec::new();
    let mut has_ctx = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !current_block.is_empty() {
                if !has_ctx {
                    for rule in &current_block {
                        let _ = builder.add_line(None, rule);
                    }
                }
                current_block.clear();
                has_ctx = false;
            }
        } else if trimmed == "#[ctx]" {
            has_ctx = true;
        } else {
            current_block.push(line.to_string());
        }
    }

    if !current_block.is_empty() && !has_ctx {
        for rule in &current_block {
            let _ = builder.add_line(None, rule);
        }
    }

    builder.build().ok()
}
