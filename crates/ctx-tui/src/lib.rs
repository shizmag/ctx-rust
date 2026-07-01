use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use ctx_models::{NodeKind, TreeNode};

struct TuiApp {
    path: PathBuf,
    files: Vec<TuiFileItem>,
    list_state: ListState,
    message: Option<(String, std::time::Instant)>,
}

struct TuiFileItem {
    path: PathBuf,
    rel_path: String,
    tokens: usize,
    lines: usize,
    bytes: u64,
    is_text: bool,
    checked: bool,
}

impl TuiApp {
    fn new(path: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let scan_result = ctx_core::scan(&path, ctx_models::ScanOptions::default())?;
        let mut files = Vec::new();
        collect_files(&scan_result.root, &scan_result.root.path, &mut files);
        
        let mut list_state = ListState::default();
        if !files.is_empty() {
            list_state.select(Some(0));
        }

        Ok(Self {
            path,
            files,
            list_state,
            message: None,
        })
    }

    fn rescan(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let scan_result = ctx_core::scan(&self.path, ctx_models::ScanOptions::default())?;
        let mut new_files = Vec::new();
        collect_files(&scan_result.root, &scan_result.root.path, &mut new_files);

        for new_item in &mut new_files {
            if let Some(old_item) = self.files.iter().find(|f| f.path == new_item.path) {
                new_item.checked = old_item.checked;
            }
        }

        self.files = new_files;
        let selected = self.list_state.selected().unwrap_or(0);
        if self.files.is_empty() {
            self.list_state.select(None);
        } else if selected >= self.files.len() {
            self.list_state.select(Some(self.files.len() - 1));
        } else {
            self.list_state.select(Some(selected));
        }

        self.message = Some((
            "Rescanned directory!".to_string(),
            std::time::Instant::now(),
        ));
        Ok(())
    }
}

fn collect_files(node: &TreeNode, root_path: &Path, files: &mut Vec<TuiFileItem>) {
    if node.kind == NodeKind::File {
        let rel_path = match node.path.strip_prefix(root_path) {
            Ok(rel) => rel.to_string_lossy().to_string(),
            Err(_) => node.path.to_string_lossy().to_string(),
        };
        
        let is_text = node.stats.lines > 0 || node.stats.bytes == 0;
        files.push(TuiFileItem {
            path: node.path.clone(),
            rel_path,
            tokens: node.stats.tokens,
            lines: node.stats.lines,
            bytes: node.stats.bytes,
            is_text,
            checked: true,
        });
    }

    for child in &node.children {
        collect_files(child, root_path, files);
    }
}

fn copy_selection_to_clipboard(app: &TuiApp) -> Result<String, Box<dyn std::error::Error>> {
    let mut out = String::new();
    let root_name = app.path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    let checked_files: Vec<&TuiFileItem> = app.files.iter().filter(|f| f.checked).collect();
    let total_tokens: usize = checked_files.iter().map(|f| f.tokens).sum();

    out.push_str(&format!("Project Context: {}\n", root_name));
    out.push_str(&format!("Selected files: {} | Total tokens: {}\n\n", checked_files.len(), total_tokens));

    out.push_str("=== DIRECTORY STRUCTURE (SELECTED FILES) ===\n");
    for f in &checked_files {
        out.push_str(&format!("├── {} ({} tokens)\n", f.rel_path, f.tokens));
    }
    out.push_str("\n=== FILE CONTENTS ===\n\n");

    for f in &checked_files {
        out.push_str(&format!("--- FILE: {} ({} tokens) ---\n", f.rel_path, f.tokens));
        if !f.is_text {
            out.push_str("[File skipped: Binary or non-UTF8]\n\n");
            continue;
        }
        match std::fs::read_to_string(&f.path) {
            Ok(content) => {
                out.push_str(&content);
                if !content.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
            Err(e) => {
                out.push_str(&format!("[File skipped: Read error ({})]\n\n", e));
            }
        }
    }

    let mut ctx_clipboard = arboard::Clipboard::new()?;
    ctx_clipboard.set_text(out)?;

    Ok(format!(
        "Copied {} files ({} tokens) to clipboard!",
        checked_files.len(),
        total_tokens
    ))
}

pub fn run_default_interactive_menu() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = TuiApp::new(PathBuf::from("."))?;

    let run_res = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = run_res {
        println!("TUI Error: {}", err);
    }

    Ok(())
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut TuiApp,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != event::KeyEventKind::Release {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            return Ok(());
                        }
                        KeyCode::Char('c') | KeyCode::Enter => {
                            match copy_selection_to_clipboard(app) {
                                Ok(success_msg) => {
                                    app.message = Some((success_msg, std::time::Instant::now()));
                                }
                                Err(e) => {
                                    app.message = Some((format!("Error: {}", e), std::time::Instant::now()));
                                }
                            }
                        }
                        KeyCode::Char('r') => {
                            if let Err(e) = app.rescan() {
                                app.message = Some((format!("Error: {}", e), std::time::Instant::now()));
                            }
                        }
                        KeyCode::Char(' ') => {
                            if let Some(selected) = app.list_state.selected() {
                                if let Some(item) = app.files.get_mut(selected) {
                                    item.checked = !item.checked;
                                }
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if !app.files.is_empty() {
                                let selected = app.list_state.selected().unwrap_or(0);
                                let next = (selected + 1) % app.files.len();
                                app.list_state.select(Some(next));
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if !app.files.is_empty() {
                                let selected = app.list_state.selected().unwrap_or(0);
                                let prev = if selected == 0 {
                                    app.files.len() - 1
                                } else {
                                    selected - 1
                                };
                                app.list_state.select(Some(prev));
                            }
                        }
                        KeyCode::Char('g') => {
                            if !app.files.is_empty() {
                                app.list_state.select(Some(0));
                            }
                        }
                        KeyCode::Char('G') => {
                            if !app.files.is_empty() {
                                app.list_state.select(Some(app.files.len() - 1));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn ui(f: &mut ratatui::Frame, app: &mut TuiApp) {
    let size = f.size();

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(size);

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(main_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(0)])
        .split(body_chunks[1]);

    let items: Vec<ListItem> = app.files.iter().enumerate().map(|(idx, item)| {
        let checkbox = if item.checked { "[•] " } else { "[ ] " };
        let highlight = if app.list_state.selected() == Some(idx) { "▶ " } else { "  " };
        
        let style = if item.checked {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let span_style = if app.list_state.selected() == Some(idx) {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        };

        ListItem::new(Line::from(vec![
            Span::styled(highlight, Style::default().fg(Color::Yellow)),
            Span::styled(checkbox, Style::default().fg(Color::Cyan)),
            Span::styled(format!("{} ({} tokens)", item.rel_path, item.tokens), span_style),
        ]))
    }).collect();

    let files_block = Block::default()
        .borders(Borders::ALL)
        .title(" Files to Include ")
        .border_style(Style::default().fg(Color::Blue));
    let files_list = List::new(items)
        .block(files_block)
        .highlight_style(Style::default().bg(Color::Rgb(30, 30, 30)));
    f.render_stateful_widget(files_list, body_chunks[0], &mut app.list_state);

    let total_files = app.files.len();
    let checked_files = app.files.iter().filter(|f| f.checked).count();
    let total_tokens: usize = app.files.iter().map(|f| f.tokens).sum();
    let checked_tokens: usize = app.files.iter().filter(|f| f.checked).map(|f| f.tokens).sum();
    let checked_lines: usize = app.files.iter().filter(|f| f.checked).map(|f| f.lines).sum();
    let total_bytes: u64 = app.files.iter().map(|f| f.bytes).sum();
    let checked_bytes: u64 = app.files.iter().filter(|f| f.checked).map(|f| f.bytes).sum();

    let stats_text = vec![
        Line::from(vec![
            Span::raw("Scan Path: "),
            Span::styled(app.path.to_string_lossy().to_string(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::raw("Files: "),
            Span::styled(format!("{} / {}", checked_files, total_files), Style::default().fg(Color::Cyan)),
            Span::raw("  |  Tokens: "),
            Span::styled(format!("{} / {}", checked_tokens, total_tokens), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("  |  Lines: "),
            Span::styled(format!("{}", checked_lines), Style::default().fg(Color::Magenta)),
            Span::raw("  |  Size: "),
            Span::styled(format!("{} / {}", format_bytes(checked_bytes), format_bytes(total_bytes)), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw("Status: "),
            match &app.message {
                Some((msg, ts)) => {
                    if ts.elapsed().as_secs() < 4 {
                        Span::styled(msg, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                    } else {
                        Span::styled("Idle", Style::default().fg(Color::DarkGray))
                    }
                }
                None => Span::styled("Idle", Style::default().fg(Color::DarkGray)),
            }
        ]),
    ];

    let stats_block = Block::default()
        .borders(Borders::ALL)
        .title(" Context Summary ")
        .border_style(Style::default().fg(Color::Blue));
    let stats_paragraph = Paragraph::new(stats_text)
        .block(stats_block);
    f.render_widget(stats_paragraph, right_chunks[0]);

    let preview_text = if let Some(selected_idx) = app.list_state.selected() {
        if let Some(item) = app.files.get(selected_idx) {
            if !item.is_text {
                vec![Line::from(Span::styled("[Binary or Non-UTF8 File - No Preview]", Style::default().fg(Color::Red)))]
            } else {
                match std::fs::read_to_string(&item.path) {
                    Ok(content) => {
                        content.lines()
                            .take(40)
                            .map(|l| Line::from(Span::raw(l.to_string())))
                            .collect()
                    }
                    Err(e) => vec![Line::from(Span::styled(format!("Read error: {}", e), Style::default().fg(Color::Red)))],
                }
            }
        } else {
            vec![]
        }
    } else {
        vec![Line::from(Span::styled("No file selected", Style::default().fg(Color::DarkGray)))]
    };

    let preview_title = if let Some(selected_idx) = app.list_state.selected() {
        if let Some(item) = app.files.get(selected_idx) {
            format!(" Preview: {} ", item.rel_path)
        } else {
            " Preview ".to_string()
        }
    } else {
        " Preview ".to_string()
    };

    let preview_block = Block::default()
        .borders(Borders::ALL)
        .title(preview_title)
        .border_style(Style::default().fg(Color::Blue));
    let preview_paragraph = Paragraph::new(preview_text)
        .block(preview_block)
        .wrap(Wrap { trim: false });
    f.render_widget(preview_paragraph, right_chunks[1]);

    let help_spans = vec![
        Span::styled("j/k / ▲▼", Style::default().fg(Color::Yellow)),
        Span::raw(" Move | "),
        Span::styled("Space", Style::default().fg(Color::Yellow)),
        Span::raw(" Toggle | "),
        Span::styled("c / Enter", Style::default().fg(Color::Yellow)),
        Span::raw(" Copy Context | "),
        Span::styled("r", Style::default().fg(Color::Yellow)),
        Span::raw(" Rescan | "),
        Span::styled("q / Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" Quit"),
    ];
    let help_paragraph = Paragraph::new(Line::from(help_spans));
    f.render_widget(help_paragraph, main_chunks[1]);
}

pub fn run_interactive_menu<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
) -> Result<(), Box<dyn std::error::Error>> {
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
            max_depth.map(|d| d.to_string()).unwrap_or_else(|| "None".to_string())
        )?;
        writeln!(writer, "5. Set max file size (current: {} KB)", max_file_size / 1024)?;
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
                                match arboard::Clipboard::new() {
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
                                    Err(e) => writeln!(writer, "Clipboard initialization error: {}", e)?,
                                }
                            }
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

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
