use ctx_config::{
    ensure_global_config, find_and_load_config, find_config, find_project_config,
    global_config_path, load_config, load_global_config, merge_configs, save_config,
    save_global_config, Config, CONFIG_DIR_NAME, CONFIG_FILE_NAME, EnsureOutcome,
};
use ctx_models::Mode;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn with_xdg_config_home<F: FnOnce(&PathBuf)>(f: F) {
    let _guard = env_lock();
    let temp_dir = tempfile::tempdir().unwrap();
    let xdg = temp_dir.path().join("xdg-config");
    fs::create_dir_all(&xdg).unwrap();
    // SAFETY: guarded test-only env mutation.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &xdg) };
    f(&xdg);
    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
}

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn test_load_config() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("config");
    fs::write(
        &config_path,
        r#"
# Test Configuration
mode = code
max_depth = 8
max_file_size = 1048576
exclude = target, node_modules, temp_file.txt
"#,
    )
    .unwrap();

    let config = load_config(&config_path).unwrap();

    assert_eq!(config.mode, Some(Mode::Code));
    assert_eq!(config.max_depth, Some(8));
    assert_eq!(config.max_file_size, Some(1048576));
    assert_eq!(
        config.exclude,
        vec![
            "target".to_string(),
            "node_modules".to_string(),
            "temp_file.txt".to_string()
        ]
    );
}

#[test]
fn test_find_project_config() {
    let temp_dir = tempfile::tempdir().unwrap();

    let sub_dir = temp_dir.path().join("src/bin/inner");
    fs::create_dir_all(&sub_dir).unwrap();

    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(&config_path, "mode = smart\n").unwrap();

    let found = find_project_config(&sub_dir).unwrap();
    assert_eq!(
        found.canonicalize().unwrap(),
        config_path.canonicalize().unwrap()
    );
}

#[test]
fn global_config_path_uses_xdg_config_home() {
    with_xdg_config_home(|xdg| {
        let path = global_config_path().unwrap();
        assert_eq!(
            path,
            xdg.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME)
        );
    });
}

#[test]
fn find_config_returns_global_path() {
    with_xdg_config_home(|xdg| {
        let found = find_config(PathBuf::from(".").as_path()).unwrap();
        assert_eq!(
            found,
            xdg.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME)
        );
    });
}

#[test]
fn find_and_load_config_merges_global_and_project() {
    with_xdg_config_home(|xdg| {
        let temp_dir = tempfile::tempdir().unwrap();
        let global_path = xdg.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME);
        fs::create_dir_all(global_path.parent().unwrap()).unwrap();
        fs::write(&global_path, "mode = smart\ndefault_format = yaml\n").unwrap();

        let project_path = temp_dir.path().join(".ctxconfig");
        fs::write(&project_path, "mode = code\n").unwrap();

        let config = find_and_load_config(&temp_dir.path()).unwrap();
        assert_eq!(config.mode, Some(Mode::Code));
        assert_eq!(config.default_format.as_deref(), Some("yaml"));
    });
}

#[test]
fn load_config_missing_file_returns_default() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("nonexistent.ctxconfig");

    let config = load_config(&config_path).unwrap();

    assert_eq!(config, Config::default());
}

#[test]
fn load_config_ignores_invalid_mode() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(&config_path, "mode = invalid_mode\n").unwrap();

    let config = load_config(&config_path).unwrap();

    assert_eq!(config.mode, None);
}

#[test]
fn load_config_ignores_invalid_max_depth_and_max_file_size() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(
        &config_path,
        "max_depth = not_a_number\nmax_file_size = also_invalid\n",
    )
    .unwrap();

    let config = load_config(&config_path).unwrap();

    assert_eq!(config.max_depth, None);
    assert_eq!(config.max_file_size, None);
}

#[test]
fn load_config_empty_exclude_list() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(&config_path, "exclude = \n").unwrap();

    let config = load_config(&config_path).unwrap();

    assert!(config.exclude.is_empty());
}

#[test]
fn load_config_ignores_comments_and_blank_lines() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(
        &config_path,
        r#"
# This is a comment

mode = docs

# Another comment
max_depth = 3
"#,
    )
    .unwrap();

    let config = load_config(&config_path).unwrap();

    assert_eq!(config.mode, Some(Mode::Docs));
    assert_eq!(config.max_depth, Some(3));
    assert_eq!(config.max_file_size, None);
    assert!(config.exclude.is_empty());
}

#[test]
fn find_project_config_returns_none_when_no_config_exists() {
    let temp_dir = tempfile::tempdir().unwrap();
    let sub_dir = temp_dir.path().join("src/deep");
    fs::create_dir_all(&sub_dir).unwrap();

    let found = find_project_config(&sub_dir);

    assert!(found.is_none());
}

#[test]
fn find_and_load_config_returns_default_when_no_config() {
    with_xdg_config_home(|_xdg| {
        let temp_dir = tempfile::tempdir().unwrap();
        let sub_dir = temp_dir.path().join("nested/dir");
        fs::create_dir_all(&sub_dir).unwrap();

        let config = find_and_load_config(&sub_dir).unwrap();

        assert_eq!(config, Config::default_values());
    });
}

#[test]
fn load_config_exclude_trims_extra_whitespace() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(
        &config_path,
        "exclude =  target ,  node_modules  , , temp_file.txt \n",
    )
    .unwrap();

    let config = load_config(&config_path).unwrap();

    assert_eq!(
        config.exclude,
        vec![
            "target".to_string(),
            "node_modules".to_string(),
            "temp_file.txt".to_string()
        ]
    );
}

#[test]
fn load_config_agent_settings_and_defaults() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(
        &config_path,
        r#"
# AI agent focused settings
default_format = yaml
mcp_target = cursor
use_lsp = false
stats_enabled = true
default_packing = sandwich
default_ranking = hybrid
default_token_budget = 8000
"#,
    )
    .unwrap();

    let config = load_config(&config_path).unwrap();

    assert_eq!(config.default_format.as_deref(), Some("yaml"));
    assert_eq!(config.mcp_target.as_deref(), Some("cursor"));
    assert_eq!(config.use_lsp, Some(false));
    assert_eq!(config.stats_enabled, Some(true));
    assert_eq!(config.default_packing.as_deref(), Some("sandwich"));
    assert_eq!(config.default_ranking.as_deref(), Some("hybrid"));
    assert_eq!(config.default_token_budget, Some(8000));
}

#[test]
fn load_config_settings_use_aliases_and_ignore_invalid() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(
        &config_path,
        "format = json\nlsp = true\ncollect_stats = 1\npacking = frontloaded\ntoken_budget = notnum\n",
    )
    .unwrap();

    let config = load_config(&config_path).unwrap();

    assert_eq!(config.default_format.as_deref(), Some("json"));
    assert_eq!(config.use_lsp, Some(true));
    assert_eq!(config.stats_enabled, None);
    assert_eq!(config.default_packing.as_deref(), Some("frontloaded"));
    assert_eq!(config.default_token_budget, None);
}

#[test]
fn load_config_settings_defaults_to_none_when_absent() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(&config_path, "mode = llm\n").unwrap();

    let config = load_config(&config_path).unwrap();

    assert_eq!(config.mode, Some(Mode::Llm));
    assert!(config.default_format.is_none());
    assert!(config.use_lsp.is_none());
    assert!(config.stats_enabled.is_none());
    assert!(config.default_token_budget.is_none());
}

#[test]
fn load_config_agent_settings_with_more_aliases() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(
        &config_path,
        "agent_format = yaml\ninstall_target = vscode\nlsp = false\nstats = false\npacking = balanced\nranking = graph\ntoken_budget = 5000\n",
    )
    .unwrap();

    let config = load_config(&config_path).unwrap();

    assert_eq!(config.default_format.as_deref(), Some("yaml"));
    assert_eq!(config.mcp_target.as_deref(), Some("vscode"));
    assert_eq!(config.use_lsp, Some(false));
    assert_eq!(config.stats_enabled, Some(false));
    assert_eq!(config.default_packing.as_deref(), Some("balanced"));
    assert_eq!(config.default_ranking.as_deref(), Some("graph"));
    assert_eq!(config.default_token_budget, Some(5000));
}

#[test]
fn save_config_roundtrip_and_creates_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("config");

    let cfg = Config {
        mode: Some(Mode::Code),
        max_depth: Some(4),
        max_file_size: Some(12345),
        exclude: vec!["target".to_string(), "node_modules".to_string()],
        default_format: Some("yaml".to_string()),
        mcp_target: Some("cursor".to_string()),
        use_lsp: Some(true),
        stats_enabled: Some(false),
        default_packing: Some("sandwich".to_string()),
        default_ranking: Some("hybrid".to_string()),
        default_token_budget: Some(12000),
        ..Default::default()
    };

    save_config(&config_path, &cfg).unwrap();

    let loaded = load_config(&config_path).unwrap();
    assert_eq!(loaded.apply_defaults(), cfg.apply_defaults());

    let mut partial = Config::default();
    partial.mode = Some(Mode::Llm);
    partial.exclude = vec!["*.log".into()];
    let p2 = temp_dir.path().join("config2");
    save_config(&p2, &partial).unwrap();
    let loaded2 = load_config(&p2).unwrap();
    assert_eq!(loaded2.mode, Some(Mode::Llm));
    assert_eq!(loaded2.exclude, vec!["*.log".to_string()]);
    assert_eq!(loaded2.max_depth, Config::default_values().max_depth);
}

#[test]
fn save_global_config_writes_under_xdg_config_home() {
    with_xdg_config_home(|xdg| {
        let mut cfg = Config::default();
        cfg.mode = Some(Mode::Docs);
        cfg.default_format = Some("yaml".into());

        let path = save_global_config(&cfg).unwrap();
        assert_eq!(path, xdg.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME));
        assert!(path.exists());

        let loaded = load_config(&path).unwrap();
        assert_eq!(loaded.mode, Some(Mode::Docs));
        assert_eq!(loaded.default_format.as_deref(), Some("yaml"));
    });
}

#[test]
fn merge_configs_prefers_overlay_values() {
    let base = Config {
        mode: Some(Mode::Smart),
        max_depth: Some(5),
        default_format: Some("yaml".into()),
        exclude: vec!["target".into()],
        ..Default::default()
    };
    let overlay = Config {
        mode: Some(Mode::Code),
        default_token_budget: Some(8000),
        ..Default::default()
    };

    let merged = merge_configs(base, overlay);
    assert_eq!(merged.mode, Some(Mode::Code));
    assert_eq!(merged.max_depth, Some(5));
    assert_eq!(merged.default_format.as_deref(), Some("yaml"));
    assert_eq!(merged.default_token_budget, Some(8000));
    assert_eq!(merged.exclude, vec!["target".to_string()]);
}

#[test]
fn ensure_global_config_imports_legacy_project_ctxconfig_on_create() {
    with_xdg_config_home(|_xdg| {
        let temp_dir = tempfile::tempdir().unwrap();
        fs::write(
            temp_dir.path().join(".ctxconfig"),
            "max_depth = 5\nmax_file_size = 532480\ndefault_token_budget = 8000\n",
        )
        .unwrap();

        let (_, config, outcome) = ensure_global_config(temp_dir.path()).unwrap();
        assert_eq!(outcome, EnsureOutcome::Created);
        assert_eq!(config.max_depth, Some(5));
        assert_eq!(config.max_file_size, Some(532480));
        assert_eq!(config.default_token_budget, Some(8000));
        assert_eq!(config.mode, Config::default_values().mode);
    });
}

#[test]
fn ensure_global_config_imports_legacy_project_when_global_is_pristine() {
    with_xdg_config_home(|xdg| {
        let temp_dir = tempfile::tempdir().unwrap();
        fs::write(
            temp_dir.path().join(".ctxconfig"),
            "max_depth = 5\n",
        )
        .unwrap();

        let path = xdg.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME);
        save_config(&path, &Config::default_values()).unwrap();

        let (_, config, outcome) = ensure_global_config(temp_dir.path()).unwrap();
        assert_eq!(outcome, EnsureOutcome::Upgraded);
        assert_eq!(config.max_depth, Some(5));

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("max_depth = 5"));
        assert!(content.contains("exclude ="));
        assert!(content.contains("# embedding_model ="));
    });
}

#[test]
fn save_config_writes_placeholders_for_optional_paths() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("config");
    save_config(&config_path, &Config::default_values()).unwrap();
    let content = fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("exclude ="));
    assert!(content.contains("# embedding_model ="));
    assert!(content.contains("# embedding_tokenizer ="));
    assert!(content.contains("# rerank_tokenizer ="));
    assert!(content.contains("# mcp_target ="));
}

#[test]
fn load_config_legacy_tokenizer_dir_maps_to_embedding_tokenizer() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("config");
    fs::write(&config_path, "tokenizer_dir = /legacy/tokenizer\n").unwrap();

    let config = load_config(&config_path).unwrap();
    assert_eq!(
        config.embedding_tokenizer.as_deref(),
        Some("/legacy/tokenizer")
    );
    assert!(config.rerank_tokenizer.is_none());
}

#[test]
fn resolved_tokenizers_default_to_model_parent_dirs() {
    let config = Config::default();
    let embedding = std::path::Path::new("/models/embed/model.onnx");
    let reranker = std::path::Path::new("/models/rerank/model.onnx");
    assert_eq!(
        config.resolved_embedding_tokenizer(embedding),
        std::path::PathBuf::from("/models/embed")
    );
    assert_eq!(
        config.resolved_rerank_tokenizer(reranker),
        std::path::PathBuf::from("/models/rerank")
    );
}

#[test]
fn resolved_tokenizers_use_explicit_paths() {
    let config = Config {
        embedding_tokenizer: Some("/tok/embed".into()),
        rerank_tokenizer: Some("/tok/rerank".into()),
        ..Default::default()
    };
    let embedding = std::path::Path::new("/models/embed/model.onnx");
    let reranker = std::path::Path::new("/models/rerank/model.onnx");
    assert_eq!(
        config.resolved_embedding_tokenizer(embedding),
        std::path::PathBuf::from("/tok/embed")
    );
    assert_eq!(
        config.resolved_rerank_tokenizer(reranker),
        std::path::PathBuf::from("/tok/rerank")
    );
}

#[test]
fn default_values_has_no_model_paths() {
    let defaults = Config::default_values();
    assert!(defaults.embedding_model.is_none());
    assert!(defaults.reranker_model.is_none());
    assert!(defaults.embedding_tokenizer.is_none());
    assert!(defaults.rerank_tokenizer.is_none());
    assert_eq!(defaults.mode, Some(Mode::Smart));
    assert_eq!(defaults.default_format.as_deref(), Some("yaml"));
    assert_eq!(defaults.default_retrieval_strategy.as_deref(), Some("hybrid"));
    assert_eq!(defaults.rrf_k, Some(60));
}

#[test]
fn apply_defaults_fills_missing_fields() {
    let partial = Config {
        mode: Some(Mode::Code),
        ..Default::default()
    };
    let filled = partial.apply_defaults();
    assert_eq!(filled.mode, Some(Mode::Code));
    assert_eq!(filled.max_depth, Config::default_values().max_depth);
    assert_eq!(filled.default_format, Config::default_values().default_format);
    assert_eq!(
        filled.default_retrieval_strategy,
        Config::default_values().default_retrieval_strategy
    );
}

#[test]
fn ensure_global_config_creates_defaults_without_model_paths() {
    with_xdg_config_home(|xdg| {
        let empty_project = tempfile::tempdir().unwrap();
        let (path, config, outcome) = ensure_global_config(empty_project.path()).unwrap();
        assert_eq!(outcome, EnsureOutcome::Created);
        assert_eq!(path, xdg.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME));
        assert!(path.exists());

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("mode = smart"));
        assert!(content.contains("default_format = yaml"));
        assert!(content.contains("default_retrieval_strategy = hybrid"));
        assert!(content.contains("rrf_k = 60"));
        assert!(content.contains("# embedding_model ="));
        assert!(content.contains("# reranker_model ="));
        assert!(content.contains("exclude ="));

        assert_eq!(config, Config::default_values());
    });
}

#[test]
fn ensure_global_config_upgrades_partial_existing_file() {
    with_xdg_config_home(|xdg| {
        let path = xdg.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "mode = code\ndefault_format = json\n").unwrap();

        let (_, config, outcome) = ensure_global_config(PathBuf::from(".").as_path()).unwrap();
        assert_eq!(outcome, EnsureOutcome::Upgraded);
        assert_eq!(config.mode, Some(Mode::Code));
        assert_eq!(config.default_format.as_deref(), Some("json"));
        assert_eq!(config.max_depth, Config::default_values().max_depth);
        assert_eq!(
            config.default_retrieval_strategy,
            Config::default_values().default_retrieval_strategy
        );

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("mode = code"));
        assert!(content.contains("default_format = json"));
        assert!(content.contains("max_depth = 10"));
        assert!(content.contains("default_retrieval_strategy = hybrid"));
        assert!(content.contains("# embedding_model ="));
    });
}

#[test]
fn ensure_global_config_is_idempotent_when_complete() {
    with_xdg_config_home(|_xdg| {
        let (_, _, first) = ensure_global_config(PathBuf::from(".").as_path()).unwrap();
        assert_eq!(first, EnsureOutcome::Created);

        let (_, _, second) = ensure_global_config(PathBuf::from(".").as_path()).unwrap();
        assert_eq!(second, EnsureOutcome::Unchanged);
    });
}

#[test]
fn save_config_writes_all_known_settings() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("config");
    save_config(&config_path, &Config::default_values()).unwrap();
    let content = fs::read_to_string(&config_path).unwrap();
    for key in [
        "mode",
        "max_depth",
        "max_file_size",
        "default_format",
        "use_lsp",
        "stats_enabled",
        "default_packing",
        "default_ranking",
        "default_token_budget",
        "rrf_k",
        "bm25_top_k",
        "dense_top_k",
        "rerank_top_k",
        "enable_rerank",
        "default_retrieval_strategy",
    ] {
        assert!(content.contains(key), "missing key {key} in:\n{content}");
    }
}

#[test]
fn load_global_config_uses_defaults_when_file_missing() {
    with_xdg_config_home(|_xdg| {
        let config = load_global_config().unwrap();
        assert_eq!(config, Config::default_values());
    });
}

#[test]
fn load_config_strips_inline_comments_from_values() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("config");
    fs::write(
        &config_path,
        "embedding_model = /tmp/model.onnx  # ONNX path\nreranker_model = /tmp/rerank.onnx\n",
    )
    .unwrap();

    let config = load_config(&config_path).unwrap();
    assert_eq!(
        config.embedding_model.as_deref(),
        Some("/tmp/model.onnx")
    );
    assert_eq!(config.reranker_model.as_deref(), Some("/tmp/rerank.onnx"));
}