use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};

pub struct GitignoreMatcher {
    pub dir_path: PathBuf,
    pub git_ignore: Option<Gitignore>,
    pub ctx_bypass: Option<Gitignore>,
}

pub struct IgnoreStack {
    pub root_path: PathBuf,
    pub global_ignore: Option<Gitignore>,
    pub local_exclude: Option<Gitignore>,
    pub matchers: Vec<GitignoreMatcher>,
}

impl IgnoreStack {
    pub fn new(root_path: PathBuf, exclude_patterns: &[String]) -> Self {
        ensure_gitignore_entries(&root_path);
        // 1. Build global ignore if available
        let mut global_ignore = None;
        if let Some(home) = get_home_dir() {
            let candidate_paths = [
                home.join(".gitignore_global"),
                home.join(".config/git/ignore"),
                home.join(".gitignore"),
            ];
            for path in &candidate_paths {
                if path.exists() {
                    if let Ok(content) = std::fs::read_to_string(path) {
                        let parsed = parse_gitignore(&content);
                        global_ignore = build_gitignore_from_rules(
                            &root_path,
                            &root_path,
                            &parsed.normal_rules,
                        );
                        if global_ignore.is_some() {
                            break;
                        }
                    }
                }
            }
        }

        // 2. Build local exclude
        let git_exclude_path = root_path.join(".git/info/exclude");
        let mut local_exclude = None;
        if git_exclude_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&git_exclude_path) {
                let parsed = parse_gitignore(&content);
                local_exclude =
                    build_gitignore_from_rules(&root_path, &root_path, &parsed.normal_rules);
            }
        }

        // 3. Build extra excludes from scan options
        let root_extra = build_gitignore_from_rules(&root_path, &root_path, exclude_patterns);

        let mut stack = Self {
            root_path: root_path.clone(),
            global_ignore,
            local_exclude,
            matchers: Vec::new(),
        };

        // Push a root level matcher for root .gitignore
        stack.update_for_path(&root_path);

        // If root_extra was built, we can add it as a separate matcher at root_path
        if let Some(extra) = root_extra {
            stack.matchers.push(GitignoreMatcher {
                dir_path: root_path,
                git_ignore: Some(extra),
                ctx_bypass: None,
            });
        }

        stack
    }

    pub fn update_for_path(&mut self, path: &Path) {
        // While matchers contains a path that is not an ancestor of `path`, pop it
        while let Some(last) = self.matchers.last() {
            if !path.starts_with(&last.dir_path) {
                self.matchers.pop();
            } else {
                break;
            }
        }

        // Find all ancestor directories of `path` starting from `root_path` up to the parent of `path`
        // that are not yet in the stack, and load their .gitignore files.
        let mut to_add = Vec::new();
        let mut current = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };

        while current.starts_with(&self.root_path) {
            if self.matchers.iter().any(|m| m.dir_path == current) {
                break;
            }
            to_add.push(current.to_path_buf());
            if current == self.root_path {
                break;
            }
            if let Some(parent) = current.parent() {
                current = parent;
            } else {
                break;
            }
        }

        to_add.reverse();

        for dir in to_add {
            let gitignore_path = dir.join(".gitignore");
            let mut git_ignore = None;
            let mut ctx_bypass = None;

            if gitignore_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&gitignore_path) {
                    let parsed = parse_gitignore(&content);
                    git_ignore =
                        build_gitignore_from_rules(&self.root_path, &dir, &parsed.normal_rules);
                    ctx_bypass =
                        build_gitignore_from_rules(&self.root_path, &dir, &parsed.ctx_rules);
                }
            }

            self.matchers.push(GitignoreMatcher {
                dir_path: dir,
                git_ignore,
                ctx_bypass,
            });
        }
    }

    pub fn is_ignored(&mut self, path: &Path, is_dir: bool) -> bool {
        self.update_for_path(path);

        for matcher in self.matchers.iter().rev() {
            if let Some(ref bypass) = matcher.ctx_bypass {
                if bypass.matched(path, is_dir).is_ignore() {
                    return false;
                }
            }
            if let Some(ref gi) = matcher.git_ignore {
                if gi.matched(path, is_dir).is_ignore() {
                    return true;
                }
            }
        }

        // Check local exclude
        if let Some(ref gi) = self.local_exclude {
            if gi.matched(path, is_dir).is_ignore() {
                return true;
            }
        }

        // Check global ignore
        if let Some(ref gi) = self.global_ignore {
            if gi.matched(path, is_dir).is_ignore() {
                return true;
            }
        }

        false
    }
}

pub struct ParsedGitignore {
    pub normal_rules: Vec<String>,
    pub ctx_rules: Vec<String>,
}

pub fn parse_gitignore(content: &str) -> ParsedGitignore {
    let mut normal_rules = Vec::new();
    let mut ctx_rules = Vec::new();
    let mut current_block = Vec::new();
    let mut has_ctx = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !current_block.is_empty() {
                if has_ctx {
                    ctx_rules.append(&mut current_block);
                } else {
                    normal_rules.append(&mut current_block);
                }
                has_ctx = false;
            }
        } else if trimmed == "#[ctx]" {
            if !current_block.is_empty() {
                if has_ctx {
                    ctx_rules.append(&mut current_block);
                } else {
                    normal_rules.append(&mut current_block);
                }
            }
            has_ctx = true;
        } else {
            current_block.push(line.to_string());
        }
    }

    if !current_block.is_empty() {
        if has_ctx {
            ctx_rules.extend(current_block);
        } else {
            normal_rules.extend(current_block);
        }
    }

    ParsedGitignore {
        normal_rules,
        ctx_rules,
    }
}

fn build_gitignore_from_rules(
    root_path: &Path,
    dir_path: &Path,
    rules: &[String],
) -> Option<Gitignore> {
    if rules.is_empty() {
        return None;
    }
    let mut builder = GitignoreBuilder::new(root_path);
    for rule in rules {
        let _ = builder.add_line(Some(dir_path.to_path_buf()), rule);
    }
    builder.build().ok()
}

fn get_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

fn ensure_gitignore_entries(root: &Path) {
    let gitignore_path = root.join(".gitignore");
    let has_git = root.join(".git").exists();

    if gitignore_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&gitignore_path) {
            let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let mut changed = false;

            let has_codegraph = lines.iter().any(|l| {
                let trimmed = l.trim();
                trimmed == ".ctx-codegraph" || trimmed == ".ctx-codegraph/"
            });
            let has_ctx_wildcard = lines.iter().any(|l| {
                let trimmed = l.trim();
                trimmed == ".ctx_*" || trimmed == ".ctx_*/"
            });

            if !has_codegraph {
                lines.push(".ctx-codegraph/".to_string());
                changed = true;
            }
            if !has_ctx_wildcard {
                lines.push(".ctx_*/".to_string());
                changed = true;
            }

            if changed {
                let mut new_content = lines.join("\n");
                if !new_content.ends_with('\n') {
                    new_content.push('\n');
                }
                let _ = std::fs::write(&gitignore_path, new_content);
            }
        }
    } else if has_git {
        let content = ".ctx-codegraph/\n.ctx_*/\n";
        let _ = std::fs::write(&gitignore_path, content);
    }
}

