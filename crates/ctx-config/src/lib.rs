use ctx_models::Mode;
use std::fs;
use std::path::{Path, PathBuf};

/// XDG config subdirectory for ctx (`~/.config/ctx/`).
pub const CONFIG_DIR_NAME: &str = "ctx";
/// Global config filename inside [`CONFIG_DIR_NAME`].
pub const CONFIG_FILE_NAME: &str = "config";

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
    // Hybrid search / ONNX models
    pub embedding_model: Option<String>,
    pub reranker_model: Option<String>,
    pub tokenizer_dir: Option<String>,
    pub rrf_k: Option<usize>,
    pub bm25_top_k: Option<usize>,
    pub dense_top_k: Option<usize>,
    pub rerank_top_k: Option<usize>,
    pub enable_rerank: Option<bool>,
    pub default_retrieval_strategy: Option<String>,
}

/// Default embedding ONNX path when not set in global/project config.
pub const DEFAULT_EMBEDDING_MODEL: &str =
    "/Users/vladimirkasterin/models/embeddings/snowflake-arctic-embed-m-v2.0/model.onnx";

/// Default reranker ONNX path when not set in global/project config.
pub const DEFAULT_RERANKER_MODEL: &str =
    "/Users/vladimirkasterin/models/reranker/jina-reranker-v2-base-multilingual/model.onnx";

impl Config {
    pub fn resolved_embedding_model(&self) -> Option<PathBuf> {
        self.embedding_model.as_ref().map(PathBuf::from)
    }

    pub fn resolved_reranker_model(&self) -> Option<PathBuf> {
        self.reranker_model.as_ref().map(PathBuf::from)
    }

    /// Suggested default embedding path for documentation / CLI hints.
    pub fn default_embedding_model_path() -> PathBuf {
        PathBuf::from(DEFAULT_EMBEDDING_MODEL)
    }

    /// Suggested default reranker path for documentation / CLI hints.
    pub fn default_reranker_model_path() -> PathBuf {
        PathBuf::from(DEFAULT_RERANKER_MODEL)
    }

    pub fn resolved_tokenizer_dir(&self, embedding_model: &Path) -> PathBuf {
        self.tokenizer_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| embedding_model.parent().unwrap_or(embedding_model).to_path_buf())
    }

    pub fn search_auto_enabled(&self) -> bool {
        self.resolved_embedding_model().is_some()
    }

    pub fn effective_with_embeddings(&self, cli_override: Option<bool>) -> bool {
        match cli_override {
            Some(v) => v,
            None => self.search_auto_enabled(),
        }
    }

    pub fn effective_with_lexical(&self, cli_override: Option<bool>) -> bool {
        match cli_override {
            Some(false) => false,
            Some(true) => true,
            None => self.search_auto_enabled(),
        }
    }
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
            "embedding_model" | "embedding_model_path" => {
                if !value.is_empty() {
                    config.embedding_model = Some(value.to_string());
                }
            }
            "reranker_model" | "reranker_model_path" => {
                if !value.is_empty() {
                    config.reranker_model = Some(value.to_string());
                }
            }
            "tokenizer_dir" => {
                if !value.is_empty() {
                    config.tokenizer_dir = Some(value.to_string());
                }
            }
            "rrf_k" => {
                if let Ok(v) = value.parse::<usize>() {
                    config.rrf_k = Some(v);
                }
            }
            "bm25_top_k" => {
                if let Ok(v) = value.parse::<usize>() {
                    config.bm25_top_k = Some(v);
                }
            }
            "dense_top_k" => {
                if let Ok(v) = value.parse::<usize>() {
                    config.dense_top_k = Some(v);
                }
            }
            "rerank_top_k" => {
                if let Ok(v) = value.parse::<usize>() {
                    config.rerank_top_k = Some(v);
                }
            }
            "enable_rerank" => {
                if let Ok(b) = value.parse::<bool>() {
                    config.enable_rerank = Some(b);
                }
            }
            "default_retrieval_strategy" | "retrieval_strategy" => {
                if !value.is_empty() {
                    config.default_retrieval_strategy = Some(value.to_string());
                }
            }
            _ => {}
        }
    }

    Ok(config)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

/// Resolve the XDG config home directory (`$XDG_CONFIG_HOME` or `~/.config`).
pub fn xdg_config_home() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    home_dir().map(|home| home.join(".config"))
}

/// Path to the global ctx config file (`~/.config/ctx/config`).
pub fn global_config_path() -> Option<PathBuf> {
    xdg_config_home().map(|root| root.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME))
}

/// Walk upward from `start_dir` looking for a legacy project-local `.ctxconfig`.
pub fn find_project_config(start_dir: &Path) -> Option<PathBuf> {
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

/// Return the global config path used by `ctx setting` for reads and writes.
pub fn find_config(_start_dir: &Path) -> Option<PathBuf> {
    global_config_path()
}

/// Merge `overlay` on top of `base`; project-local values win when set.
pub fn merge_configs(base: Config, overlay: Config) -> Config {
    Config {
        mode: overlay.mode.or(base.mode),
        max_depth: overlay.max_depth.or(base.max_depth),
        max_file_size: overlay.max_file_size.or(base.max_file_size),
        exclude: if overlay.exclude.is_empty() {
            base.exclude
        } else {
            overlay.exclude
        },
        default_format: overlay.default_format.or(base.default_format),
        mcp_target: overlay.mcp_target.or(base.mcp_target),
        use_lsp: overlay.use_lsp.or(base.use_lsp),
        stats_enabled: overlay.stats_enabled.or(base.stats_enabled),
        default_packing: overlay.default_packing.or(base.default_packing),
        default_ranking: overlay.default_ranking.or(base.default_ranking),
        default_token_budget: overlay.default_token_budget.or(base.default_token_budget),
        embedding_model: overlay.embedding_model.or(base.embedding_model),
        reranker_model: overlay.reranker_model.or(base.reranker_model),
        tokenizer_dir: overlay.tokenizer_dir.or(base.tokenizer_dir),
        rrf_k: overlay.rrf_k.or(base.rrf_k),
        bm25_top_k: overlay.bm25_top_k.or(base.bm25_top_k),
        dense_top_k: overlay.dense_top_k.or(base.dense_top_k),
        rerank_top_k: overlay.rerank_top_k.or(base.rerank_top_k),
        enable_rerank: overlay.enable_rerank.or(base.enable_rerank),
        default_retrieval_strategy: overlay
            .default_retrieval_strategy
            .or(base.default_retrieval_strategy),
    }
}

pub fn find_and_load_config(start_dir: &Path) -> Result<Config, std::io::Error> {
    let global = if let Some(path) = global_config_path() {
        load_config(&path)?
    } else {
        Config::default()
    };

    let merged = if let Some(project_path) = find_project_config(start_dir) {
        let project = load_config(&project_path)?;
        merge_configs(global, project)
    } else {
        global
    };

    Ok(merged)
}

/// Save the config to the global config file (or an explicit path).
/// Produces the simple key=value format understood by load_config.
pub fn save_config(config_path: &Path, config: &Config) -> Result<(), std::io::Error> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut lines: Vec<String> = vec![
        format!(
            "# {} - saved by `ctx setting`",
            config_path.display()
        ),
        "# Edit manually or via interactive TUI".to_string(),
    ];

    if let Some(m) = &config.mode {
        lines.push(format!("mode = {}", mode_to_str(m)));
    }
    if let Some(d) = config.max_depth {
        lines.push(format!("max_depth = {}", d));
    }
    if let Some(s) = config.max_file_size {
        lines.push(format!("max_file_size = {}", s));
    }
    if !config.exclude.is_empty() {
        lines.push(format!("exclude = {}", config.exclude.join(", ")));
    }
    if let Some(f) = &config.default_format {
        lines.push(format!("default_format = {}", f));
    }
    if let Some(t) = &config.mcp_target {
        lines.push(format!("mcp_target = {}", t));
    }
    if let Some(b) = config.use_lsp {
        lines.push(format!("use_lsp = {}", b));
    }
    if let Some(b) = config.stats_enabled {
        lines.push(format!("stats_enabled = {}", b));
    }
    if let Some(p) = &config.default_packing {
        lines.push(format!("default_packing = {}", p));
    }
    if let Some(r) = &config.default_ranking {
        lines.push(format!("default_ranking = {}", r));
    }
    if let Some(b) = config.default_token_budget {
        lines.push(format!("default_token_budget = {}", b));
    }
    if let Some(p) = &config.embedding_model {
        lines.push(format!("embedding_model = {}", p));
    }
    if let Some(p) = &config.reranker_model {
        lines.push(format!("reranker_model = {}", p));
    }
    if let Some(p) = &config.tokenizer_dir {
        lines.push(format!("tokenizer_dir = {}", p));
    }
    if let Some(v) = config.rrf_k {
        lines.push(format!("rrf_k = {}", v));
    }
    if let Some(v) = config.bm25_top_k {
        lines.push(format!("bm25_top_k = {}", v));
    }
    if let Some(v) = config.dense_top_k {
        lines.push(format!("dense_top_k = {}", v));
    }
    if let Some(v) = config.rerank_top_k {
        lines.push(format!("rerank_top_k = {}", v));
    }
    if let Some(b) = config.enable_rerank {
        lines.push(format!("enable_rerank = {}", b));
    }
    if let Some(s) = &config.default_retrieval_strategy {
        lines.push(format!("default_retrieval_strategy = {}", s));
    }

    let content = lines.join("\n") + "\n";
    fs::write(config_path, content)
}

/// Save config to the global location (`~/.config/ctx/config`).
pub fn save_global_config(config: &Config) -> Result<PathBuf, std::io::Error> {
    let path = global_config_path().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "could not resolve global config path (HOME/XDG_CONFIG_HOME not set)",
        )
    })?;
    save_config(&path, config)?;
    Ok(path)
}

fn mode_to_str(m: &Mode) -> &'static str {
    match m {
        Mode::Smart => "smart",
        Mode::All => "all",
        Mode::Code => "code",
        Mode::Docs => "docs",
        Mode::Llm => "llm",
    }
}
