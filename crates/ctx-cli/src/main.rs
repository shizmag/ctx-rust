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

    let format = args.format.map(Format::from).unwrap_or(Format::Markdown);

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

    #[test]
    fn test_cli_passes_path_to_tui() {
        let args = Args {
            command: None,
            path: PathBuf::from("/mock/path/to/project"),
            format: None,
            mode: None,
            max_depth: None,
            max_file_size: None,
            output: None,
            no_stats: false,
            list_hidden: false,
            clipboard: false,
            code: false,
            interactive: true,
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
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Analyze the project and query dependency or symbol relationships
    #[command(visible_alias = "g")]
    Graph(GraphCommand),
}

#[derive(clap::Args, Debug)]
#[command(
    about = "Analyze the project and build/query a symbol and call graph",
    long_about = "The graph command scans the selected project files and builds a local SQLite index of \
                 modules, symbols, calls, and dependencies. You can build this index and query it to \
                 find all symbols, view callers/callees of a symbol, or compute a call slice tree \
                 to understand how functions are connected.",
    after_help = "Examples:\n  \
                  ctx graph build\n  \
                  ctx graph symbols\n  \
                  ctx graph calls my_function\n  \
                  ctx graph callers my_function\n  \
                  ctx graph slice my_function\n  \
                  ctx g symbols"
)]
struct GraphCommand {
    #[command(subcommand)]
    command: GraphSubcommand,

    /// Target directory path containing the project to analyze
    #[arg(default_value = ".", global = true)]
    path: PathBuf,

    /// Disable rust-analyzer database fallback (forces tree-sitter fallback only)
    #[arg(long, global = true)]
    no_rust_analyzer: bool,
}

#[derive(clap::Subcommand, Debug)]
enum GraphSubcommand {
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
}

fn handle_graph_command(graph_args: GraphCommand) -> Result<(), Box<dyn std::error::Error>> {
    use ctx_codegraph::BuildIndexOptions;
    use std::collections::HashMap;

    let use_rust_analyzer = !graph_args.no_rust_analyzer;

    match graph_args.command {
        GraphSubcommand::Build => {
            println!("Building codegraph index...");
            let options = BuildIndexOptions {
                use_rust_analyzer,
                max_depth: None,
                include_tests: true,
            };
            ctx_codegraph::rebuild_index_db(&graph_args.path, options)?;
            println!("Index successfully built at .ctx-codegraph/codegraph.sqlite");
        }
        GraphSubcommand::Symbols { mut query } => {
            let mut target_path = graph_args.path.clone();
            if let Some(ref q) = query {
                if std::path::Path::new(q).is_dir() {
                    target_path = std::path::PathBuf::from(q);
                    query = None;
                }
            }

            let conn = get_connection_or_rebuild(&target_path, use_rust_analyzer)?;

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
            let conn = get_connection_or_rebuild(&graph_args.path, use_rust_analyzer)?;
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
                            "  - {} -> {} ({:?})",
                            edge.raw_name, t.qualified_name, edge.confidence
                        ),
                        None => println!(
                            "  - {} -> [Unresolved] ({:?})",
                            edge.raw_name, edge.confidence
                        ),
                    }
                }
            }
        }
        GraphSubcommand::Callers { symbol } => {
            let conn = get_connection_or_rebuild(&graph_args.path, use_rust_analyzer)?;
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
                    println!(
                        "  - {} via `{}` at L{}:{} ({:?})",
                        caller.qualified_name,
                        edge.raw_name,
                        edge.call_range.start_line,
                        edge.call_range.start_col,
                        edge.confidence
                    );
                }
            }
        }
        GraphSubcommand::Slice { symbol } => {
            let conn = get_connection_or_rebuild(&graph_args.path, use_rust_analyzer)?;
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
            print_slice_tree_helper(&index, sym.id.unwrap(), 0, 10, &mut visited);
        }
    }

    Ok(())
}

fn get_connection_or_rebuild(
    path: &Path,
    use_rust_analyzer: bool,
) -> Result<rusqlite::Connection, Box<dyn std::error::Error>> {
    let db_path = path.join(".ctx-codegraph/codegraph.sqlite");
    if !db_path.exists() {
        println!("Index not found. Building codegraph index...");
        let options = ctx_codegraph::BuildIndexOptions {
            use_rust_analyzer,
            max_depth: None,
            include_tests: true,
        };
        ctx_codegraph::rebuild_index_db(path, options)?;
    }
    let conn = ctx_codegraph::open_db(path)?;
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
) {
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
        if edge.from == curr_id {
            if let Some(to_id) = edge.to {
                if !seen_targets.insert(to_id) {
                    continue;
                }
                if !visited.contains(&to_id) {
                    visited.insert(to_id);
                    print_slice_tree_helper(index, to_id, depth + 1, max_depth, visited);
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
}
