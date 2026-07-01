use std::fs;
use std::path::PathBuf;
use clap::Parser;
use ctx_models::{Mode, ScanOptions};
use ctx_render::{Format, RenderOptions};

#[derive(Parser, Debug)]
#[command(
    name = "ctx", 
    version, 
    about = "✨ ctx: A highly informative, interactive directory tree visualizer and LLM context gatherer.\n\nRuns a beautiful, interactive TUI or outputs detailed markdown/plain/xml context for your files."
)]
struct Args {
    /// Target directory path to analyze.
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Format for the full context output. Choose from: 'markdown' (or 'md'), 'xml', 'plain' (or 'text', 'txt').
    #[arg(short, long, default_value = "markdown")]
    format: String,

    /// Gathering strategy mode: 'smart' (respects gitignore + sensible skips), 'all' (scans all files), 'code' (prioritizes code files), 'docs' (prioritizes docs/markdown), 'llm' (structures with token counts).
    #[arg(short, long, default_value = "smart")]
    mode: String,

    /// Restrict directory traversal to the specified maximum depth.
    #[arg(long)]
    max_depth: Option<usize>,

    /// Exclude files exceeding this size limit in bytes from the final context contents.
    #[arg(long, default_value_t = 512 * 1024)]
    max_file_size: u64,

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

fn parse_mode(s: &str) -> Result<Mode, String> {
    match s.to_lowercase().as_str() {
        "smart" => Ok(Mode::Smart),
        "all" => Ok(Mode::All),
        "code" => Ok(Mode::Code),
        "docs" => Ok(Mode::Docs),
        "llm" => Ok(Mode::Llm),
        _ => Err(format!(
            "invalid mode '{}'. Choose from: smart, all, code, docs, llm",
            s
        )),
    }
}

fn parse_format(s: &str) -> Result<Format, String> {
    match s.to_lowercase().as_str() {
        "markdown" | "md" => Ok(Format::Markdown),
        "xml" => Ok(Format::Xml),
        "plain" | "txt" | "text" => Ok(Format::Plain),
        _ => Err(format!(
            "invalid format '{}'. Choose from: markdown (md), xml, plain (txt)",
            s
        )),
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if args.interactive {
        return ctx_tui::run_default_interactive_menu();
    }

    let mode = parse_mode(&args.mode)?;
    let format = parse_format(&args.format)?;

    let scan_options = ScanOptions {
        max_depth: args.max_depth,
        max_file_size: args.max_file_size,
        mode,
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
            max_file_size: args.max_file_size,
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
