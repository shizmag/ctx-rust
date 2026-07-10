use ctx_codegraph::backend::{LanguageBackend, ParseInput, ResolverBackend};
use ctx_codegraph::index::BuildIndexOptions;
use ctx_codegraph::model::IndexState;
use ctx_codegraph::storage::{find_workspace_root, get_index_state};
use ctx_codegraph_models::{EmbeddingModel, EMBEDDING_DIM, RerankerModel};
use ctx_codegraph_resolver::{GenericLspClient, LspDefinitionResolver};
use ctx_config::Config;
use serde::Serialize;
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

impl CheckStatus {
    fn icon(self) -> &'static str {
        match self {
            Self::Ok => "✓",
            Self::Warn => "⚠",
            Self::Fail => "✗",
        }
    }

}

#[derive(Debug, Clone, Serialize)]
pub struct ComponentCheck {
    pub name: String,
    pub status: CheckStatus,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParserCheck {
    pub language: String,
    pub parser_id: String,
    pub parser_version: String,
    pub status: CheckStatus,
    pub message: String,
    pub symbols_found: usize,
    pub occurrences_found: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspCheck {
    pub language: String,
    pub resolver_id: String,
    pub command: String,
    pub status: CheckStatus,
    pub in_path: bool,
    pub version: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchCheck {
    pub component: String,
    pub status: CheckStatus,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexCheck {
    pub state: String,
    pub database_exists: bool,
    pub database_path: String,
    pub database_size_bytes: Option<u64>,
    pub files: i64,
    pub symbols: i64,
    pub edges: i64,
    pub chunks: i64,
    pub embeddings: i64,
    pub languages: Vec<(String, i64)>,
    pub edge_confidence: Vec<(String, i64)>,
    pub metadata: Vec<(String, String)>,
    pub status: CheckStatus,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigCheck {
    pub global_config_path: Option<String>,
    pub project_config_path: Option<String>,
    pub use_lsp: bool,
    pub search_configured: bool,
    pub embedding_model: Option<String>,
    pub reranker_model: Option<String>,
    pub default_retrieval_strategy: Option<String>,
    pub default_ranking: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthSummary {
    pub ok: usize,
    pub warn: usize,
    pub fail: usize,
    pub overall: CheckStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub ctx_version: String,
    pub binary_path: Option<String>,
    pub requested_path: String,
    pub workspace_root: String,
    pub elapsed_ms: u64,
    pub probe_enabled: bool,
    pub summary: HealthSummary,
    pub config: ConfigCheck,
    pub parsers: Vec<ParserCheck>,
    pub lsp: Vec<LspCheck>,
    pub index: IndexCheck,
    pub search: Vec<SearchCheck>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct HealthcheckOptions {
    pub probe: bool,
}

impl Default for HealthcheckOptions {
    fn default() -> Self {
        Self { probe: false }
    }
}

pub fn run_healthcheck(
    path: &Path,
    ctx_version: &str,
    options: HealthcheckOptions,
) -> HealthReport {
    let started = Instant::now();
    let config = ctx_config::find_and_load_config(path).unwrap_or_default();
    let workspace_root = find_workspace_root(path);
    let build_options = BuildIndexOptions {
        use_lsp: config.use_lsp.unwrap_or(false),
        ..Default::default()
    };

    let config_check = build_config_check(path, &config);
    let parsers = check_parsers();
    let lsp = check_lsp(&workspace_root, options.probe);
    let index = check_index(&workspace_root, &build_options);
    let search = check_search(&workspace_root, &config, options.probe);

    let mut components: Vec<CheckStatus> = parsers.iter().map(|p| p.status).collect();
    components.extend(lsp.iter().map(|l| l.status));
    components.push(index.status);
    components.extend(search.iter().map(|s| s.status));

    let summary = summarize(&components);
    let notes = build_notes(&config, &index, options.probe);

    HealthReport {
        ctx_version: ctx_version.to_string(),
        binary_path: std::env::current_exe()
            .ok()
            .map(|p| p.display().to_string()),
        requested_path: path.display().to_string(),
        workspace_root: workspace_root.display().to_string(),
        elapsed_ms: started.elapsed().as_millis() as u64,
        probe_enabled: options.probe,
        summary,
        config: config_check,
        parsers,
        lsp,
        index,
        search,
        notes,
    }
}

pub fn render_text(report: &HealthReport) -> String {
    let mut out = String::new();

    writeln!(out, "ctx healthcheck").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "Overview").unwrap();
    writeln!(out, "  version: {}", report.ctx_version).unwrap();
    if let Some(bin) = &report.binary_path {
        writeln!(out, "  binary: {bin}").unwrap();
    }
    writeln!(out, "  path: {}", report.requested_path).unwrap();
    writeln!(out, "  workspace: {}", report.workspace_root).unwrap();
    writeln!(
        out,
        "  probe: {}",
        if report.probe_enabled { "enabled" } else { "disabled (use --probe for live checks)" }
    )
    .unwrap();
    writeln!(out, "  elapsed: {}ms", report.elapsed_ms).unwrap();
    writeln!(
        out,
        "  overall: {} {} ({} ok, {} warn, {} fail)",
        report.summary.overall.icon(),
        status_label(report.summary.overall),
        report.summary.ok,
        report.summary.warn,
        report.summary.fail
    )
    .unwrap();
    writeln!(out).unwrap();

    writeln!(out, "Configuration").unwrap();
    if let Some(p) = &report.config.global_config_path {
        writeln!(out, "  global config: {p}").unwrap();
    } else {
        writeln!(out, "  global config: (not found)").unwrap();
    }
    if let Some(p) = &report.config.project_config_path {
        writeln!(out, "  project config: {p}").unwrap();
    }
    writeln!(out, "  use_lsp: {}", report.config.use_lsp).unwrap();
    writeln!(
        out,
        "  hybrid search configured: {}",
        report.config.search_configured
    )
    .unwrap();
    if let Some(m) = &report.config.embedding_model {
        writeln!(out, "  embedding_model: {m}").unwrap();
    }
    if let Some(m) = &report.config.reranker_model {
        writeln!(out, "  reranker_model: {m}").unwrap();
    }
    if let Some(s) = &report.config.default_retrieval_strategy {
        writeln!(out, "  retrieval_strategy: {s}").unwrap();
    }
    if let Some(r) = &report.config.default_ranking {
        writeln!(out, "  ranking: {r}").unwrap();
    }
    writeln!(out).unwrap();

    writeln!(out, "Tree-sitter parsers").unwrap();
    for parser in &report.parsers {
        writeln!(
            out,
            "  {} {} ({}) — {}",
            parser.status.icon(),
            parser.language,
            parser.parser_id,
            parser.message
        )
        .unwrap();
        writeln!(
            out,
            "      version: {}, symbols: {}, occurrences: {}",
            parser.parser_version, parser.symbols_found, parser.occurrences_found
        )
        .unwrap();
    }
    writeln!(out).unwrap();

    writeln!(out, "Language servers (LSP)").unwrap();
    for lsp in &report.lsp {
        writeln!(
            out,
            "  {} {} ({}) — {}",
            lsp.status.icon(),
            lsp.language,
            lsp.command,
            lsp.message
        )
        .unwrap();
        writeln!(out, "      in PATH: {}", lsp.in_path).unwrap();
        if let Some(v) = &lsp.version {
            writeln!(out, "      version: {v}").unwrap();
        }
        if let Some(ms) = lsp.probe_ms {
            writeln!(out, "      probe: {ms}ms").unwrap();
        }
    }
    writeln!(out).unwrap();

    writeln!(out, "Codegraph index").unwrap();
    writeln!(
        out,
        "  {} — {}",
        report.index.status.icon(),
        report.index.message
    )
    .unwrap();
    writeln!(out, "  state: {}", report.index.state).unwrap();
    writeln!(out, "  database: {}", report.index.database_path).unwrap();
    if let Some(size) = report.index.database_size_bytes {
        writeln!(out, "  size: {size} bytes").unwrap();
    }
    if report.index.database_exists {
        writeln!(out, "  files: {}", report.index.files).unwrap();
        writeln!(out, "  symbols: {}", report.index.symbols).unwrap();
        writeln!(out, "  edges: {}", report.index.edges).unwrap();
        if report.index.chunks > 0 {
            writeln!(out, "  chunks: {}", report.index.chunks).unwrap();
        }
        if report.index.embeddings > 0 {
            writeln!(out, "  embeddings: {}", report.index.embeddings).unwrap();
        }
        if !report.index.languages.is_empty() {
            writeln!(out, "  languages:").unwrap();
            for (lang, count) in &report.index.languages {
                writeln!(out, "    - {lang}: {count}").unwrap();
            }
        }
        if !report.index.edge_confidence.is_empty() {
            writeln!(out, "  edge confidence:").unwrap();
            for (conf, count) in &report.index.edge_confidence {
                writeln!(out, "    - {conf}: {count}").unwrap();
            }
        }
        if !report.index.metadata.is_empty() {
            writeln!(out, "  metadata:").unwrap();
            for (key, value) in &report.index.metadata {
                writeln!(out, "    - {key}: {value}").unwrap();
            }
        }
    }
    writeln!(out).unwrap();

    writeln!(out, "Hybrid search").unwrap();
    if report.search.is_empty() {
        writeln!(out, "  (not configured)").unwrap();
    } else {
        for item in &report.search {
            let path_suffix = item
                .path
                .as_ref()
                .map(|p| format!(" [{p}]"))
                .unwrap_or_default();
            let count_suffix = item
                .count
                .map(|c| format!(" (count: {c})"))
                .unwrap_or_default();
            writeln!(
                out,
                "  {} {} — {}{}{}",
                item.status.icon(),
                item.component,
                item.message,
                path_suffix,
                count_suffix
            )
            .unwrap();
        }
    }
    writeln!(out).unwrap();

    if !report.notes.is_empty() {
        writeln!(out, "Notes").unwrap();
        for note in &report.notes {
            writeln!(out, "  - {note}").unwrap();
        }
        writeln!(out).unwrap();
    }

    out
}

pub fn render_json(report: &HealthReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

fn status_label(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Ok => "healthy",
        CheckStatus::Warn => "degraded",
        CheckStatus::Fail => "unhealthy",
    }
}

fn summarize(statuses: &[CheckStatus]) -> HealthSummary {
    let mut ok = 0usize;
    let mut warn = 0usize;
    let mut fail = 0usize;
    for status in statuses {
        match status {
            CheckStatus::Ok => ok += 1,
            CheckStatus::Warn => warn += 1,
            CheckStatus::Fail => fail += 1,
        }
    }
    let overall = if fail > 0 {
        CheckStatus::Fail
    } else if warn > 0 {
        CheckStatus::Warn
    } else {
        CheckStatus::Ok
    };
    HealthSummary {
        ok,
        warn,
        fail,
        overall,
    }
}

fn build_config_check(path: &Path, config: &Config) -> ConfigCheck {
    ConfigCheck {
        global_config_path: ctx_config::global_config_path().map(|p| p.display().to_string()),
        project_config_path: ctx_config::find_project_config(path)
            .map(|p| p.display().to_string()),
        use_lsp: config.use_lsp.unwrap_or(false),
        search_configured: config.search_auto_enabled(),
        embedding_model: config.embedding_model.clone(),
        reranker_model: config.reranker_model.clone(),
        default_retrieval_strategy: config.default_retrieval_strategy.clone(),
        default_ranking: config.default_ranking.clone(),
    }
}

fn check_parsers() -> Vec<ParserCheck> {
    let registry = ctx_codegraph::global_registry();
    let samples: [(&str, &str); 2] = [
        ("rust", "fn health_probe() {}\n"),
        ("python", "def health_probe():\n    pass\n"),
    ];

    let mut checks = Vec::new();
    for (ext, source) in samples {
        let Some(backend) = registry.find_by_language(&ctx_codegraph::model::Language(
            ext.to_string(),
        )) else {
            checks.push(ParserCheck {
                language: ext.to_string(),
                parser_id: "unknown".into(),
                parser_version: "unknown".into(),
                status: CheckStatus::Fail,
                message: "no backend registered".into(),
                symbols_found: 0,
                occurrences_found: 0,
            });
            continue;
        };

        let probe = probe_parser(backend, ext, source);
        checks.push(probe);
    }
    checks
}

fn probe_parser(backend: &dyn LanguageBackend, ext: &str, source: &str) -> ParserCheck {
    let parser = backend.parser();
    let parser_id = parser.parser_id().0.clone();
    let parser_version = parser.parser_version();

    let temp = match tempfile::Builder::new()
        .suffix(&format!(".{ext}"))
        .tempfile()
    {
        Ok(f) => f,
        Err(e) => {
            return ParserCheck {
                language: backend.language().0.clone(),
                parser_id,
                parser_version,
                status: CheckStatus::Fail,
                message: format!("failed to create temp file: {e}"),
                symbols_found: 0,
                occurrences_found: 0,
            };
        }
    };

    if let Err(e) = std::fs::write(temp.path(), source) {
        return ParserCheck {
            language: backend.language().0.clone(),
            parser_id,
            parser_version,
            status: CheckStatus::Fail,
            message: format!("failed to write probe file: {e}"),
            symbols_found: 0,
            occurrences_found: 0,
        };
    }

    match parser.parse_file(ParseInput {
        path: temp.path(),
    }) {
        Ok(parsed) => {
            let symbols = parsed.symbols.len();
            let occurrences = parsed.occurrences.len();
            let status = if symbols > 0 {
                CheckStatus::Ok
            } else {
                CheckStatus::Warn
            };
            ParserCheck {
                language: backend.language().0.clone(),
                parser_id,
                parser_version,
                status,
                message: if symbols > 0 {
                    "parse probe succeeded".into()
                } else {
                    "parsed but found no symbols".into()
                },
                symbols_found: symbols,
                occurrences_found: occurrences,
            }
        }
        Err(e) => ParserCheck {
            language: backend.language().0.clone(),
            parser_id,
            parser_version,
            status: CheckStatus::Fail,
            message: format!("parse failed: {e}"),
            symbols_found: 0,
            occurrences_found: 0,
        },
    }
}

fn check_lsp(workspace_root: &Path, probe: bool) -> Vec<LspCheck> {
    let resolvers = [
        ("Rust", LspDefinitionResolver::rust()),
        ("Python", LspDefinitionResolver::python()),
    ];

    resolvers
        .into_iter()
        .map(|(language, resolver)| probe_lsp(language, &resolver, workspace_root, probe))
        .collect()
}

fn probe_lsp(
    language: &str,
    resolver: &LspDefinitionResolver,
    workspace_root: &Path,
    probe: bool,
) -> LspCheck {
    let resolver_id = resolver.resolver_id().0.clone();
    let command = match language {
        "Rust" => "rust-analyzer",
        "Python" => "pyright-langserver",
        _ => "unknown",
    }
    .to_string();

    let in_path = command_in_path(&command);
    let version = command_version(&command);

    let (status, message, probe_ms) = if !in_path {
        (
            CheckStatus::Warn,
            format!("{command} not found in PATH; tree-sitter fallback will be used"),
            None,
        )
    } else if !probe {
        (
            CheckStatus::Ok,
            format!("{command} found in PATH"),
            None,
        )
    } else {
        let started = Instant::now();
        match GenericLspClient::new(workspace_root, &command, lsp_args(language)) {
            Ok(_client) => (
                CheckStatus::Ok,
                format!("{command} initialized successfully"),
                Some(started.elapsed().as_millis() as u64),
            ),
            Err(err) => (
                CheckStatus::Warn,
                format!("{command} found but probe failed: {err}"),
                Some(started.elapsed().as_millis() as u64),
            ),
        }
    };

    LspCheck {
        language: language.to_string(),
        resolver_id,
        command,
        status,
        in_path,
        version,
        message,
        probe_ms,
    }
}

fn lsp_args(language: &str) -> &[&str] {
    match language {
        "Python" => &["--stdio"],
        _ => &[],
    }
}

fn command_in_path(command: &str) -> bool {
    Command::new("which")
        .arg(command)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn command_version(command: &str) -> Option<String> {
    let output = Command::new(command).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next()?.trim();
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

fn check_index(workspace_root: &Path, options: &BuildIndexOptions) -> IndexCheck {
    let db_path = workspace_root.join(".ctx-codegraph/codegraph.sqlite");
    let state = get_index_state(workspace_root, options).unwrap_or(IndexState::Missing);
    let state_label = format_index_state(&state);

    let mut languages = Vec::new();
    let mut edge_confidence = Vec::new();
    let mut metadata = Vec::new();
    let mut file_count = 0i64;
    let mut symbol_count = 0i64;
    let mut edge_count = 0i64;
    let mut chunk_count = 0i64;
    let mut embedding_count = 0i64;
    let mut db_size_bytes = None;

    if db_path.exists() {
        if let Ok(meta) = std::fs::metadata(&db_path) {
            db_size_bytes = Some(meta.len());
        }
        if let Ok(conn) = ctx_codegraph::open_db(workspace_root) {
            file_count = query_count(&conn, "SELECT COUNT(*) FROM files");
            symbol_count = query_count(&conn, "SELECT COUNT(*) FROM symbols");
            edge_count = query_count(&conn, "SELECT COUNT(*) FROM edges");
            if table_exists(&conn, "chunks") {
                chunk_count = query_count(&conn, "SELECT COUNT(*) FROM chunks");
            }
            if table_exists(&conn, "chunk_embeddings") {
                embedding_count = query_count(&conn, "SELECT COUNT(*) FROM chunk_embeddings");
            }

            if let Ok(mut stmt) = conn.prepare(
                "SELECT language, COUNT(*) FROM files GROUP BY language ORDER BY COUNT(*) DESC",
            ) {
                if let Ok(rows) = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                }) {
                    for row in rows.flatten() {
                        languages.push(row);
                    }
                }
            }

            if let Ok(mut stmt) = conn.prepare(
                "SELECT confidence, COUNT(*) FROM edges GROUP BY confidence ORDER BY COUNT(*) DESC",
            ) {
                if let Ok(rows) = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                }) {
                    for row in rows.flatten() {
                        edge_confidence.push(row);
                    }
                }
            }

            if let Ok(mut stmt) = conn.prepare("SELECT key, value FROM metadata ORDER BY key") {
                if let Ok(rows) = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                }) {
                    for row in rows.flatten() {
                        metadata.push(row);
                    }
                }
            }
        }
    }

    let (status, message) = match &state {
        IndexState::Ready => (CheckStatus::Ok, "index is ready".to_string()),
        IndexState::Missing => (
            CheckStatus::Warn,
            "no index found; run `ctx graph build`".to_string(),
        ),
        IndexState::NeedsIncrementalUpdate(diff) => (
            CheckStatus::Warn,
            format!(
                "index stale (+{} ~{} -{} files); run `ctx graph build`",
                diff.added.len(),
                diff.modified.len(),
                diff.deleted.len()
            ),
        ),
        IndexState::NeedsFullRebuild(reason) => (
            CheckStatus::Warn,
            format!("index needs rebuild ({reason:?})"),
        ),
    };

    IndexCheck {
        state: state_label,
        database_exists: db_path.exists(),
        database_path: db_path.display().to_string(),
        database_size_bytes: db_size_bytes,
        files: file_count,
        symbols: symbol_count,
        edges: edge_count,
        chunks: chunk_count,
        embeddings: embedding_count,
        languages,
        edge_confidence,
        metadata,
        status,
        message,
    }
}

fn check_search(workspace_root: &Path, config: &Config, probe: bool) -> Vec<SearchCheck> {
    if !config.search_auto_enabled() {
        return vec![SearchCheck {
            component: "hybrid search".into(),
            status: CheckStatus::Warn,
            message: "not configured; set embedding_model in config to enable".into(),
            path: None,
            count: None,
        }];
    }

    let mut checks = Vec::new();
    let embedding_path = config.resolved_embedding_model();
    let embedding_tokenizer = embedding_path
        .as_ref()
        .map(|p| config.resolved_embedding_tokenizer(p));

    if let Some(model_path) = embedding_path {
        let exists = model_path.is_file();
        checks.push(SearchCheck {
            component: "embedding model".into(),
            status: if exists {
                CheckStatus::Ok
            } else {
                CheckStatus::Fail
            },
            message: if exists {
                "ONNX model file found".into()
            } else {
                "ONNX model file missing".into()
            },
            path: Some(model_path.display().to_string()),
            count: None,
        });

        if let Some(tokenizer_dir) = embedding_tokenizer {
            let tokenizer_file = tokenizer_dir.join("tokenizer.json");
            checks.push(SearchCheck {
                component: "embedding tokenizer".into(),
                status: if tokenizer_file.exists() {
                    CheckStatus::Ok
                } else {
                    CheckStatus::Fail
                },
                message: if tokenizer_file.exists() {
                    "tokenizer.json found".into()
                } else {
                    "tokenizer.json missing".into()
                },
                path: Some(tokenizer_dir.display().to_string()),
                count: None,
            });
        }

        if probe && exists {
            let tokenizer_dir = config.resolved_embedding_tokenizer(&model_path);
            let started = Instant::now();
            match EmbeddingModel::load(&model_path, &tokenizer_dir) {
                Ok(mut model) => match model.embed_texts(&["ctx health probe".to_string()]) {
                    Ok(vectors) if vectors.len() == 1 && vectors[0].len() == EMBEDDING_DIM => {
                        checks.push(SearchCheck {
                            component: "embedding inference".into(),
                            status: CheckStatus::Ok,
                            message: format!(
                                "probe embedding succeeded in {}ms",
                                started.elapsed().as_millis()
                            ),
                            path: None,
                            count: Some(EMBEDDING_DIM as u64),
                        });
                    }
                    Ok(vectors) => {
                        checks.push(SearchCheck {
                            component: "embedding inference".into(),
                            status: CheckStatus::Fail,
                            message: format!(
                                "unexpected embedding output: {} vectors",
                                vectors.len()
                            ),
                            path: None,
                            count: None,
                        });
                    }
                    Err(err) => {
                        checks.push(SearchCheck {
                            component: "embedding inference".into(),
                            status: CheckStatus::Fail,
                            message: format!("inference failed: {err}"),
                            path: None,
                            count: None,
                        });
                    }
                },
                Err(err) => {
                    checks.push(SearchCheck {
                        component: "embedding inference".into(),
                        status: CheckStatus::Fail,
                        message: format!("model load failed: {err}"),
                        path: None,
                        count: None,
                    });
                }
            }
        }
    }

    if let Some(reranker_path) = config.resolved_reranker_model() {
        let exists = reranker_path.exists();
        checks.push(SearchCheck {
            component: "reranker model".into(),
            status: if exists {
                CheckStatus::Ok
            } else {
                CheckStatus::Warn
            },
            message: if exists {
                "ONNX reranker found".into()
            } else {
                "reranker configured but file missing".into()
            },
            path: Some(reranker_path.display().to_string()),
            count: None,
        });

        if probe && exists && config.enable_rerank.unwrap_or(false) {
            let tokenizer_dir = config.resolved_rerank_tokenizer(&reranker_path);
            match RerankerModel::load(&reranker_path, &tokenizer_dir) {
                Ok(mut model) => {
                    match model.score_pairs("health probe", &["fn main() {}".to_string()]) {
                        Ok(scores) if scores.len() == 1 => {
                            checks.push(SearchCheck {
                                component: "reranker inference".into(),
                                status: CheckStatus::Ok,
                                message: "probe rerank succeeded".into(),
                                path: None,
                                count: None,
                            });
                        }
                        Ok(scores) => {
                            checks.push(SearchCheck {
                                component: "reranker inference".into(),
                                status: CheckStatus::Fail,
                                message: format!("unexpected score count: {}", scores.len()),
                                path: None,
                                count: None,
                            });
                        }
                        Err(err) => {
                            checks.push(SearchCheck {
                                component: "reranker inference".into(),
                                status: CheckStatus::Fail,
                                message: format!("rerank failed: {err}"),
                                path: None,
                                count: None,
                            });
                        }
                    }
                }
                Err(err) => {
                    checks.push(SearchCheck {
                        component: "reranker inference".into(),
                        status: CheckStatus::Fail,
                        message: format!("reranker load failed: {err}"),
                        path: None,
                        count: None,
                    });
                }
            }
        }
    }

    let lexical_path = workspace_root.join(".ctx-codegraph/lexical");
    let lexical_meta = lexical_path.join("meta.json");
    checks.push(SearchCheck {
        component: "lexical index (BM25)".into(),
        status: if lexical_meta.exists() {
            CheckStatus::Ok
        } else {
            CheckStatus::Warn
        },
        message: if lexical_meta.exists() {
            "Tantivy index present".into()
        } else {
            "index missing; run `ctx graph build` with embeddings enabled".into()
        },
        path: Some(lexical_path.display().to_string()),
        count: None,
    });

    let dense_path = workspace_root.join(".ctx-codegraph/dense.sqlite");
    let dense_count = dense_embedding_count(&dense_path);
    checks.push(SearchCheck {
        component: "dense index".into(),
        status: if dense_count > 0 {
            CheckStatus::Ok
        } else if dense_path.exists() {
            CheckStatus::Warn
        } else {
            CheckStatus::Warn
        },
        message: if dense_count > 0 {
            format!("{dense_count} embeddings indexed")
        } else if dense_path.exists() {
            "dense DB exists but has no embeddings".into()
        } else {
            "dense index missing; run `ctx graph build` with embeddings enabled".into()
        },
        path: Some(dense_path.display().to_string()),
        count: Some(dense_count),
    });

    if probe {
        match ctx_codegraph::WorkspaceHybridBackend::try_with_config(workspace_root, config) {
            Ok(Some(_backend)) => {
                checks.push(SearchCheck {
                    component: "hybrid backend".into(),
                    status: CheckStatus::Ok,
                    message: "WorkspaceHybridBackend initialized".into(),
                    path: None,
                    count: None,
                });
            }
            Ok(None) => {
                checks.push(SearchCheck {
                    component: "hybrid backend".into(),
                    status: CheckStatus::Warn,
                    message: "hybrid backend not available (search not configured)".into(),
                    path: None,
                    count: None,
                });
            }
            Err(err) => {
                checks.push(SearchCheck {
                    component: "hybrid backend".into(),
                    status: CheckStatus::Fail,
                    message: format!("backend init failed: {err}"),
                    path: None,
                    count: None,
                });
            }
        }
    }

    checks
}

fn dense_embedding_count(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    let Ok(conn) = rusqlite::Connection::open(path) else {
        return 0;
    };
    query_count(&conn, "SELECT COUNT(*) FROM chunk_embeddings").max(0) as u64
}

fn build_notes(config: &Config, index: &IndexCheck, probe: bool) -> Vec<String> {
    let mut notes = Vec::new();

    if !probe {
        notes.push(
            "Run with --probe to test LSP initialization, ONNX inference, and hybrid backend wiring."
                .into(),
        );
    }

    if !index.database_exists {
        notes.push("Build the codegraph with `ctx graph build` before graph queries.".into());
    }

    if config.search_auto_enabled() && index.chunks == 0 {
        notes.push(
            "Hybrid search is configured but no chunks are indexed; rebuild with embeddings."
                .into(),
        );
    }

    if config.use_lsp.unwrap_or(false) {
        notes.push("LSP enrichment is enabled; ensure language servers are installed.".into());
    } else {
        notes.push("LSP enrichment is disabled; only tree-sitter resolution will be used.".into());
    }

    notes.push("Use `ctx graph info` for detailed index metadata.".into());
    notes.push("Use `ctx stats` for project scan totals and MCP session stats.".into());

    notes
}

fn format_index_state(state: &IndexState) -> String {
    match state {
        IndexState::Missing => "missing".to_string(),
        IndexState::Ready => "ready".to_string(),
        IndexState::NeedsIncrementalUpdate(diff) => format!(
            "stale (+{} added, ~{} modified, -{} deleted)",
            diff.added.len(),
            diff.modified.len(),
            diff.deleted.len()
        ),
        IndexState::NeedsFullRebuild(reason) => format!("needs rebuild ({reason:?})"),
    }
}

fn query_count(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).unwrap_or(0)
}

fn table_exists(conn: &rusqlite::Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn tree_sitter_probes_succeed_for_rust_and_python() {
        let checks = check_parsers();
        assert_eq!(checks.len(), 2);

        for check in &checks {
            assert_eq!(
                check.status,
                CheckStatus::Ok,
                "{} parser failed: {}",
                check.language,
                check.message
            );
            assert!(check.symbols_found > 0, "{}", check.language);
        }
    }

    #[test]
    fn healthcheck_runs_in_temp_workspace() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();

        let report = run_healthcheck(dir.path(), "0.0-test", HealthcheckOptions::default());
        assert!(!report.parsers.is_empty());
        assert_eq!(report.parsers.len(), 2);
        assert!(report.summary.ok > 0);
        assert!(render_text(&report).contains("Tree-sitter parsers"));
    }

    #[test]
    fn json_output_is_valid() {
        let dir = tempfile::tempdir().unwrap();
        let report = run_healthcheck(dir.path(), "0.0-test", HealthcheckOptions::default());
        let json = render_json(&report).expect("json");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse json");
        assert!(parsed.get("parsers").is_some());
        assert!(parsed.get("lsp").is_some());
        assert!(parsed.get("search").is_some());
    }

    #[test]
    #[ignore = "requires local ONNX models; set CTX_TEST_MODELS=1 to run"]
    fn healthcheck_dense_index_ok_after_ready_embedding_build() {
        use ctx_codegraph::BuildIndexOptions;
        use ctx_codegraph::storage::rebuild_index_db;
        use ctx_codegraph_models::ModelPaths;

        if std::env::var("CTX_TEST_MODELS").ok().as_deref() != Some("1") {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let paths = ModelPaths::default_paths();
        if !paths.embedding_onnx.is_file() {
            eprintln!("skipping: embedding model missing");
            return;
        }
        if EmbeddingModel::load(&paths.embedding_onnx, &paths.embedding_tokenizer).is_err() {
            eprintln!("skipping: embedding model not loadable in this environment");
            return;
        }

        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"health_dense\"\nversion=\"0.1.0\"\nedition=\"2021\"",
        )
        .unwrap();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("lib.rs"), "pub fn greet() {}\n").unwrap();
        let model_dir = paths
            .embedding_onnx
            .parent()
            .expect("embedding model parent dir");
        fs::write(
            root.join(".ctxconfig"),
            format!(
                "embedding_model = {}\nembedding_tokenizer = {}\n",
                model_dir.display(),
                paths.embedding_tokenizer.display()
            ),
        )
        .unwrap();

        rebuild_index_db(
            root,
            BuildIndexOptions {
                with_lexical: Some(false),
                with_embeddings: Some(false),
                ..Default::default()
            },
        )
        .unwrap();

        rebuild_index_db(
            root,
            BuildIndexOptions {
                with_lexical: Some(true),
                with_embeddings: Some(true),
                ..Default::default()
            },
        )
        .unwrap();

        let report = run_healthcheck(root, "0.0-test", HealthcheckOptions::default());
        let dense = report
            .search
            .iter()
            .find(|c| c.component == "dense index")
            .expect("dense index check");
        assert_eq!(
            dense.status,
            CheckStatus::Ok,
            "expected dense index ok, got: {}",
            dense.message
        );
        assert!(dense.count.unwrap_or(0) > 0);
    }
}