use std::fs;
use std::path::PathBuf;
use clap::Parser;
use ctx_models::{Mode, ScanOptions};
use ctx_render::{Format, RenderOptions};

#[derive(Parser, Debug)]
#[command(name = "ctx", version, about = "Context gatherer for LLMs")]
struct Args {
    /// Path to the directory to scan
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Output format: markdown (or md), xml, plain (or text/txt)
    #[arg(short, long, default_value = "markdown")]
    format: String,

    /// Scan mode: smart, all, code, docs, llm
    #[arg(short, long, default_value = "smart")]
    mode: String,

    /// Maximum depth to scan
    #[arg(long)]
    max_depth: Option<usize>,

    /// Maximum file size in bytes to read content
    #[arg(long, default_value_t = 512 * 1024)]
    max_file_size: u64,

    /// Output file path (defaults to stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Exclude statistics in the output
    #[arg(long)]
    no_stats: bool,

    /// Print lists of skipped/hidden files to stderr
    #[arg(long)]
    list_hidden: bool,

    /// Copy the output to the system clipboard
    #[arg(short, long)]
    clipboard: bool,

    /// Run in interactive mode (TUI)
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
            "Context copied to clipboard! ({} files, {} tokens)",
            scan_result.summary.files, scan_result.summary.tokens
        );
    } else if let Some(output_path) = args.output {
        fs::write(&output_path, rendered)?;
        println!("Context saved to {}", output_path.display());
    } else {
        print!("{}", rendered);
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
