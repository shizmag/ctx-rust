use std::io::{BufRead, Write};
use std::path::PathBuf;

pub fn run_interactive_menu<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
) -> Result<(), crate::error::TuiError> {
    let mut path = PathBuf::from(".");
    let mut mode = "smart".to_string();
    let mut format = "markdown".to_string();
    let mut max_depth: Option<usize> = None;
    let mut max_file_size = 512 * 1024;

    loop {
        writeln!(writer, "\n=== CTX Terminal Interactive Menu ===")?;
        writeln!(writer, "1. Set scan path (current: {})", path.display())?;
        writeln!(writer, "2. Set scan mode (current: {})", mode)?;
        writeln!(writer, "3. Set output format (current: {})", format)?;
        writeln!(
            writer,
            "4. Set max depth (current: {})",
            max_depth
                .map(|d| d.to_string())
                .unwrap_or_else(|| "None".to_string())
        )?;
        writeln!(
            writer,
            "5. Set max file size (current: {} KB)",
            max_file_size / 1024
        )?;
        writeln!(writer, "6. Run scan and print context to stdout")?;
        writeln!(writer, "7. Run scan and copy context to clipboard")?;
        writeln!(writer, "8. Exit")?;
        write!(writer, "Choose option (1-8): ")?;
        writer.flush()?;

        let mut input = String::new();
        if reader.read_line(&mut input)? == 0 {
            break;
        }
        let choice = input.trim();

        match choice {
            "1" => {
                write!(writer, "Enter new scan path: ")?;
                writer.flush()?;
                let mut path_input = String::new();
                if reader.read_line(&mut path_input)? == 0 {
                    break;
                }
                let new_path = path_input.trim();
                if !new_path.is_empty() {
                    path = PathBuf::from(new_path);
                }
            }
            "2" => {
                writeln!(writer, "Select mode:")?;
                writeln!(writer, "  1. Smart (default)")?;
                writeln!(writer, "  2. All")?;
                writeln!(writer, "  3. Code")?;
                writeln!(writer, "  4. Docs")?;
                writeln!(writer, "  5. Llm")?;
                write!(writer, "Choose (1-5): ")?;
                writer.flush()?;
                let mut mode_input = String::new();
                if reader.read_line(&mut mode_input)? == 0 {
                    break;
                }
                match mode_input.trim() {
                    "1" => mode = "smart".to_string(),
                    "2" => mode = "all".to_string(),
                    "3" => mode = "code".to_string(),
                    "4" => mode = "docs".to_string(),
                    "5" => mode = "llm".to_string(),
                    _ => writeln!(writer, "Invalid option, keeping: {}", mode)?,
                }
            }
            "3" => {
                writeln!(writer, "Select format:")?;
                writeln!(writer, "  1. Markdown (default)")?;
                writeln!(writer, "  2. XML")?;
                writeln!(writer, "  3. Plain text")?;
                write!(writer, "Choose (1-3): ")?;
                writer.flush()?;
                let mut fmt_input = String::new();
                if reader.read_line(&mut fmt_input)? == 0 {
                    break;
                }
                match fmt_input.trim() {
                    "1" => format = "markdown".to_string(),
                    "2" => format = "xml".to_string(),
                    "3" => format = "plain".to_string(),
                    _ => writeln!(writer, "Invalid option, keeping: {}", format)?,
                }
            }
            "4" => {
                write!(writer, "Enter max depth (leave blank for None): ")?;
                writer.flush()?;
                let mut depth_input = String::new();
                if reader.read_line(&mut depth_input)? == 0 {
                    break;
                }
                let d = depth_input.trim();
                if d.is_empty() {
                    max_depth = None;
                } else if let Ok(depth) = d.parse::<usize>() {
                    max_depth = Some(depth);
                } else {
                    writeln!(writer, "Invalid number.")?;
                }
            }
            "5" => {
                write!(writer, "Enter max file size in KB: ")?;
                writer.flush()?;
                let mut size_input = String::new();
                if reader.read_line(&mut size_input)? == 0 {
                    break;
                }
                if let Ok(size_kb) = size_input.trim().parse::<u64>() {
                    max_file_size = size_kb * 1024;
                } else {
                    writeln!(writer, "Invalid number.")?;
                }
            }
            "6" => {
                writeln!(writer, "\n--- Running scan... ---")?;
                let parsed_mode = match mode.as_str() {
                    "smart" => ctx_models::Mode::Smart,
                    "all" => ctx_models::Mode::All,
                    "code" => ctx_models::Mode::Code,
                    "docs" => ctx_models::Mode::Docs,
                    "llm" => ctx_models::Mode::Llm,
                    _ => ctx_models::Mode::Smart,
                };

                let parsed_format = match format.as_str() {
                    "markdown" => ctx_render::Format::Markdown,
                    "xml" => ctx_render::Format::Xml,
                    "plain" => ctx_render::Format::Plain,
                    _ => ctx_render::Format::Markdown,
                };

                let scan_options = ctx_models::ScanOptions {
                    max_depth,
                    max_file_size,
                    mode: parsed_mode,
                    exclude: Vec::new(),
                };
                match ctx_core::scan(&path, scan_options) {
                    Ok(scan_result) => {
                        let render_options = ctx_render::RenderOptions {
                            format: parsed_format,
                            include_stats: true,
                            max_file_size,
                        };
                        match ctx_render::render(&scan_result, &render_options) {
                            Ok(rendered) => {
                                writeln!(writer, "{}", rendered)?;
                            }
                            Err(e) => writeln!(writer, "Rendering error: {}", e)?,
                        }
                    }
                    Err(e) => writeln!(writer, "Scanning error: {}", e)?,
                }
            }
            "7" => {
                writeln!(writer, "\n--- Running scan and copying to clipboard... ---")?;
                let parsed_mode = match mode.as_str() {
                    "smart" => ctx_models::Mode::Smart,
                    "all" => ctx_models::Mode::All,
                    "code" => ctx_models::Mode::Code,
                    "docs" => ctx_models::Mode::Docs,
                    "llm" => ctx_models::Mode::Llm,
                    _ => ctx_models::Mode::Smart,
                };

                let parsed_format = match format.as_str() {
                    "markdown" => ctx_render::Format::Markdown,
                    "xml" => ctx_render::Format::Xml,
                    "plain" => ctx_render::Format::Plain,
                    _ => ctx_render::Format::Markdown,
                };

                let scan_options = ctx_models::ScanOptions {
                    max_depth,
                    max_file_size,
                    mode: parsed_mode,
                    exclude: Vec::new(),
                };
                match ctx_core::scan(&path, scan_options) {
                    Ok(scan_result) => {
                        let render_options = ctx_render::RenderOptions {
                            format: parsed_format,
                            include_stats: true,
                            max_file_size,
                        };
                        match ctx_render::render(&scan_result, &render_options) {
                            Ok(rendered) => match arboard::Clipboard::new() {
                                Ok(mut ctx_clipboard) => {
                                    if let Err(e) = ctx_clipboard.set_text(rendered) {
                                        writeln!(writer, "Clipboard error: {}", e)?;
                                    } else {
                                        writeln!(
                                            writer,
                                            "Context successfully copied to clipboard! ({} files, {} tokens)",
                                            scan_result.summary.files, scan_result.summary.tokens
                                        )?;
                                    }
                                }
                                Err(e) => {
                                    writeln!(writer, "Clipboard initialization error: {}", e)?
                                }
                            },
                            Err(e) => writeln!(writer, "Rendering error: {}", e)?,
                        }
                    }
                    Err(e) => writeln!(writer, "Scanning error: {}", e)?,
                }
            }
            "8" | "exit" | "quit" => {
                writeln!(writer, "Goodbye!")?;
                break;
            }
            _ => {
                writeln!(writer, "Unknown option.")?;
            }
        }
    }

    Ok(())
}
