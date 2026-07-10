use clap::Parser;
use ctx_models::{Mode, ScanOptions};
use ctx_render::{Format, RenderOptions};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
#[value(rename_all = "lowercase")]
pub enum CliMode {
    Smart,
    All,
    Code,
    Docs,
    Llm,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
#[value(rename_all = "lowercase")]
pub enum CliFormat {
    #[value(alias = "md")]
    Markdown,
    Xml,
    #[value(alias = "txt", alias = "text")]
    Plain,
}

impl From<CliMode> for Mode {
    fn from(mode: CliMode) -> Self {
        match mode {
            CliMode::Smart => Mode::Smart,
            CliMode::All => Mode::All,
            CliMode::Code => Mode::Code,
            CliMode::Docs => Mode::Docs,
            CliMode::Llm => Mode::Llm,
        }
    }
}

impl From<CliFormat> for Format {
    fn from(format: CliFormat) -> Self {
        match format {
            CliFormat::Markdown => Format::Markdown,
            CliFormat::Xml => Format::Xml,
            CliFormat::Plain => Format::Plain,
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "ctx",
    version,
    about = "✨ ctx: A highly informative, interactive directory tree visualizer and LLM context gatherer.\n\nRuns a beautiful, interactive TUI or outputs detailed markdown/plain/xml context for your files."
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Target directory path to analyze.
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Format for the full context output. Choose from: 'markdown' (or 'md'), 'xml', 'plain' (or 'text', 'txt').
    #[arg(short, long)]
    format: Option<CliFormat>,

    /// Gathering strategy mode: 'smart' (respects gitignore + sensible skips), 'all' (scans all files), 'code' (prioritizes code files), 'docs' (prioritizes docs/markdown), 'llm' (structures with token counts).
    #[arg(short, long)]
    mode: Option<CliMode>,

    /// Restrict directory traversal to the specified maximum depth.
    #[arg(long)]
    max_depth: Option<usize>,

    /// Exclude files exceeding this size limit in bytes from the final context contents.
    #[arg(long)]
    max_file_size: Option<u64>,

    /// Save the compiled context output directly to the specified file path instead of printing to stdout.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Exclude the project summary tables and statistics from the generated context output.
    #[arg(long)]
    no_stats: bool,

    /// Print lists of skipped, gitignored, or hidden files to stderr for transparency.
    #[arg(long)]
    list_hidden: bool,

    /// Copy the fully compiled context output straight to the system clipboard.
    #[arg(short, long)]
    clipboard: bool,

    /// Output the full code context (file structure and contents) to stdout instead of only showing the colored directory tree.
    #[arg(short = 'C', long)]
    code: bool,

    /// Launch the interactive, keyboard-driven terminal user interface (TUI) for selecting files.
    #[arg(short, long)]
    interactive: bool,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if let Some(cmd) = args.command {
        match cmd {
            Command::Graph(g) => return handle_graph_command(g),
            Command::Mcp(mcp_cmd) => match mcp_cmd {
                McpCommand::Serve => {
                    if let Err(e) = ctx_mcp::run_mcp_server() {
                        eprintln!("MCP Server Error: {}", e);
                        std::process::exit(1);
                    }
                    return Ok(());
                }
                McpCommand::Install(install) => {
                    return handle_mcp_install(install);
                }
            },
            Command::Setting(s) => {
                return ctx_tui::run_settings_editor(s.path).map_err(Into::into);
            }
            Command::Stats(s) => {
                return handle_stats_command(s);
            }
            Command::Healthcheck(h) => {
                return handle_healthcheck_command(h);
            }
        }
    }
    run_with_args(args, ctx_tui::run_interactive)
}

fn run_with_args<F, E>(args: Args, run_tui: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnOnce(PathBuf) -> Result<(), E>,
    E: Into<Box<dyn std::error::Error>>,
{
    if args.interactive {
        return run_tui(args.path).map_err(Into::into);
    }

    let config = ctx_config::find_and_load_config(&args.path).unwrap_or_default();

    let mode = args
        .mode
        .map(Mode::from)
        .or(config.mode)
        .unwrap_or(Mode::Smart);

    let format = args
        .format
        .map(Format::from)
        .or_else(|| {
            config
                .default_format
                .as_ref()
                .and_then(|f| match f.to_lowercase().as_str() {
                    "markdown" | "md" => Some(Format::Markdown),
                    "xml" => Some(Format::Xml),
                    "plain" | "text" | "txt" => Some(Format::Plain),
                    _ => None, // e.g. yaml is for agent context tools, not project render here
                })
        })
        .unwrap_or(Format::Markdown);

    let max_depth = args.max_depth.or(config.max_depth);

    let max_file_size = args
        .max_file_size
        .or(config.max_file_size)
        .unwrap_or(512 * 1024);

    let exclude = config.exclude;

    let scan_options = ScanOptions {
        max_depth,
        max_file_size,
        mode,
        exclude,
    };

    let scan_result = ctx_core::scan(&args.path, scan_options)?;

    let is_ordinary_call = !args.code && !args.clipboard && args.output.is_none();

    if is_ordinary_call {
        let colored_tree = ctx_render::render_colored_tree(&scan_result)?;
        print!("{}", colored_tree);
    } else {
        let render_options = RenderOptions {
            format,
            include_stats: !args.no_stats,
            max_file_size,
        };

        let rendered = ctx_render::render(&scan_result, &render_options)?;

        if args.clipboard {
            let mut ctx_clipboard = arboard::Clipboard::new()?;
            ctx_clipboard.set_text(rendered)?;
            println!(
                "\x1b[1;38;2;158;206;106m✨ Context successfully copied to clipboard!\x1b[0m \x1b[38;2;86;95;137m({} files, {} tokens)\x1b[0m",
                scan_result.summary.files, scan_result.summary.tokens
            );
        } else if let Some(output_path) = args.output {
            fs::write(&output_path, rendered)?;
            println!("Context saved to {}", output_path.display());
        } else {
            print!("{}", rendered);
        }
    }

    if args.list_hidden {
        eprintln!("\nHidden/Skipped items ({}):", scan_result.hidden.len());
        for item in &scan_result.hidden {
            eprintln!(
                "  [{}] {} - {}",
                if item.is_dir { "Dir" } else { "File" },
                item.path.display(),
                item.reason.label()
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_codegraph::model::{IndexState, RebuildReason};

    fn base_args(path: PathBuf) -> Args {
        Args {
            command: None,
            path,
            format: None,
            mode: None,
            max_depth: None,
            max_file_size: None,
            output: None,
            no_stats: false,
            list_hidden: false,
            clipboard: false,
            code: false,
            interactive: false,
        }
    }

    #[test]
    fn test_cli_passes_path_to_tui() {
        let args = base_args(PathBuf::from("/mock/path/to/project"));
        let args = Args {
            interactive: true,
            ..args
        };

        let mut path_called = None;
        let mock_run_tui = |path: PathBuf| -> Result<(), Box<dyn std::error::Error>> {
            path_called = Some(path);
            Ok(())
        };

        let res = run_with_args(args, mock_run_tui);
        assert!(res.is_ok());
        assert_eq!(path_called, Some(PathBuf::from("/mock/path/to/project")));
    }

    #[test]
    fn test_cli_code_mode_renders_markdown_by_default() {
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_path = temp_dir.path().to_path_buf();
        fs::write(temp_path.join("main.rs"), "fn main() {}\n").unwrap();

        let args = Args {
            code: true,
            ..base_args(temp_path)
        };

        let res = run_with_args(args, |_| Ok::<(), Box<dyn std::error::Error>>(()));
        assert!(res.is_ok());
    }

    #[test]
    fn test_cli_writes_output_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_path = temp_dir.path().to_path_buf();
        fs::write(temp_path.join("main.rs"), "fn main() {}\n").unwrap();
        let output_path = temp_path.join("out.md");

        let args = Args {
            code: true,
            output: Some(output_path.clone()),
            ..base_args(temp_path)
        };

        let res = run_with_args(args, |_| Ok::<(), Box<dyn std::error::Error>>(()));
        assert!(res.is_ok());
        let written = fs::read_to_string(output_path).unwrap();
        assert!(written.contains("main.rs"));
    }

    #[test]
    fn test_cli_xml_format_and_list_hidden() {
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_path = temp_dir.path().to_path_buf();
        fs::write(temp_path.join("main.rs"), "fn main() {}\n").unwrap();
        fs::create_dir_all(temp_path.join("target")).unwrap();
        fs::write(temp_path.join("target/ignored.rs"), "fn ignored() {}\n").unwrap();

        let args = Args {
            code: true,
            format: Some(CliFormat::Xml),
            list_hidden: true,
            ..base_args(temp_path)
        };

        let res = run_with_args(args, |_| Ok::<(), Box<dyn std::error::Error>>(()));
        assert!(res.is_ok());
    }

    #[test]
    fn test_format_index_state_labels() {
        assert_eq!(format_index_state(&IndexState::Missing), "missing");
        assert_eq!(format_index_state(&IndexState::Ready), "ready");
        assert!(format_index_state(&IndexState::NeedsIncrementalUpdate(
            ctx_codegraph::model::IndexDiff {
                added: vec![ctx_codegraph::model::FileSnapshot {
                    file_id: None,
                    rel_path: PathBuf::from("a.rs"),
                    abs_path: PathBuf::from("/tmp/a.rs"),
                    language: ctx_codegraph::LanguageId::rust(),
                    backend_id: ctx_codegraph::BackendId::new("rust-backend"),
                    size_bytes: 1,
                    mtime_ms: 1,
                    mtime_ns: None,
                    content_hash: None,
                    parser_id: ctx_codegraph::ParserId::new("rust-parser"),
                    parser_version: "1".to_string(),
                    parser_config_hash: "x".to_string(),
                    indexed_at_ms: None,
                    parse_status: ctx_codegraph::model::FileParseStatus::Success,
                }],
                modified: vec![],
                deleted: vec![],
                unchanged: vec![],
            }
        ))
        .contains("stale"));
        assert!(format_index_state(&IndexState::NeedsFullRebuild(
            RebuildReason::SchemaVersionChanged
        ))
        .contains("schema version changed"));
    }

    #[test]
    fn test_graph_info_hints_for_ready_and_missing() {
        let ready = graph_info_hints(&IndexState::Ready, true);
        assert!(ready.iter().any(|h| h.contains("symbols")));

        let missing = graph_info_hints(&IndexState::Missing, false);
        assert!(missing.iter().any(|h| h.contains("graph build")));
    }

    #[test]
    fn test_get_markdown_lang_and_kind_to_str() {
        assert_eq!(get_markdown_lang(Path::new("foo.rs")), "rust");
        assert_eq!(get_markdown_lang(Path::new("foo.unknown")), "");
        assert_eq!(
            kind_to_str(ctx_codegraph::LanguageObjectKind::Function),
            "fn"
        );
        assert_eq!(
            kind_to_str(ctx_codegraph::LanguageObjectKind::Unknown),
            "unknown"
        );
    }

    #[test]
    fn test_get_file_span_content_bounds() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file = temp_dir.path().join("sample.txt");
        fs::write(&file, "line1\nline2\nline3\n").unwrap();

        let content = get_file_span_content(&file, 1, 2).unwrap();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
        assert!(!content.contains("line3"));

        assert_eq!(get_file_span_content(&file, 0, 1).unwrap(), "");
        assert_eq!(get_file_span_content(&file, 10, 12).unwrap(), "");
    }

    #[test]
    fn test_cli_mode_and_format_conversions() {
        assert_eq!(Mode::from(CliMode::Smart), Mode::Smart);
        assert_eq!(Mode::from(CliMode::All), Mode::All);
        assert_eq!(Mode::from(CliMode::Code), Mode::Code);
        assert_eq!(Mode::from(CliMode::Docs), Mode::Docs);
        assert_eq!(Mode::from(CliMode::Llm), Mode::Llm);

        assert_eq!(Format::from(CliFormat::Markdown), Format::Markdown);
        assert_eq!(Format::from(CliFormat::Xml), Format::Xml);
        assert_eq!(Format::from(CliFormat::Plain), Format::Plain);
    }

    #[test]
    fn test_cli_graph_context_mode_conversion() {
        use ctx_codegraph::GraphContextMode;
        assert_eq!(
            GraphContextMode::from(CliGraphContextMode::Callers),
            GraphContextMode::Callers
        );
        assert_eq!(
            GraphContextMode::from(CliGraphContextMode::Callees),
            GraphContextMode::Callees
        );
        assert_eq!(
            GraphContextMode::from(CliGraphContextMode::Dependencies),
            GraphContextMode::Dependencies
        );
        assert_eq!(
            GraphContextMode::from(CliGraphContextMode::Dependents),
            GraphContextMode::Dependents
        );
        assert_eq!(
            GraphContextMode::from(CliGraphContextMode::ForwardSlice),
            GraphContextMode::ForwardSlice
        );
        assert_eq!(
            GraphContextMode::from(CliGraphContextMode::ReverseSlice),
            GraphContextMode::ReverseSlice
        );
        assert_eq!(
            GraphContextMode::from(CliGraphContextMode::Neighborhood),
            GraphContextMode::Neighborhood
        );
    }

    #[test]
    fn test_format_index_state_all_rebuild_reason_labels() {
        use ctx_codegraph::model::{IndexState, RebuildReason};
        let reasons = [
            RebuildReason::MissingDatabase,
            RebuildReason::CorruptDatabase,
            RebuildReason::IndexerVersionChanged,
            RebuildReason::BackendSetChanged,
            RebuildReason::BackendVersionChanged,
            RebuildReason::ParserVersionChanged,
            RebuildReason::ParserConfigChanged,
            RebuildReason::ResolverVersionChanged,
            RebuildReason::ResolverConfigChanged,
            RebuildReason::DiscoveryConfigChanged,
            RebuildReason::ChangeDetectionStrategyChanged,
            RebuildReason::PreviousRunIncomplete,
            RebuildReason::PreviousRunFailed,
            RebuildReason::EmbeddingModelChanged,
            RebuildReason::LexicalIndexStale,
            RebuildReason::ChunkSchemaChanged,
        ];
        for reason in reasons {
            let label = format_index_state(&IndexState::NeedsFullRebuild(reason));
            assert!(label.contains("needs rebuild"));
        }
    }

    #[test]
    fn test_graph_info_hints_incremental_and_missing_db_paths() {
        let incremental = graph_info_hints(
            &ctx_codegraph::model::IndexState::NeedsIncrementalUpdate(
                ctx_codegraph::model::IndexDiff {
                    added: vec![],
                    modified: vec![ctx_codegraph::model::FileSnapshot {
                        file_id: None,
                        rel_path: PathBuf::from("a.rs"),
                        abs_path: PathBuf::from("/tmp/a.rs"),
                        language: ctx_codegraph::LanguageId::rust(),
                        backend_id: ctx_codegraph::BackendId::new("rust-backend"),
                        size_bytes: 1,
                        mtime_ms: 1,
                        mtime_ns: None,
                        content_hash: None,
                        parser_id: ctx_codegraph::ParserId::new("rust-parser"),
                        parser_version: "1".to_string(),
                        parser_config_hash: "x".to_string(),
                        indexed_at_ms: None,
                        parse_status: ctx_codegraph::model::FileParseStatus::Success,
                    }],
                    deleted: vec![],
                    unchanged: vec![],
                },
            ),
            true,
        );
        assert!(incremental.iter().any(|h| h.contains("refresh changed files")));

        use ctx_codegraph::model::IndexState;
        let missing_no_db = graph_info_hints(&IndexState::Missing, false);
        assert!(missing_no_db.iter().any(|h| h.contains("graph build")));
    }

    #[test]
    fn test_kind_to_str_all_variants() {
        use ctx_codegraph::LanguageObjectKind;
        assert_eq!(kind_to_str(LanguageObjectKind::Method), "fn");
        assert_eq!(kind_to_str(LanguageObjectKind::Struct), "struct");
        assert_eq!(kind_to_str(LanguageObjectKind::Enum), "enum");
        assert_eq!(kind_to_str(LanguageObjectKind::Trait), "trait");
        assert_eq!(kind_to_str(LanguageObjectKind::Impl), "impl");
        assert_eq!(kind_to_str(LanguageObjectKind::Module), "mod");
        assert_eq!(kind_to_str(LanguageObjectKind::Class), "class");
        assert_eq!(kind_to_str(LanguageObjectKind::Interface), "interface");
        assert_eq!(kind_to_str(LanguageObjectKind::TypeAlias), "type");
        assert_eq!(kind_to_str(LanguageObjectKind::Constant), "const");
        assert_eq!(kind_to_str(LanguageObjectKind::Variable), "var");
    }

    #[test]
    fn test_get_markdown_lang_common_extensions() {
        assert_eq!(get_markdown_lang(Path::new("app.py")), "python");
        assert_eq!(get_markdown_lang(Path::new("app.js")), "javascript");
        assert_eq!(get_markdown_lang(Path::new("app.ts")), "typescript");
        assert_eq!(get_markdown_lang(Path::new("app.go")), "go");
        assert_eq!(get_markdown_lang(Path::new("app.java")), "java");
        assert_eq!(get_markdown_lang(Path::new("README.md")), "markdown");
        assert_eq!(get_markdown_lang(Path::new("config.yaml")), "yaml");
    }

    #[test]
    fn test_query_count_and_table_exists_helpers() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE sample (id INTEGER)", []).unwrap();
        conn.execute("INSERT INTO sample (id) VALUES (1), (2), (3)", [])
            .unwrap();

        assert_eq!(query_count(&conn, "SELECT COUNT(*) FROM sample"), 3);
        assert!(table_exists(&conn, "sample"));
        assert!(!table_exists(&conn, "missing_table"));
    }

    #[test]
    fn test_print_slice_tree_helper_cycle_and_truncation() {
        use ctx_codegraph::model::{
            CodeIndex, EdgeKind, GraphEdge, Language, ResolutionConfidence, Symbol, SymbolId,
            SymbolKind, TextRange,
        };

        let root = PathBuf::from("/proj");
        let file = root.join("lib.rs");
        let sym_a = Symbol {
            id: Some(SymbolId(1)),
            file_id: None,
            name: "a".to_string(),
            qualified_name: "a".to_string(),
            kind: SymbolKind::Function,
            language: Language::rust(),
            file: file.clone(),
            range: TextRange {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            },
            body_range: None,
        };
        let sym_b = Symbol {
            id: Some(SymbolId(2)),
            file_id: None,
            name: "b".to_string(),
            qualified_name: "b".to_string(),
            kind: SymbolKind::Function,
            language: Language::rust(),
            file: file.clone(),
            range: TextRange {
                start_line: 2,
                start_col: 1,
                end_line: 2,
                end_col: 1,
            },
            body_range: None,
        };
        let index = CodeIndex {
            root,
            files: vec![],
            symbols: vec![sym_a, sym_b],
            occurrences: vec![],
            call_sites: vec![],
            edges: vec![
                GraphEdge {
                    id: None,
                    kind: EdgeKind::Call,
                    from_file_id: None,
                    from_symbol_id: Some(SymbolId(1)),
                    to_symbol_id: Some(SymbolId(2)),
                    to_external: None,
                    occurrence_id: None,
                    raw_text: None,
                    range: None,
                    confidence: ResolutionConfidence::Syntax,
                    produced_by: None,
                },
                GraphEdge {
                    id: None,
                    kind: EdgeKind::Call,
                    from_file_id: None,
                    from_symbol_id: Some(SymbolId(2)),
                    to_symbol_id: Some(SymbolId(1)),
                    to_external: None,
                    occurrence_id: None,
                    raw_text: None,
                    range: None,
                    confidence: ResolutionConfidence::Syntax,
                    produced_by: None,
                },
            ],
        };

        let mut visited = HashSet::new();
        visited.insert(SymbolId(1));
        let mut printed_count = 0;
        print_slice_tree_helper(&index, SymbolId(1), 0, 5, &mut visited, &mut printed_count);
        assert!(printed_count >= 2);
    }

    #[test]
    fn test_write_mcp_entry_dry_run_and_unchanged() {
        let temp_dir = tempfile::tempdir().unwrap();
        let target = temp_dir.path().join("mcp.json");

        let changed = write_mcp_entry(
            &target,
            "/usr/local/bin/ctx",
            true,
            "Test Client",
            "mcpServers",
            false,
        )
        .unwrap();
        assert!(changed);
        assert!(!target.exists(), "dry-run must not write files");

        let changed = write_mcp_entry(
            &target,
            "/usr/local/bin/ctx",
            false,
            "Test Client",
            "mcpServers",
            false,
        )
        .unwrap();
        assert!(changed);
        assert!(target.exists());

        let unchanged = write_mcp_entry(
            &target,
            "/usr/local/bin/ctx",
            false,
            "Test Client",
            "mcpServers",
            false,
        )
        .unwrap();
        assert!(!unchanged);
    }

    #[test]
    fn test_run_with_args_plain_format_from_config() {
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_path = temp_dir.path().to_path_buf();
        fs::write(temp_path.join("note.txt"), "hello docs\n").unwrap();
        fs::write(
            temp_path.join(".ctxconfig"),
            "mode = docs\nformat = plain\n",
        )
        .unwrap();

        let args = Args {
            code: true,
            ..base_args(temp_path)
        };

        let res = run_with_args(args, |_| Ok::<(), Box<dyn std::error::Error>>(()));
        assert!(res.is_ok());
    }

    #[test]
    fn test_run_with_args_yaml_format_in_config_is_ignored() {
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_path = temp_dir.path().to_path_buf();
        fs::write(temp_path.join("main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            temp_path.join(".ctxconfig"),
            "default_format = yaml\n",
        )
        .unwrap();

        let args = Args {
            code: true,
            ..base_args(temp_path)
        };

        let res = run_with_args(args, |_| Ok::<(), Box<dyn std::error::Error>>(()));
        assert!(res.is_ok());
    }
}

#[derive(clap::Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
enum Command {
    /// Analyze the project and query dependency or symbol relationships
    #[command(visible_alias = "g")]
    Graph(GraphCommand),
    /// MCP commands: `ctx mcp` (or `ctx mcp serve`) starts the server; `ctx mcp install` registers ctx with coding agents
    #[command(subcommand)]
    Mcp(McpCommand),
    /// Open interactive TUI to view/edit global settings (~/.config/ctx/config)
    #[command(visible_alias = "config")]
    Setting(SettingCommand),
    /// Show project-level usage stats (files, tokens, lines), codegraph index info if present, and MCP notes
    Stats(StatsCommand),
    /// Comprehensive health report for parsers, LSP, hybrid search, and codegraph index
    #[command(visible_alias = "health", visible_alias = "hc")]
    Healthcheck(HealthcheckCommand),
}

#[derive(clap::Subcommand, Debug)]
enum McpCommand {
    /// Start the Model Context Protocol (MCP) server over stdio (default when running `ctx mcp`)
    Serve,
    /// Auto-install / register the ctx MCP server into popular coding agents (Claude Desktop, Cursor, Gemini, Continue, etc.)
    Install(InstallCommand),
}

#[derive(clap::Args, Debug)]
struct SettingCommand {
    /// Project directory for merging legacy project-local .ctxconfig overrides
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(clap::Args, Debug)]
struct StatsCommand {
    /// Target project directory for stats (uses .ctxconfig if present)
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(clap::Args, Debug)]
#[command(
    about = "Report health of tree-sitter parsers, LSP servers, hybrid search, and codegraph index",
    after_help = "Examples:\n  \
                  ctx healthcheck\n  \
                  ctx healthcheck --probe\n  \
                  ctx healthcheck --format json\n  \
                  ctx hc /path/to/project"
)]
struct HealthcheckCommand {
    /// Target project directory (uses .ctxconfig if present)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Run live probes: LSP init, ONNX inference, hybrid backend wiring
    #[arg(long)]
    probe: bool,

    /// Output format (text or json)
    #[arg(long, default_value = "text")]
    format: String,
}

#[derive(clap::Args, Debug)]
struct InstallCommand {
    /// Which clients to target (comma separated). Defaults to common ones. Examples: claude,cursor,gemini,continue,code,vscode
    #[arg(long, value_delimiter = ',')]
    clients: Vec<String>,

    /// Dry run: show what would be written without modifying files
    #[arg(long)]
    dry_run: bool,
}

#[derive(clap::Args, Debug)]
#[command(
    about = "Analyze the project and build/query a symbol and call graph",
    long_about = "The graph command scans the selected project files and builds a local SQLite index of \
                  modules, symbols, calls, and dependencies. You can build this index and query it to \
                  find all symbols, view callers/callees of a symbol, or compute a call slice tree \
                  to understand how functions are connected.\n\n\
                  By default, ctx builds a fast tree-sitter based graph. Edges are labeled with \
                  their resolution confidence: Syntax, Heuristic, Unresolved. Use --with-lsp to \
                  ask language servers (e.g. rust-analyzer for Rust, pyright-langserver for Python) \
                  to enrich resolvable edges as LspExact. This is slower but more semantically precise.",
    after_help = "Examples:\n  \
                  ctx graph build\n  \
                  ctx graph build --with-lsp\n  \
                  ctx graph symbols\n  \
                  ctx graph calls my_function\n  \
                  ctx graph callers my_function\n  \
                  ctx graph slice my_function\n  \
                  ctx graph info\n  \
                  ctx g symbols\n  \
                  ctx g info"
)]
struct GraphCommand {
    #[command(subcommand)]
    command: GraphSubcommand,

    /// Target directory path containing the project to analyze
    #[arg(default_value = ".", global = true)]
    path: PathBuf,

    /// Disable language server database fallback (forces tree-sitter fallback only)
    #[arg(long, global = true)]
    no_rust_analyzer: bool,

    /// Enable language server database fallback (slow but precise call resolution, marks edges as LspExact)
    #[arg(long, global = true)]
    with_lsp: bool,

    /// Build dense embedding index (auto-enabled when embedding_model is in .ctxconfig)
    #[arg(long, global = true)]
    with_emb: bool,

    /// Skip dense embedding index even when configured
    #[arg(long, global = true)]
    without_emb: bool,

    /// Build Tantivy BM25 lexical index (auto-enabled when embedding_model is in .ctxconfig)
    #[arg(long, global = true)]
    with_lex: bool,

    /// Skip lexical index even when configured
    #[arg(long, global = true)]
    without_lex: bool,

    /// Show verbose build report and timings
    #[arg(long, short, global = true)]
    verbose: bool,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
#[value(rename_all = "kebab-case")]
pub enum CliGraphContextMode {
    Callers,
    Callees,
    Dependencies,
    Dependents,
    ForwardSlice,
    ReverseSlice,
    Neighborhood,
}

#[derive(clap::Subcommand, Debug)]
enum GraphSubcommand {
    /// Show codegraph index status and graph-related project overview
    Info {
        /// Output format (text, json)
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Build or rebuild the codegraph SQLite index database
    Build,
    /// List all indexed symbols grouped by their files, or find a specific symbol
    Symbols {
        /// The name or qualified path of the target symbol
        query: Option<String>,
    },
    /// List the direct callees (called functions/symbols) of a target symbol
    Calls {
        /// The name or qualified path of the target symbol
        symbol: String,
    },
    /// List the direct callees (called functions/symbols) of a target symbol
    Callees {
        /// The name or qualified path of the target symbol
        symbol: String,
    },
    /// List the direct callers of a target symbol
    Callers {
        /// The name or qualified path of the target symbol
        symbol: String,
    },
    /// Compute and display the forward call slice tree starting from a target symbol
    Slice {
        /// The name or qualified path of the target symbol
        symbol: String,
    },
    /// Extract a graph context around a target symbol and render it
    Context {
        /// The target symbol query
        symbol: String,
        /// Traversal mode
        #[arg(long)]
        mode: CliGraphContextMode,
        /// Maximum traversal depth
        #[arg(long, default_value_t = 2)]
        depth: usize,
        /// Maximum number of nodes to include in the context
        #[arg(long, default_value = "50")]
        max_nodes: usize,
    },
    /// Retrieve ranked code context under token budget around symbol
    Affect {
        /// The query to resolve (symbol name, qualified name, file path, etc.)
        query: String,
        /// Traversal mode (callers, callees, dependencies, dependents, forward, reverse, neighborhood, impact)
        #[arg(long, default_value = "neighborhood")]
        mode: String,
        /// Traversal depth (e.g. 2 or auto)
        #[arg(long, default_value = "auto")]
        depth: String,
        /// Maximum traversal nodes
        #[arg(long, default_value = "40")]
        max_nodes: usize,
        /// Maximum traversal files
        #[arg(long, default_value = "12")]
        max_files: usize,
        /// Specific edge kinds to traverse (repeatable)
        #[arg(long, short = 'e')]
        edge_kind: Vec<String>,
        /// Include test symbols
        #[arg(long, default_value_t = false)]
        include_tests: bool,
        /// Include unresolved edges
        #[arg(long, default_value_t = false)]
        include_unresolved: bool,
        /// Output format (text, json)
        #[arg(long, default_value = "text")]
        format: String,
        /// Include snippets (default)
        #[arg(long, conflicts_with = "no_snippets")]
        with_snippets: bool,
        /// Disable snippets
        #[arg(long)]
        no_snippets: bool,
        /// Context lines around snippets
        #[arg(long, default_value = "3")]
        context_lines: usize,
        /// Token budget
        #[arg(long, default_value = "12000")]
        token_budget: usize,
        /// Context window size
        #[arg(long, default_value = "32000")]
        model_context_window: usize,
        /// Packing strategy (balanced, frontloaded, sandwich)
        #[arg(long, default_value = "sandwich")]
        packing: String,
        /// Ranking strategy (graph, lexical, hybrid)
        #[arg(long, default_value = "hybrid")]
        ranking: String,
        /// Explain ranking
        #[arg(long, default_value_t = false)]
        explain_ranking: bool,
    },
}

fn handle_graph_command(graph_args: GraphCommand) -> Result<(), Box<dyn std::error::Error>> {
    use ctx_codegraph::BuildIndexOptions;
    use std::collections::HashMap;

    let use_rust_analyzer = graph_args.with_lsp && !graph_args.no_rust_analyzer;

    match graph_args.command {
        GraphSubcommand::Info { format } => {
            handle_graph_info(&graph_args.path, use_rust_analyzer, &format)?;
        }
        GraphSubcommand::Build => {
            let start_time = std::time::Instant::now();
            println!("\x1b[36m\x1b[1mBuilding codegraph index...\x1b[0m");
            let config = ctx_config::find_and_load_config(&graph_args.path).unwrap_or_default();
            let with_emb = if graph_args.without_emb {
                Some(false)
            } else if graph_args.with_emb {
                Some(true)
            } else {
                None
            };
            let with_lex = if graph_args.without_lex {
                Some(false)
            } else if graph_args.with_lex {
                Some(true)
            } else {
                None
            };
            let options = BuildIndexOptions {
                use_lsp: use_rust_analyzer,
                max_depth: None,
                include_tests: true,
                change_detection: ctx_codegraph::model::FileChangeDetection::MtimeAndSize,
                with_embeddings: with_emb,
                with_lexical: with_lex,
                force_search_rebuild: false,
            };
            let (_index, report) = ctx_codegraph::rebuild_index_db(&graph_args.path, options)?;
            let elapsed = start_time.elapsed();

            if graph_args.verbose {
                println!("\x1b[35m\x1b[1m--- Codegraph Build Report ---\x1b[0m");
                println!(
                    "Full Rebuild: {}",
                    if report.full_rebuild {
                        "\x1b[33myes\x1b[0m"
                    } else {
                        "\x1b[32mno\x1b[0m"
                    }
                );
                if let Some(reason) = report.full_rebuild_reason {
                    println!("Full Rebuild Reason: {:?}", reason);
                }
                println!(
                    "Files: \x1b[32m{} added\x1b[0m, \x1b[33m{} modified\x1b[0m, \x1b[31m{} deleted\x1b[0m, \x1b[90m{} unchanged\x1b[0m",
                    report.added_files,
                    report.modified_files,
                    report.deleted_files,
                    report.unchanged_files
                );
                println!(
                    "Parsed Files: {}, Reused Files: {}",
                    report.parsed_files, report.reused_files
                );
                println!(
                    "Symbols Written: {}, Call Sites Written: {}, Edges Written: {}",
                    report.symbols_written, report.call_sites_written, report.edges_written
                );
                println!(
                    "Edge Resolution Confidence: LspExact={}, Syntax={}, Heuristic={}, Unresolved={}",
                    report.lsp_edges_exact,
                    report.syntax_edges,
                    report.heuristic_edges,
                    report.unresolved_edges
                );
                if report.chunks_written > 0
                    || report.embeddings_written > 0
                    || report.lexical_docs_written > 0
                {
                    println!(
                        "Search Index: {} chunks, {} embeddings, {} lexical docs",
                        report.chunks_written, report.embeddings_written, report.lexical_docs_written
                    );
                }
                println!("Build Time: \x1b[36m{:.2?}\x1b[0m", elapsed);
                println!("\x1b[35m\x1b[1m-----------------------------\x1b[0m");
            } else {
                if report.full_rebuild {
                    let suffix = if let Some(reason) = report.full_rebuild_reason {
                        match reason {
                            ctx_codegraph::model::RebuildReason::MissingDatabase => {
                                " (Index not found)"
                            }
                            ctx_codegraph::model::RebuildReason::CorruptDatabase => {
                                " (Database corrupted)"
                            }
                            ctx_codegraph::model::RebuildReason::SchemaVersionChanged => {
                                " (Schema version changed)"
                            }
                            ctx_codegraph::model::RebuildReason::IndexerVersionChanged => {
                                " (Indexer version changed)"
                            }
                            ctx_codegraph::model::RebuildReason::BackendSetChanged => {
                                " (Backend set changed)"
                            }
                            ctx_codegraph::model::RebuildReason::BackendVersionChanged => {
                                " (Backend version changed)"
                            }
                            ctx_codegraph::model::RebuildReason::ParserVersionChanged => {
                                " (Parser version changed)"
                            }
                            ctx_codegraph::model::RebuildReason::ParserConfigChanged => {
                                " (Parser configuration changed)"
                            }
                            ctx_codegraph::model::RebuildReason::ResolverVersionChanged => {
                                " (Resolver version changed)"
                            }
                            ctx_codegraph::model::RebuildReason::ResolverConfigChanged => {
                                " (Resolver configuration changed)"
                            }
                            ctx_codegraph::model::RebuildReason::DiscoveryConfigChanged => {
                                " (Discovery configuration changed)"
                            }
                            ctx_codegraph::model::RebuildReason::ChangeDetectionStrategyChanged => {
                                " (Change detection strategy changed)"
                            }
                            ctx_codegraph::model::RebuildReason::PreviousRunIncomplete => {
                                " (Previous run was incomplete)"
                            }
                            ctx_codegraph::model::RebuildReason::PreviousRunFailed => {
                                " (Previous run failed)"
                            }
                            ctx_codegraph::model::RebuildReason::EmbeddingModelChanged => {
                                " (Embedding model changed)"
                            }
                            ctx_codegraph::model::RebuildReason::LexicalIndexStale => {
                                " (Lexical index stale)"
                            }
                            ctx_codegraph::model::RebuildReason::ChunkSchemaChanged => {
                                " (Chunk schema changed)"
                            }
                        }
                    } else {
                        ""
                    };
                    println!("\x1b[32m✨ Full rebuild completed{}.\x1b[0m", suffix);
                } else {
                    println!("\x1b[32m✨ Incremental update completed.\x1b[0m");
                }
                println!(
                    "\x1b[34m📂 Files:\x1b[0m \x1b[32m{} added\x1b[0m, \x1b[33m{} modified\x1b[0m, \x1b[31m{} deleted\x1b[0m, \x1b[90m{} unchanged\x1b[0m",
                    report.added_files,
                    report.modified_files,
                    report.deleted_files,
                    report.unchanged_files
                );
                println!(
                    "\x1b[35m🕸️  Symbols updated:\x1b[0m \x1b[1m{}\x1b[0m | \x1b[36mcall sites updated:\x1b[0m \x1b[1m{}\x1b[0m | \x1b[33medges updated:\x1b[0m \x1b[1m{}\x1b[0m",
                    report.symbols_written, report.call_sites_written, report.edges_written
                );
                println!(
                    "\x1b[32m✔ Index successfully built at\x1b[0m \x1b[4m.ctx-codegraph/codegraph.sqlite\x1b[0m \x1b[36m[in {:.2?}]\x1b[0m",
                    elapsed
                );
            }
        }
        GraphSubcommand::Symbols { mut query } => {
            let mut target_path = graph_args.path.clone();
            if let Some(ref q) = query
                && std::path::Path::new(q).is_dir()
            {
                target_path = std::path::PathBuf::from(q);
                query = None;
            }

            let conn =
                get_connection_or_rebuild(&target_path, use_rust_analyzer, graph_args.verbose)?;

            if let Some(q) = query {
                match ctx_codegraph::resolve_symbol(&conn, &q)? {
                    ctx_codegraph::SymbolResolution::Unique(obj) => {
                        let rel_path = obj
                            .file_path
                            .strip_prefix(&target_path)
                            .unwrap_or(&obj.file_path);
                        println!(
                            "Unique match: {} ({:?}) in {} at L{}",
                            obj.qualified_name,
                            obj.kind,
                            rel_path.display(),
                            obj.range.start_line
                        );
                    }
                    ctx_codegraph::SymbolResolution::Ambiguous(objs) => {
                        println!("Ambiguous query: {}", q);
                        println!("\nCandidates:");
                        for obj in objs {
                            let rel_path = obj
                                .file_path
                                .strip_prefix(&target_path)
                                .unwrap_or(&obj.file_path);
                            println!(
                                "  {:<30} {}:{}",
                                obj.qualified_name,
                                rel_path.display(),
                                obj.range.start_line
                            );
                        }
                    }
                    ctx_codegraph::SymbolResolution::NotFound => {
                        println!("Symbol not found: {}", q);
                    }
                }
            } else {
                let index = ctx_codegraph::load_index(&conn, &target_path)?;

                let mut grouped: HashMap<PathBuf, Vec<ctx_codegraph::Symbol>> = HashMap::new();
                for sym in index.symbols {
                    grouped.entry(sym.file.clone()).or_default().push(sym);
                }

                let mut sorted_files: Vec<PathBuf> = grouped.keys().cloned().collect();
                sorted_files.sort();

                for file in sorted_files {
                    let rel_path = file.strip_prefix(&target_path).unwrap_or(&file);
                    println!("{}:", rel_path.display());
                    let mut file_syms = grouped.remove(&file).unwrap();
                    file_syms.sort_by_key(|s| s.range.start_line);
                    for sym in file_syms {
                        println!(
                            "  [{:?}] {} (L{}-{})",
                            sym.kind, sym.name, sym.range.start_line, sym.range.end_line
                        );
                    }
                }
            }
        }
        GraphSubcommand::Calls { symbol } | GraphSubcommand::Callees { symbol } => {
            let conn =
                get_connection_or_rebuild(&graph_args.path, use_rust_analyzer, graph_args.verbose)?;
            let candidates = ctx_codegraph::storage::find_symbols(&conn, &symbol)?;

            if candidates.is_empty() {
                eprintln!("Symbol not found: {}", symbol);
                std::process::exit(1);
            }

            if candidates.len() > 1 {
                print_ambiguity(&symbol, &candidates, &graph_args.path);
                std::process::exit(1);
            }

            let sym = &candidates[0];
            let callees = ctx_codegraph::storage::load_callees(&conn, sym.id.unwrap())?;

            println!("Callees of {}:", sym.qualified_name);
            if callees.is_empty() {
                println!("  (none)");
            } else {
                for (edge, target) in callees {
                    match target {
                        Some(t) => println!(
                            "  - {} -> {} ({})",
                            edge.raw_text.as_deref().unwrap_or_default(),
                            t.qualified_name,
                            edge.confidence
                        ),
                        None => println!(
                            "  - {} -> [Unresolved] ({})",
                            edge.raw_text.as_deref().unwrap_or_default(),
                            edge.confidence
                        ),
                    }
                }
            }
        }
        GraphSubcommand::Callers { symbol } => {
            let conn =
                get_connection_or_rebuild(&graph_args.path, use_rust_analyzer, graph_args.verbose)?;
            let candidates = ctx_codegraph::storage::find_symbols(&conn, &symbol)?;

            if candidates.is_empty() {
                eprintln!("Symbol not found: {}", symbol);
                std::process::exit(1);
            }

            if candidates.len() > 1 {
                print_ambiguity(&symbol, &candidates, &graph_args.path);
                std::process::exit(1);
            }

            let sym = &candidates[0];
            let callers = ctx_codegraph::storage::load_callers(&conn, sym.id.unwrap())?;

            println!("Callers of {}:", sym.qualified_name);
            if callers.is_empty() {
                println!("  (none)");
            } else {
                for (edge, caller) in callers {
                    let range = edge
                        .range
                        .clone()
                        .unwrap_or(ctx_codegraph::model::TextRange {
                            start_line: 0,
                            start_col: 0,
                            end_line: 0,
                            end_col: 0,
                        });
                    println!(
                        "  - {} via `{}` at L{}:{} ({})",
                        caller.qualified_name,
                        edge.raw_text.as_deref().unwrap_or_default(),
                        range.start_line,
                        range.start_col,
                        edge.confidence
                    );
                }
            }
        }
        GraphSubcommand::Slice { symbol } => {
            let conn =
                get_connection_or_rebuild(&graph_args.path, use_rust_analyzer, graph_args.verbose)?;
            let candidates = ctx_codegraph::storage::find_symbols(&conn, &symbol)?;

            if candidates.is_empty() {
                eprintln!("Symbol not found: {}", symbol);
                std::process::exit(1);
            }

            if candidates.len() > 1 {
                print_ambiguity(&symbol, &candidates, &graph_args.path);
                std::process::exit(1);
            }

            let sym = &candidates[0];
            let index = ctx_codegraph::load_index(&conn, &graph_args.path)?;

            println!("Forward slice tree for {}:", sym.qualified_name);
            let mut visited = HashSet::new();
            visited.insert(sym.id.unwrap());
            let mut printed_count = 0;
            print_slice_tree_helper(
                &index,
                sym.id.unwrap(),
                0,
                10,
                &mut visited,
                &mut printed_count,
            );
        }
        GraphSubcommand::Context {
            symbol,
            mode,
            depth,
            max_nodes,
        } => {
            // Call get_ (unified to smart check via get_index_state + cond rebuild) so
            // --with-lsp flag is respected for this query, messages emitted only on actual work.
            let _conn =
                get_connection_or_rebuild(&graph_args.path, use_rust_analyzer, graph_args.verbose)?;
            let service = ctx_codegraph::GraphContextService::load_or_build(&graph_args.path)?;

            match service.resolve_symbol(&symbol)? {
                ctx_codegraph::SymbolResolution::Unique(obj) => {
                    let options = ctx_codegraph::GraphContextOptions {
                        mode: mode.into(),
                        max_depth: depth,
                        max_nodes,
                        include_root: true,
                    };
                    let result = service.build_context_for_symbol(obj.id, options)?;
                    let rendered = render_graph_context(
                        &result,
                        &graph_args.path,
                        mode.into(),
                        depth,
                        max_nodes,
                    )?;
                    print!("{}", rendered);
                }
                ctx_codegraph::SymbolResolution::Ambiguous(objs) => {
                    eprintln!("Ambiguous symbol: {}", symbol);
                    eprintln!("\nCandidates:");
                    for cand in objs {
                        let rel_path = cand
                            .file_path
                            .strip_prefix(&graph_args.path)
                            .unwrap_or(&cand.file_path);
                        eprintln!(
                            "  {:<30} {}:{}",
                            cand.qualified_name,
                            rel_path.display(),
                            cand.range.start_line
                        );
                    }
                    std::process::exit(1);
                }
                ctx_codegraph::SymbolResolution::NotFound => {
                    eprintln!("Symbol not found: {}", symbol);
                    std::process::exit(1);
                }
            }
        }
        GraphSubcommand::Affect {
            query,
            mode,
            depth,
            max_nodes,
            max_files,
            edge_kind,
            include_tests,
            include_unresolved,
            format,
            with_snippets: _,
            no_snippets,
            context_lines,
            token_budget,
            model_context_window,
            packing,
            ranking,
            explain_ranking,
        } => {
            let conn =
                get_connection_or_rebuild(&graph_args.path, use_rust_analyzer, graph_args.verbose)?;

            let ctx_mode = match mode.as_str() {
                "callers" => ctx_codegraph::GraphContextMode::Callers,
                "callees" => ctx_codegraph::GraphContextMode::Callees,
                "dependencies" => ctx_codegraph::GraphContextMode::Dependencies,
                "dependents" => ctx_codegraph::GraphContextMode::Dependents,
                "forward" => ctx_codegraph::GraphContextMode::Forward,
                "reverse" => ctx_codegraph::GraphContextMode::Reverse,
                "neighborhood" => ctx_codegraph::GraphContextMode::Neighborhood,
                "impact" => ctx_codegraph::GraphContextMode::Impact,
                _ => ctx_codegraph::GraphContextMode::Neighborhood,
            };

            let depth_limit = if depth == "auto" {
                ctx_codegraph::DepthLimit::Auto
            } else if let Ok(d) = depth.parse::<usize>() {
                ctx_codegraph::DepthLimit::Fixed(d)
            } else {
                return Err(format!(
                    "Invalid depth '{}'. Depth must be a non-negative integer or 'auto'.",
                    depth
                )
                .into());
            };

            let ranking_mode = match ranking.as_str() {
                "graph" => ctx_codegraph::RankingMode::Graph,
                "lexical" => ctx_codegraph::RankingMode::Lexical,
                "hybrid" => ctx_codegraph::RankingMode::Hybrid,
                _ => ctx_codegraph::RankingMode::Hybrid,
            };

            let packing_mode = match packing.as_str() {
                "frontloaded" => ctx_codegraph::ContextPackingMode::Frontloaded,
                "sandwich" => ctx_codegraph::ContextPackingMode::Sandwich,
                "balanced" => ctx_codegraph::ContextPackingMode::Balanced,
                _ => ctx_codegraph::ContextPackingMode::Sandwich,
            };

            let use_snippets = !no_snippets;

            let budget = ctx_codegraph::ContextBudget {
                token_budget,
                model_context_window: Some(model_context_window),
                reserve_output_tokens: 1000,
                reserve_instruction_tokens: 1000,
            };

            let parsed_edge_kinds: Vec<ctx_codegraph::EdgeKind> = edge_kind
                .iter()
                .filter_map(|k| ctx_codegraph::EdgeKind::from_str(k))
                .collect();

            let result = ctx_codegraph::retrieve_graph_context(
                &conn,
                &query,
                ctx_mode,
                depth_limit,
                max_nodes,
                max_files,
                ranking_mode,
                packing_mode,
                use_snippets,
                context_lines,
                &budget,
                include_tests,
                &parsed_edge_kinds,
                include_unresolved,
                explain_ranking,
            )?;

            if format != "json" && format != "text" {
                return Err(format!(
                    "Invalid format '{}'. Supported formats are 'text' and 'json'.",
                    format
                )
                .into());
            }

            if format == "json" {
                let json_str = serde_json::to_string_pretty(&result)?;
                println!("{}", json_str);
            } else {
                for s in &result.sections {
                    print!("{}", s.text);
                }
            }
        }
    }

    Ok(())
}

fn format_index_state(state: &ctx_codegraph::model::IndexState) -> String {
    use ctx_codegraph::model::{IndexState, RebuildReason};
    match state {
        IndexState::Missing => "missing".to_string(),
        IndexState::Ready => "ready".to_string(),
        IndexState::NeedsIncrementalUpdate(diff) => format!(
            "stale (+{} added, ~{} modified, -{} deleted)",
            diff.added.len(),
            diff.modified.len(),
            diff.deleted.len()
        ),
        IndexState::NeedsFullRebuild(reason) => {
            let label = match reason {
                RebuildReason::MissingDatabase => "missing database",
                RebuildReason::CorruptDatabase => "corrupt database",
                RebuildReason::SchemaVersionChanged => "schema version changed",
                RebuildReason::IndexerVersionChanged => "indexer version changed",
                RebuildReason::BackendSetChanged => "backend set changed",
                RebuildReason::BackendVersionChanged => "backend version changed",
                RebuildReason::ParserVersionChanged => "parser version changed",
                RebuildReason::ParserConfigChanged => "parser configuration changed",
                RebuildReason::ResolverVersionChanged => "resolver version changed",
                RebuildReason::ResolverConfigChanged => "resolver configuration changed",
                RebuildReason::DiscoveryConfigChanged => "discovery configuration changed",
                RebuildReason::ChangeDetectionStrategyChanged => {
                    "change detection strategy changed"
                }
                RebuildReason::PreviousRunIncomplete => "previous run incomplete",
                RebuildReason::PreviousRunFailed => "previous run failed",
                RebuildReason::EmbeddingModelChanged => "embedding model changed",
                RebuildReason::LexicalIndexStale => "lexical index stale",
                RebuildReason::ChunkSchemaChanged => "chunk schema changed",
            };
            format!("needs rebuild ({label})")
        }
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

fn handle_graph_info(
    path: &Path,
    cli_use_lsp: bool,
    format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use ctx_codegraph::index::BuildIndexOptions;
    use ctx_codegraph::model::IndexState;
    use ctx_codegraph::storage::{find_workspace_root, get_index_state};
    use std::fmt::Write as _;

    let config = ctx_config::find_and_load_config(path).unwrap_or_default();
    let use_lsp = cli_use_lsp || config.use_lsp.unwrap_or(false);
    let workspace_root = find_workspace_root(path);
    let db_path = workspace_root.join(".ctx-codegraph/codegraph.sqlite");
    let lexical_path = workspace_root.join(".ctx-codegraph/lexical");
    let options = BuildIndexOptions {
        use_lsp,
        ..Default::default()
    };
    let state = get_index_state(&workspace_root, &options).unwrap_or(IndexState::Missing);
    let state_label = format_index_state(&state);

    let mut languages: Vec<(String, i64)> = Vec::new();
    let mut edge_confidence: Vec<(String, i64)> = Vec::new();
    let mut metadata: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut file_count = 0i64;
    let mut symbol_count = 0i64;
    let mut edge_count = 0i64;
    let mut chunk_count = 0i64;
    let mut embedding_count = 0i64;
    let mut chunks_table = false;
    let mut embeddings_table = false;
    let mut db_size_bytes: Option<u64> = None;
    let mut db_mtime: Option<String> = None;

    if db_path.exists() {
        if let Ok(meta) = std::fs::metadata(&db_path) {
            db_size_bytes = Some(meta.len());
            if let Ok(mtime) = meta.modified() {
                db_mtime = Some(format!("{mtime:?}"));
            }
        }
        if let Ok(conn) = ctx_codegraph::open_db(&workspace_root) {
            file_count = query_count(&conn, "SELECT COUNT(*) FROM files");
            symbol_count = query_count(&conn, "SELECT COUNT(*) FROM symbols");
            edge_count = query_count(&conn, "SELECT COUNT(*) FROM edges");
            chunks_table = table_exists(&conn, "chunks");
            embeddings_table = table_exists(&conn, "chunk_embeddings");
            if chunks_table {
                chunk_count = query_count(&conn, "SELECT COUNT(*) FROM chunks");
            }
            if embeddings_table {
                embedding_count = query_count(&conn, "SELECT COUNT(*) FROM chunk_embeddings");
            }

            let mut lang_stmt = conn.prepare(
                "SELECT language, COUNT(*) FROM files GROUP BY language ORDER BY COUNT(*) DESC",
            )?;
            let lang_rows = lang_stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            for row in lang_rows {
                languages.push(row?);
            }

            let mut conf_stmt = conn.prepare(
                "SELECT confidence, COUNT(*) FROM edges GROUP BY confidence ORDER BY COUNT(*) DESC",
            )?;
            let conf_rows = conf_stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            for row in conf_rows {
                edge_confidence.push(row?);
            }

            let mut meta_stmt = conn.prepare("SELECT key, value FROM metadata ORDER BY key")?;
            let meta_rows = meta_stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in meta_rows {
                let (k, v) = row?;
                metadata.insert(k, v);
            }
        }
    }

    let search_configured = config.search_auto_enabled();
    let lexical_present = lexical_path.exists();

    if format == "json" {
        let mut lang_map = serde_json::Map::new();
        for (lang, count) in &languages {
            lang_map.insert(lang.clone(), serde_json::json!(count));
        }
        let mut confidence_map = serde_json::Map::new();
        for (conf, count) in &edge_confidence {
            confidence_map.insert(conf.clone(), serde_json::json!(count));
        }
        let payload = serde_json::json!({
            "requested_path": path.display().to_string(),
            "workspace_root": workspace_root.display().to_string(),
            "index": {
                "state": state_label,
                "database": db_path.display().to_string(),
                "database_exists": db_path.exists(),
                "database_size_bytes": db_size_bytes,
                "database_mtime": db_mtime,
                "files": file_count,
                "symbols": symbol_count,
                "edges": edge_count,
                "chunks": chunk_count,
                "embeddings": embedding_count,
                "languages": lang_map,
                "edge_confidence": confidence_map,
                "metadata": metadata,
            },
            "search": {
                "configured": search_configured,
                "embedding_model": config.embedding_model,
                "reranker_model": config.reranker_model,
                "lexical_index_present": lexical_present,
                "default_retrieval_strategy": config.default_retrieval_strategy,
            },
            "settings": {
                "use_lsp": use_lsp,
                "default_format": config.default_format,
                "default_ranking": config.default_ranking,
                "default_packing": config.default_packing,
                "default_token_budget": config.default_token_budget,
            },
            "hints": graph_info_hints(&state, db_path.exists()),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    if format != "text" {
        return Err(format!("unsupported format: {format} (use text or json)").into());
    }

    let mut out = String::new();
    writeln!(out, "ctx graph info")?;
    writeln!(out)?;
    writeln!(out, "Workspace")?;
    writeln!(out, "  path: {}", path.display())?;
    writeln!(out, "  root: {}", workspace_root.display())?;
    writeln!(out)?;
    writeln!(out, "Index")?;
    writeln!(out, "  state: {state_label}")?;
    writeln!(out, "  database: {}", db_path.display())?;
    if let Some(size) = db_size_bytes {
        writeln!(out, "  size: {} bytes", size)?;
    }
    if let Some(mtime) = &db_mtime {
        writeln!(out, "  modified: {mtime}")?;
    }
    if db_path.exists() {
        writeln!(out, "  files: {file_count}")?;
        writeln!(out, "  symbols: {symbol_count}")?;
        writeln!(out, "  edges: {edge_count}")?;
        if chunks_table {
            writeln!(out, "  chunks: {chunk_count}")?;
        }
        if embeddings_table || search_configured {
            writeln!(out, "  embeddings: {embedding_count}")?;
        }
        if !languages.is_empty() {
            writeln!(out, "  languages:")?;
            for (lang, count) in &languages {
                writeln!(out, "    - {lang}: {count}")?;
            }
        }
        if !edge_confidence.is_empty() {
            writeln!(out, "  edge confidence:")?;
            for (conf, count) in &edge_confidence {
                writeln!(out, "    - {conf}: {count}")?;
            }
        }
        if !metadata.is_empty() {
            writeln!(out, "  metadata:")?;
            for (key, value) in &metadata {
                writeln!(out, "    - {key}: {value}")?;
            }
        }
    } else {
        writeln!(out, "  (no index yet)")?;
    }
    writeln!(out)?;
    writeln!(out, "Search")?;
    writeln!(
        out,
        "  hybrid enabled: {}",
        if search_configured { "yes" } else { "no" }
    )?;
    if let Some(model) = &config.embedding_model {
        writeln!(out, "  embedding_model: {model}")?;
    }
    if let Some(model) = &config.reranker_model {
        writeln!(out, "  reranker_model: {model}")?;
    }
    writeln!(
        out,
        "  lexical index: {}",
        if lexical_present { "present" } else { "missing" }
    )?;
    if let Some(strategy) = &config.default_retrieval_strategy {
        writeln!(out, "  default strategy: {strategy}")?;
    }
    writeln!(out)?;
    writeln!(out, "Settings")?;
    writeln!(out, "  use_lsp: {use_lsp}")?;
    if let Some(fmt) = &config.default_format {
        writeln!(out, "  default_format: {fmt}")?;
    }
    if let Some(ranking) = &config.default_ranking {
        writeln!(out, "  default_ranking: {ranking}")?;
    }
    if let Some(packing) = &config.default_packing {
        writeln!(out, "  default_packing: {packing}")?;
    }
    if let Some(budget) = config.default_token_budget {
        writeln!(out, "  default_token_budget: {budget}")?;
    }
    writeln!(out)?;
    writeln!(out, "Next steps")?;
    for hint in graph_info_hints(&state, db_path.exists()) {
        writeln!(out, "  - {hint}")?;
    }
    print!("{out}");
    Ok(())
}

fn graph_info_hints(
    state: &ctx_codegraph::model::IndexState,
    db_exists: bool,
) -> Vec<String> {
    use ctx_codegraph::model::IndexState;
    let mut hints = Vec::new();
    match state {
        IndexState::Missing | IndexState::NeedsFullRebuild(_) if !db_exists => {
            hints.push("Run `ctx graph build` to create the codegraph index.".into());
        }
        IndexState::NeedsFullRebuild(_) => {
            hints.push("Run `ctx graph build` to rebuild the index.".into());
        }
        IndexState::NeedsIncrementalUpdate(_) => {
            hints.push("Run `ctx graph build` to refresh changed files.".into());
        }
        IndexState::Ready => {
            hints.push("Index is ready. Try `ctx graph symbols` or `ctx graph affect <query>`.".into());
        }
        IndexState::Missing => {}
    }
    if hints.is_empty() && !db_exists {
        hints.push("Run `ctx graph build` to create the codegraph index.".into());
    }
    hints.push("Run `ctx stats` for scan totals and MCP usage.".into());
    hints
}

fn get_connection_or_rebuild(
    path: &Path,
    use_rust_analyzer: bool,
    verbose: bool,
) -> Result<rusqlite::Connection, Box<dyn std::error::Error>> {
    let workspace_root = ctx_codegraph::storage::find_workspace_root(path);
    let options = ctx_codegraph::BuildIndexOptions {
        use_lsp: use_rust_analyzer,
        ..Default::default()
    };

    // Unified "ensure fresh" logic: use smart state check for fast path.
    // Only call rebuild (to obtain report for messages) if not Ready.
    // For queries, no output at all when Ready (no work).
    let state = ctx_codegraph::get_index_state(&workspace_root, &options).unwrap_or_else(|_| {
        // On unexpected state err, fall back to rebuild path (will handle)
        ctx_codegraph::model::IndexState::NeedsFullRebuild(
            ctx_codegraph::model::RebuildReason::MissingDatabase,
        )
    });

    let conn = if matches!(state, ctx_codegraph::model::IndexState::Ready) {
        ctx_codegraph::open_db(&workspace_root)?
    } else {
        let (_index, report) = ctx_codegraph::rebuild_index_db(&workspace_root, options)?;
        if verbose {
            println!("--- Codegraph Build Report ---");
            println!(
                "Full Rebuild: {}",
                if report.full_rebuild { "yes" } else { "no" }
            );
            if let Some(reason) = report.full_rebuild_reason {
                println!("Full Rebuild Reason: {:?}", reason);
            }
            println!(
                "Files: {} added, {} modified, {} deleted, {} unchanged",
                report.added_files,
                report.modified_files,
                report.deleted_files,
                report.unchanged_files
            );
            println!(
                "Parsed Files: {}, Reused Files: {}",
                report.parsed_files, report.reused_files
            );
            println!(
                "Symbols Written: {}, Call Sites Written: {}, Edges Written: {}",
                report.symbols_written, report.call_sites_written, report.edges_written
            );
            println!(
                "Edge Resolution Confidence: LspExact={}, Syntax={}, Heuristic={}, Unresolved={}",
                report.lsp_edges_exact,
                report.syntax_edges,
                report.heuristic_edges,
                report.unresolved_edges
            );
            println!("-----------------------------");
        } else {
            if report.full_rebuild {
                if let Some(reason) = report.full_rebuild_reason {
                    match reason {
                        ctx_codegraph::model::RebuildReason::MissingDatabase => {
                            println!("Index not found. Built codegraph index.");
                        }
                        ctx_codegraph::model::RebuildReason::CorruptDatabase => {
                            println!("Database corrupted. Rebuilt codegraph index cleanly.");
                        }
                        ctx_codegraph::model::RebuildReason::SchemaVersionChanged => {
                            println!("Schema version changed. Rebuilt codegraph index cleanly.");
                        }
                        ctx_codegraph::model::RebuildReason::IndexerVersionChanged => {
                            println!("Indexer version changed. Rebuilt codegraph index cleanly.");
                        }
                        ctx_codegraph::model::RebuildReason::BackendSetChanged => {
                            println!("Backend set changed. Rebuilt codegraph index cleanly.");
                        }
                        ctx_codegraph::model::RebuildReason::BackendVersionChanged => {
                            println!("Backend version changed. Rebuilt codegraph index cleanly.");
                        }
                        ctx_codegraph::model::RebuildReason::ParserVersionChanged => {
                            println!("Parser version changed. Rebuilt codegraph index cleanly.");
                        }
                        ctx_codegraph::model::RebuildReason::ParserConfigChanged => {
                            println!(
                                "Parser configuration changed. Rebuilt codegraph index cleanly."
                            );
                        }
                        ctx_codegraph::model::RebuildReason::ResolverVersionChanged => {
                            println!("Resolver version changed. Rebuilt codegraph index cleanly.");
                        }
                        ctx_codegraph::model::RebuildReason::ResolverConfigChanged => {
                            println!(
                                "Resolver configuration changed. Rebuilt codegraph index cleanly."
                            );
                        }
                        ctx_codegraph::model::RebuildReason::DiscoveryConfigChanged => {
                            println!(
                                "Discovery configuration changed. Rebuilt codegraph index cleanly."
                            );
                        }
                        ctx_codegraph::model::RebuildReason::ChangeDetectionStrategyChanged => {
                            println!(
                                "Change detection strategy changed. Rebuilt codegraph index cleanly."
                            );
                        }
                        ctx_codegraph::model::RebuildReason::PreviousRunIncomplete => {
                            println!(
                                "Previous index run was incomplete. Rebuilt codegraph index cleanly."
                            );
                        }
                        ctx_codegraph::model::RebuildReason::PreviousRunFailed => {
                            println!("Previous index run failed. Rebuilt codegraph index cleanly.");
                        }
                        ctx_codegraph::model::RebuildReason::EmbeddingModelChanged => {
                            println!("Embedding model changed. Rebuilt search indexes.");
                        }
                        ctx_codegraph::model::RebuildReason::LexicalIndexStale => {
                            println!("Lexical index stale. Rebuilt search indexes.");
                        }
                        ctx_codegraph::model::RebuildReason::ChunkSchemaChanged => {
                            println!("Chunk schema changed. Rebuilt search indexes.");
                        }
                    }
                } else {
                    println!("Rebuilt codegraph index.");
                }
            } else if report.added_files > 0
                || report.modified_files > 0
                || report.deleted_files > 0
            {
                println!(
                    "Incremental update: {} added, {} modified, {} deleted files.",
                    report.added_files, report.modified_files, report.deleted_files
                );
            }
        }
        ctx_codegraph::open_db(&workspace_root)?
    };
    Ok(conn)
}

fn print_ambiguity(symbol: &str, candidates: &[ctx_codegraph::Symbol], root_path: &Path) {
    eprintln!("Ambiguous symbol: {}", symbol);
    eprintln!("\nCandidates:");
    for cand in candidates {
        let rel_path = cand.file.strip_prefix(root_path).unwrap_or(&cand.file);
        eprintln!(
            "  {:<30} {}:{}",
            cand.qualified_name,
            rel_path.display(),
            cand.range.start_line
        );
    }
}

fn print_slice_tree_helper(
    index: &ctx_codegraph::CodeIndex,
    curr_id: ctx_codegraph::SymbolId,
    depth: usize,
    max_depth: usize,
    visited: &mut HashSet<ctx_codegraph::SymbolId>,
    printed_count: &mut usize,
) {
    if *printed_count >= 100 {
        if *printed_count == 100 {
            let indent = "  ".repeat(depth);
            println!("{}└─ ... (truncated)", indent);
            *printed_count += 1;
        }
        return;
    }
    *printed_count += 1;

    let sym = match index.symbols.iter().find(|s| s.id == Some(curr_id)) {
        Some(s) => s,
        None => return,
    };

    let indent = "  ".repeat(depth);
    if depth > 0 {
        println!("{}└─ {}", indent, sym.qualified_name);
    } else {
        println!("{}", sym.qualified_name);
    }

    if depth >= max_depth {
        return;
    }

    let mut seen_targets = HashSet::new();
    for edge in &index.edges {
        if edge.from_symbol_id == Some(curr_id)
            && let Some(to_id) = edge.to_symbol_id
        {
            if !seen_targets.insert(to_id) {
                continue;
            }
            if !visited.contains(&to_id) {
                visited.insert(to_id);
                print_slice_tree_helper(index, to_id, depth + 1, max_depth, visited, printed_count);
                visited.remove(&to_id);
            } else {
                if let Some(target_sym) = index.symbols.iter().find(|s| s.id == Some(to_id)) {
                    println!(
                        "{}  └─ {} (already visited)",
                        indent, target_sym.qualified_name
                    );
                }
            }
        }
    }
}

impl From<CliGraphContextMode> for ctx_codegraph::GraphContextMode {
    fn from(mode: CliGraphContextMode) -> Self {
        match mode {
            CliGraphContextMode::Callers => ctx_codegraph::GraphContextMode::Callers,
            CliGraphContextMode::Callees => ctx_codegraph::GraphContextMode::Callees,
            CliGraphContextMode::Dependencies => ctx_codegraph::GraphContextMode::Dependencies,
            CliGraphContextMode::Dependents => ctx_codegraph::GraphContextMode::Dependents,
            CliGraphContextMode::ForwardSlice => ctx_codegraph::GraphContextMode::ForwardSlice,
            CliGraphContextMode::ReverseSlice => ctx_codegraph::GraphContextMode::ReverseSlice,
            CliGraphContextMode::Neighborhood => ctx_codegraph::GraphContextMode::Neighborhood,
        }
    }
}

fn kind_to_str(kind: ctx_codegraph::LanguageObjectKind) -> &'static str {
    match kind {
        ctx_codegraph::LanguageObjectKind::Function => "fn",
        ctx_codegraph::LanguageObjectKind::Method => "fn",
        ctx_codegraph::LanguageObjectKind::Struct => "struct",
        ctx_codegraph::LanguageObjectKind::Enum => "enum",
        ctx_codegraph::LanguageObjectKind::Trait => "trait",
        ctx_codegraph::LanguageObjectKind::Impl => "impl",
        ctx_codegraph::LanguageObjectKind::Module => "mod",
        ctx_codegraph::LanguageObjectKind::Class => "class",
        ctx_codegraph::LanguageObjectKind::Interface => "interface",
        ctx_codegraph::LanguageObjectKind::TypeAlias => "type",
        ctx_codegraph::LanguageObjectKind::Constant => "const",
        ctx_codegraph::LanguageObjectKind::Variable => "var",
        ctx_codegraph::LanguageObjectKind::Unknown => "unknown",
    }
}

fn get_file_span_content(
    path: &Path,
    start_line: usize,
    end_line: usize,
) -> Result<String, std::io::Error> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    if start_line == 0 || start_line > lines.len() {
        return Ok("".to_string());
    }
    let end = std::cmp::min(end_line, lines.len());
    if start_line > end {
        return Ok("".to_string());
    }
    let mut result = String::new();
    for line in &lines[(start_line - 1)..end] {
        result.push_str(line);
        result.push('\n');
    }
    Ok(result)
}

fn get_markdown_lang(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("rs") => "rust",
        Some("py") => "python",
        Some("js") => "javascript",
        Some("ts") => "typescript",
        Some("tsx") => "tsx",
        Some("jsx") => "jsx",
        Some("html") => "html",
        Some("css") => "css",
        Some("json") => "json",
        Some("toml") => "toml",
        Some("md") => "markdown",
        Some("sh") => "bash",
        Some("yaml") | Some("yml") => "yaml",
        Some("go") => "go",
        Some("c") => "c",
        Some("cpp") | Some("cc") | Some("h") | Some("hpp") => "cpp",
        Some("java") => "java",
        Some("kt") => "kotlin",
        Some("swift") => "swift",
        Some("txt") => "text",
        _ => "",
    }
}

fn render_graph_context(
    result: &ctx_codegraph::GraphContextResult,
    root_path: &Path,
    mode: ctx_codegraph::GraphContextMode,
    depth: usize,
    max_nodes: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut out = String::new();

    // Header
    out.push_str("# Graph Context\n\n");
    let root_kind = kind_to_str(result.root.kind);
    let root_rel_path = result
        .root
        .file_path
        .strip_prefix(root_path)
        .unwrap_or(&result.root.file_path);
    out.push_str(&format!("Root: {} {}\n", root_kind, result.root.name));
    out.push_str(&format!(
        "Path: {}:{}-{}\n",
        root_rel_path.display(),
        result.root.range.start_line,
        result.root.range.end_line
    ));
    out.push_str(&format!("Mode: {:?}\n", mode));
    out.push_str(&format!("Depth: {}\n", depth));
    out.push_str(&format!("Max nodes: {}\n\n", max_nodes));

    // Graph
    out.push_str("## Graph\n\n");
    let mut symbol_names = std::collections::HashMap::new();
    symbol_names.insert(result.root.id, result.root.qualified_name.clone());
    for node in &result.nodes {
        symbol_names.insert(node.id, node.qualified_name.clone());
    }

    let mut edge_lines = Vec::new();
    for edge in &result.edges {
        let from_name = symbol_names
            .get(&edge.from)
            .cloned()
            .unwrap_or_else(|| format!("unknown_{:?}", edge.from));
        let to_name = symbol_names
            .get(&edge.to)
            .cloned()
            .unwrap_or_else(|| format!("unknown_{:?}", edge.to));
        edge_lines.push(format!("{} -> {}", from_name, to_name));
    }
    edge_lines.sort();
    for line in edge_lines {
        out.push_str(&line);
        out.push('\n');
    }
    out.push('\n');

    // Included Symbols
    out.push_str("## Included Symbols\n\n");
    let mut symbols_list = Vec::new();

    let format_symbol = |obj: &ctx_codegraph::LanguageObject| -> String {
        let kind = kind_to_str(obj.kind);
        let rel_path = obj
            .file_path
            .strip_prefix(root_path)
            .unwrap_or(&obj.file_path);
        format!(
            "- {} {} \u{2014} {}:{}-{}",
            kind,
            obj.name,
            rel_path.display(),
            obj.range.start_line,
            obj.range.end_line
        )
    };

    symbols_list.push(format_symbol(&result.root));
    for node in &result.nodes {
        symbols_list.push(format_symbol(node));
    }
    symbols_list.sort();

    for sym_line in symbols_list {
        out.push_str(&sym_line);
        out.push('\n');
    }
    out.push('\n');

    // Files
    out.push_str("## Files\n\n");

    let mut sorted_files = result.files.clone();
    sorted_files.sort_by(|a, b| match a.file_path.cmp(&b.file_path) {
        std::cmp::Ordering::Equal => a.range.start_line.cmp(&b.range.start_line),
        other => other,
    });

    for file_span in sorted_files {
        let rel_path = file_span
            .file_path
            .strip_prefix(root_path)
            .unwrap_or(&file_span.file_path);
        out.push_str(&format!(
            "### {}:{}-{}\n\n",
            rel_path.display(),
            file_span.range.start_line,
            file_span.range.end_line
        ));

        let content = match get_file_span_content(
            &file_span.file_path,
            file_span.range.start_line,
            file_span.range.end_line,
        ) {
            Ok(c) => c,
            Err(e) => format!("Error reading file: {}\n", e),
        };

        let lang = get_markdown_lang(&file_span.file_path);
        out.push_str(&format!("```{}\n", lang));
        out.push_str(&content);
        if !content.ends_with('\n') && !content.is_empty() {
            out.push('\n');
        }
        out.push_str("```\n\n");
    }

    Ok(out)
}

fn handle_healthcheck_command(
    health: HealthcheckCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    if health.format != "text" && health.format != "json" {
        return Err(format!(
            "unsupported format: {} (use text or json)",
            health.format
        )
        .into());
    }

    let report = ctx_health::run_healthcheck(
        &health.path,
        env!("CARGO_PKG_VERSION"),
        ctx_health::HealthcheckOptions {
            probe: health.probe,
        },
    );

    if health.format == "json" {
        println!("{}", ctx_health::render_json(&report)?);
    } else {
        print!("{}", ctx_health::render_text(&report));
    }

    if report.summary.overall == ctx_health::CheckStatus::Fail {
        std::process::exit(1);
    }

    Ok(())
}

fn handle_stats_command(stats: StatsCommand) -> Result<(), Box<dyn std::error::Error>> {
    let path = &stats.path;
    println!("📊 ctx stats for {}", path.display());
    let config = ctx_config::find_and_load_config(path).unwrap_or_default();

    // Note if collection disabled (affects what MCP will persist).
    if let Some(false) = config.stats_enabled {
        println!("(stats_enabled=false in .ctxconfig; MCP usage collection disabled)");
    }

    let mode = config.mode.unwrap_or(Mode::Smart);
    let scan_options = ctx_models::ScanOptions {
        max_depth: config.max_depth,
        max_file_size: config.max_file_size.unwrap_or(512 * 1024),
        mode,
        exclude: config.exclude,
    };
    let scan_result = ctx_core::scan(path, scan_options)?;
    println!(
        "Project (mode: {:?}): files={}, dirs={}, lines={}, tokens={}",
        mode,
        scan_result.summary.files,
        scan_result.summary.dirs,
        scan_result.summary.lines,
        scan_result.summary.tokens
    );

    // Detailed codegraph index: open directly (similar to resources read_index_status) without loading full service.
    // Reuses open_db + metadata queries + get_index_state for counts, schema, resolver, last build info.
    let workspace_root = ctx_codegraph::storage::find_workspace_root(path);
    let db_path = workspace_root.join(".ctx-codegraph/codegraph.sqlite");
    if db_path.exists() {
        println!("Codegraph index present at {}", db_path.display());

        let options = ctx_codegraph::index::BuildIndexOptions::default();
        let state = ctx_codegraph::storage::get_index_state(&workspace_root, &options)
            .unwrap_or(ctx_codegraph::model::IndexState::Missing);
        println!("- State: {:?}", state);

        match ctx_codegraph::open_db(&workspace_root) {
            Ok(conn) => {
                let file_count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
                    .unwrap_or(0);
                let symbol_count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
                    .unwrap_or(0);
                let edge_count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))
                    .unwrap_or(0);
                println!("- Files indexed: {}", file_count);
                println!("- Symbols: {}", symbol_count);
                println!("- Edges: {}", edge_count);

                let meta_value = |key: &str| -> Option<String> {
                    conn.query_row("SELECT value FROM metadata WHERE key = ?", [key], |row| {
                        row.get::<_, String>(0)
                    })
                    .ok()
                };
                if let Some(v) = meta_value("schema_version") {
                    println!("- Schema version: {}", v);
                }
                if let Some(v) = meta_value("resolver_id") {
                    println!("- Resolver: {}", v);
                }
                if let Some(v) = meta_value("change_detection_strategy") {
                    println!("- Change detection: {}", v);
                }
                if let Some(v) = meta_value("indexer_version") {
                    println!("- Indexer version: {}", v);
                }
                if let Some(v) = meta_value("lsp_enrichment") {
                    println!("- LSP enrichment: {}", v);
                }
                if let Ok(meta) = std::fs::metadata(&db_path) {
                    if let Ok(mtime) = meta.modified() {
                        println!("- DB mtime (last build approx): {:?}", mtime);
                    }
                }
            }
            Err(e) => println!("- (could not open DB: {})", e),
        }
    } else {
        println!("Codegraph index: none (run `ctx graph build`)");
    }

    // MCP last known: read persisted JSON from mcp_last_stats key (written by MCP shutdown).
    if let Some(json_str) = ctx_codegraph::storage::read_metadata(&workspace_root, "mcp_last_stats") {
        println!("Last MCP session stats (persisted from mcp_last_stats):");
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
            if let Ok(p) = serde_json::to_string_pretty(&val) {
                println!("{}", p);
            } else {
                println!("{}", json_str);
            }
        } else {
            println!("{}", json_str);
        }
    } else {
        println!("MCP usage: no prior persisted stats (run `ctx mcp serve` to collect; also ctx://stats/mcp live)");
    }

    Ok(())
}

/// Basic auto-install for the MCP server.
/// Targets common clients using the standard {"mcpServers": {"ctx": {"command": "...", "args": ["mcp"] }}} format.
/// Respects --clients and --dry-run. Uses absolute path to current binary when possible.
fn handle_mcp_install(args: InstallCommand) -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "ctx".to_string());

    let default_clients = vec![
        "claude".to_string(),
        "cursor".to_string(),
        "gemini".to_string(),
    ];
    let clients: Vec<String> = if args.clients.is_empty() {
        default_clients
    } else {
        args.clients
    };

    println!("Installing ctx MCP server using binary: {}", exe);
    println!("Target clients: {}", clients.join(", "));

    // Detection based on real filesystem (inspired by user discovery commands)
    println!("\nDetection on this machine:");
    let home = std::env::var_os("HOME").map(PathBuf::from);
    detect_and_print(
        "Claude Desktop",
        home.as_ref()
            .map(|h| h.join("Library/Application Support/Claude")),
    );
    detect_and_print("Cursor", home.as_ref().map(|h| h.join(".cursor")));
    detect_and_print("Gemini", home.as_ref().map(|h| h.join(".gemini/config")));
    detect_and_print(
        "Continue",
        home.as_ref().map(|h| h.join(".config/continue")),
    );
    detect_and_print(
        "VS Code",
        home.as_ref()
            .map(|h| h.join("Library/Application Support/Code/User")),
    );
    if Path::new(".cursor").exists() || Path::new("mcp.json").exists() {
        println!("  - Cursor project-level config possible in current dir");
    }

    if args.dry_run {
        println!("\n(dry-run mode — no files will be written)");
    }

    let mut any_written = false;

    for client in &clients {
        match client.as_str() {
            "claude" | "claude-desktop" => {
                // macOS primary; extend for other OSes as needed
                if let Some(home) = std::env::var_os("HOME") {
                    let path = PathBuf::from(home)
                        .join("Library/Application Support/Claude/claude_desktop_config.json");
                    any_written |= write_mcp_entry(
                        &path,
                        &exe,
                        args.dry_run,
                        "Claude Desktop",
                        "mcpServers",
                        false,
                    )?;
                }
            }
            "cursor" => {
                // Global
                if let Some(home) = std::env::var_os("HOME") {
                    let global = PathBuf::from(home).join(".cursor/mcp.json");
                    any_written |= write_mcp_entry(
                        &global,
                        &exe,
                        args.dry_run,
                        "Cursor (global)",
                        "mcpServers",
                        false,
                    )?;
                }
                // Project-local (very common and recommended)
                let local = PathBuf::from(".cursor/mcp.json");
                any_written |= write_mcp_entry(
                    &local,
                    &exe,
                    args.dry_run,
                    "Cursor (project)",
                    "mcpServers",
                    false,
                )?;
            }
            "gemini" => {
                if let Some(home) = std::env::var_os("HOME") {
                    let path = PathBuf::from(home).join(".gemini/config/mcp_config.json");
                    any_written |=
                        write_mcp_entry(&path, &exe, args.dry_run, "Gemini", "mcpServers", false)?;
                }
            }
            "continue" => {
                if let Some(home) = std::env::var_os("HOME") {
                    // Common locations observed in the wild (mac + linux ~/.config)
                    let path = PathBuf::from(home).join(".config/continue/mcpServers/mcp.json");
                    any_written |= write_mcp_entry(
                        &path,
                        &exe,
                        args.dry_run,
                        "Continue.dev",
                        "mcpServers",
                        false,
                    )?;
                }
            }
            "code" | "vscode" => {
                if let Some(home) = std::env::var_os("HOME") {
                    // VS Code (and some derivatives) on macOS
                    let path =
                        PathBuf::from(home).join("Library/Application Support/Code/User/mcp.json");
                    any_written |=
                        write_mcp_entry(&path, &exe, args.dry_run, "VS Code", "servers", true)?;
                }
            }
            other => {
                println!("  Skipping unknown client '{}'", other);
            }
        }
    }

    if any_written && !args.dry_run {
        println!(
            "\nDone. Restart the target application(s) (Claude, Cursor, Gemini, etc.) for the MCP server to appear."
        );
    } else if !any_written {
        println!("\nNo changes made (targets may not exist or were already configured).");
    }

    Ok(())
}

/// Helper: ensure parent dir, load or create the appropriate servers json, set/overwrite the "ctx" entry.
/// root_key: "mcpServers" (most) or "servers" (VS Code style)
/// with_type: if true, add "type": "stdio" (for VS Code style)
fn write_mcp_entry(
    target: &Path,
    exe: &str,
    dry_run: bool,
    label: &str,
    root_key: &str,
    with_type: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut changed = false;

    if let Some(parent) = target.parent() {
        if !dry_run {
            let _ = fs::create_dir_all(parent);
        }
    }

    let mut doc: serde_json::Value = if target.exists() {
        let content = fs::read_to_string(target)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let servers = doc
        .as_object_mut()
        .and_then(|o| {
            o.entry(root_key)
                .or_insert(serde_json::json!({}))
                .as_object_mut()
        })
        .ok_or_else(|| format!("invalid {} structure", root_key))?;

    let mut entry = serde_json::json!({
        "command": exe,
        "args": ["mcp"]
    });
    if with_type {
        entry["type"] = serde_json::json!("stdio");
    }

    let existing = servers.get("ctx");
    if existing.is_none() || existing != Some(&entry) {
        servers.insert("ctx".to_string(), entry);
        changed = true;
    }

    if changed {
        let pretty = serde_json::to_string_pretty(&doc)?;
        let action = if target.exists() { "update" } else { "create" };
        if dry_run {
            println!(
                "  [dry-run] Would {} {} ({})",
                action,
                label,
                target.display()
            );
            println!(
                "  Content diff would affect the 'ctx' entry under {}.",
                root_key
            );
        } else {
            fs::write(target, pretty + "\n")?;
            println!(
                "  ✓ {}d {} entry at {} ({})",
                action,
                label,
                target.display(),
                label
            );
        }
    } else {
        println!(
            "  = {} already has an up-to-date ctx entry ({})",
            label,
            target.display()
        );
    }

    Ok(changed)
}

/// Simple detector to report what MCP-capable agents appear to be present.
fn detect_and_print(name: &str, path: Option<PathBuf>) {
    if let Some(p) = path {
        if p.exists() {
            println!("  ✓ {} config dir found: {}", name, p.display());
        } else {
            println!("  - {} not detected (no {})", name, p.display());
        }
    }
}
