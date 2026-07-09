use ctx_models::Mode;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub mode: Option<Mode>,
    pub max_depth: Option<usize>,
    pub max_file_size: Option<u64>,
    pub exclude: Vec<String>,
    // Global settings connected to app config, for AI agent optimization (MCP defaults etc).
    // These drive defaults for output formats (e.g. yaml for agents), LSP, stats, etc.
    pub default_format: Option<String>,
    pub mcp_target: Option<String>,
    pub use_lsp: Option<bool>,
    pub stats_enabled: Option<bool>,
    pub default_packing: Option<String>,
    pub default_ranking: Option<String>,
    pub default_token_budget: Option<usize>,
}

pub fn load_config(path: &Path) -> Result<Config, std::io::Error> {
    if !path.exists() {
        return Ok(Config::default());
    }

    let content = fs::read_to_string(path)?;
    let mut config = Config::default();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        let key = key.trim().to_lowercase();
        let value = value.trim();

        match key.as_str() {
            "mode" => {
                let m = match value.to_lowercase().as_str() {
                    "smart" => Some(Mode::Smart),
                    "all" => Some(Mode::All),
                    "code" => Some(Mode::Code),
                    "docs" => Some(Mode::Docs),
                    "llm" => Some(Mode::Llm),
                    _ => None,
                };
                if m.is_some() {
                    config.mode = m;
                }
            }
            "max_depth" => {
                if let Ok(depth) = value.parse::<usize>() {
                    config.max_depth = Some(depth);
                }
            }
            "max_file_size" => {
                if let Ok(size) = value.parse::<u64>() {
                    config.max_file_size = Some(size);
                }
            }
            "exclude" => {
                let items: Vec<String> = value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                config.exclude.extend(items);
            }
            // Settings for AI/agent optimization, MCP behavior, install targets.
            // Support common aliases for convenience in .ctxconfig
            "default_format" | "format" | "agent_format" => {
                if !value.is_empty() {
                    config.default_format = Some(value.to_string());
                }
            }
            "mcp_target" | "install_target" | "target" => {
                if !value.is_empty() {
                    config.mcp_target = Some(value.to_string());
                }
            }
            "use_lsp" | "lsp" => {
                if let Ok(b) = value.parse::<bool>() {
                    config.use_lsp = Some(b);
                }
            }
            "stats_enabled" | "collect_stats" | "stats" => {
                if let Ok(b) = value.parse::<bool>() {
                    config.stats_enabled = Some(b);
                }
            }
            "default_packing" | "packing" => {
                if !value.is_empty() {
                    config.default_packing = Some(value.to_string());
                }
            }
            "default_ranking" | "ranking" => {
                if !value.is_empty() {
                    config.default_ranking = Some(value.to_string());
                }
            }
            "default_token_budget" | "token_budget" => {
                if let Ok(b) = value.parse::<usize>() {
                    config.default_token_budget = Some(b);
                }
            }
            _ => {}
        }
    }

    Ok(config)
}

pub fn find_config(start_dir: &Path) -> Option<PathBuf> {
    let mut current = match start_dir.canonicalize() {
        Ok(path) => path,
        Err(_) => return None,
    };

    loop {
        let config_path = current.join(".ctxconfig");
        if config_path.exists() {
            return Some(config_path);
        }

        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }

    None
}

pub fn find_and_load_config(start_dir: &Path) -> Result<Config, std::io::Error> {
    if let Some(config_path) = find_config(start_dir) {
        load_config(&config_path)
    } else {
        Ok(Config::default())
    }
}
