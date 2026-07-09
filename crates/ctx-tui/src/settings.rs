//! Small dedicated settings TUI editor for `ctx setting`.
//! Reuses ratatui + crossterm patterns and color scheme from the main TUI (no big reuse of app/ui to keep focused/small).

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use std::io;
use std::path::PathBuf;

use ctx_config::{find_and_load_config, find_config, save_config, Config};
use ctx_models::Mode;

const GREEN: Color = Color::Rgb(158, 206, 106);
const GRAY: Color = Color::Rgb(86, 95, 137);
const ORANGE: Color = Color::Rgb(224, 175, 104);
const TEXT: Color = Color::Rgb(192, 202, 245);
const HIGHLIGHT_BG: Color = Color::Rgb(36, 40, 59);

struct SettingsState {
    dir: PathBuf,
    config: Config,
    selected: usize,
    input_mode: bool,
    input_buffer: String,
    input_target: Option<usize>, // which field is being edited
    message: Option<String>,
    // for exclude list simple add/remove (no inner cursor to keep small)
    // when selected == EXCLUDE_IDX, 'a' adds via input, 'r' removes last
}

const N_FIELDS: usize = 11;
const EXCLUDE_IDX: usize = 3;

impl SettingsState {
    fn new(dir: PathBuf) -> Self {
        let config = find_and_load_config(&dir).unwrap_or_default();
        Self {
            dir,
            config,
            selected: 0,
            input_mode: false,
            input_buffer: String::new(),
            input_target: None,
            message: None,
        }
    }

    fn field_label(&self, idx: usize) -> &'static str {
        match idx {
            0 => "mode (Scan)",
            1 => "max_depth (Scan)",
            2 => "max_file_size (Scan)",
            3 => "exclude (Scan)",
            4 => "default_format (AI/MCP)",
            5 => "mcp_target (AI/MCP)",
            6 => "use_lsp (AI/MCP)",
            7 => "stats_enabled (AI/MCP)",
            8 => "default_packing (AI/MCP)",
            9 => "default_ranking (AI/MCP)",
            10 => "default_token_budget (AI/MCP)",
            _ => "",
        }
    }

    fn field_value_str(&self, idx: usize) -> String {
        match idx {
            0 => match self.config.mode {
                Some(Mode::Smart) => "smart".into(),
                Some(Mode::All) => "all".into(),
                Some(Mode::Code) => "code".into(),
                Some(Mode::Docs) => "docs".into(),
                Some(Mode::Llm) => "llm".into(),
                None => "(default: smart)".into(),
            },
            1 => self
                .config
                .max_depth
                .map(|d| d.to_string())
                .unwrap_or_else(|| "(default)".into()),
            2 => self
                .config
                .max_file_size
                .map(|s| s.to_string())
                .unwrap_or_else(|| "(default)".into()),
            3 => {
                if self.config.exclude.is_empty() {
                    "(none)".into()
                } else {
                    self.config.exclude.join(", ")
                }
            }
            4 => self
                .config
                .default_format
                .clone()
                .unwrap_or_else(|| "(default)".into()),
            5 => self
                .config
                .mcp_target
                .clone()
                .unwrap_or_else(|| "(default)".into()),
            6 => match self.config.use_lsp {
                Some(true) => "true".into(),
                Some(false) => "false".into(),
                None => "(default)".into(),
            },
            7 => match self.config.stats_enabled {
                Some(true) => "true".into(),
                Some(false) => "false".into(),
                None => "(default)".into(),
            },
            8 => self
                .config
                .default_packing
                .clone()
                .unwrap_or_else(|| "(default: sandwich)".into()),
            9 => self
                .config
                .default_ranking
                .clone()
                .unwrap_or_else(|| "(default: hybrid)".into()),
            10 => self
                .config
                .default_token_budget
                .map(|b| b.to_string())
                .unwrap_or_else(|| "(default)".into()),
            _ => String::new(),
        }
    }

    fn is_bool_field(&self, idx: usize) -> bool {
        matches!(idx, 6 | 7)
    }

    fn is_enum_field(&self, idx: usize) -> bool {
        matches!(idx, 0 | 8 | 9)
    }

    fn cycle_enum(&mut self, idx: usize, forward: bool) {
        match idx {
            0 => {
                let cur = self.config.mode.unwrap_or(Mode::Smart);
                let modes = [
                    Mode::Smart,
                    Mode::All,
                    Mode::Code,
                    Mode::Docs,
                    Mode::Llm,
                ];
                let mut i = modes.iter().position(|&m| m == cur).unwrap_or(0);
                if forward {
                    i = (i + 1) % modes.len();
                } else {
                    i = if i == 0 { modes.len() - 1 } else { i - 1 };
                }
                self.config.mode = Some(modes[i]);
            }
            8 => {
                let packings = ["sandwich", "frontloaded", "balanced"];
                let cur = self
                    .config
                    .default_packing
                    .clone()
                    .unwrap_or_else(|| "sandwich".into());
                let mut i = packings.iter().position(|&p| p == cur).unwrap_or(0);
                if forward {
                    i = (i + 1) % packings.len();
                } else {
                    i = if i == 0 { packings.len() - 1 } else { i - 1 };
                }
                self.config.default_packing = Some(packings[i].to_string());
            }
            9 => {
                let rankings = ["hybrid", "graph", "lexical"];
                let cur = self
                    .config
                    .default_ranking
                    .clone()
                    .unwrap_or_else(|| "hybrid".into());
                let mut i = rankings.iter().position(|&r| r == cur).unwrap_or(0);
                if forward {
                    i = (i + 1) % rankings.len();
                } else {
                    i = if i == 0 { rankings.len() - 1 } else { i - 1 };
                }
                self.config.default_ranking = Some(rankings[i].to_string());
            }
            _ => {}
        }
    }

    fn toggle_bool(&mut self, idx: usize) {
        match idx {
            6 => {
                let v = self.config.use_lsp.unwrap_or(false);
                self.config.use_lsp = Some(!v);
            }
            7 => {
                let v = self.config.stats_enabled.unwrap_or(true);
                self.config.stats_enabled = Some(!v);
            }
            _ => {}
        }
    }

    fn start_edit(&mut self, idx: usize) {
        if self.is_enum_field(idx) {
            // cycle instead of text edit for enums
            self.cycle_enum(idx, true);
            self.message = Some("cycled (use ←→ or e to cycle)".into());
            return;
        }
        self.input_mode = true;
        self.input_target = Some(idx);
        self.input_buffer = match idx {
            1 => self.config.max_depth.map(|v| v.to_string()).unwrap_or_default(),
            2 => self
                .config
                .max_file_size
                .map(|v| v.to_string())
                .unwrap_or_default(),
            3 => String::new(), // add mode for exclude, not replace
            4 => self.config.default_format.clone().unwrap_or_default(),
            5 => self.config.mcp_target.clone().unwrap_or_default(),
            10 => self
                .config
                .default_token_budget
                .map(|v| v.to_string())
                .unwrap_or_default(),
            _ => String::new(),
        };
        if idx == EXCLUDE_IDX {
            self.message = Some("enter pattern to ADD to exclude (Esc cancel, Enter add)".into());
        } else {
            self.message = Some("edit value (Esc cancel, Enter save)".into());
        }
    }

    fn apply_input(&mut self) {
        if let Some(idx) = self.input_target {
            let buf = self.input_buffer.trim();
            match idx {
                1 => {
                    if buf.is_empty() {
                        self.config.max_depth = None;
                    } else if let Ok(v) = buf.parse::<usize>() {
                        self.config.max_depth = Some(v);
                    } else {
                        self.message = Some("invalid number".into());
                        return;
                    }
                }
                2 => {
                    if buf.is_empty() {
                        self.config.max_file_size = None;
                    } else if let Ok(v) = buf.parse::<u64>() {
                        self.config.max_file_size = Some(v);
                    } else {
                        self.message = Some("invalid number".into());
                        return;
                    }
                }
                3 => {
                    // add to exclude
                    if !buf.is_empty() && !self.config.exclude.iter().any(|e| e == buf) {
                        self.config.exclude.push(buf.to_string());
                    }
                }
                4 => {
                    if buf.is_empty() {
                        self.config.default_format = None;
                    } else {
                        self.config.default_format = Some(buf.to_string());
                    }
                }
                5 => {
                    if buf.is_empty() {
                        self.config.mcp_target = None;
                    } else {
                        self.config.mcp_target = Some(buf.to_string());
                    }
                }
                10 => {
                    if buf.is_empty() {
                        self.config.default_token_budget = None;
                    } else if let Ok(v) = buf.parse::<usize>() {
                        self.config.default_token_budget = Some(v);
                    } else {
                        self.message = Some("invalid number".into());
                        return;
                    }
                }
                _ => {}
            }
            self.message = Some("updated (press s to save to .ctxconfig)".into());
        }
        self.input_mode = false;
        self.input_target = None;
        self.input_buffer.clear();
    }

    fn cancel_input(&mut self) {
        self.input_mode = false;
        self.input_target = None;
        self.input_buffer.clear();
        self.message = None;
    }

    fn add_remove_exclude(&mut self, add: bool) {
        if add {
            // trigger input for add
            self.start_edit(EXCLUDE_IDX);
        } else {
            // remove last
            if !self.config.exclude.is_empty() {
                self.config.exclude.pop();
                self.message = Some("removed last exclude (s to save)".into());
            }
        }
    }

    fn save(&mut self) -> Result<(), String> {
        let target_path = find_config(&self.dir).unwrap_or_else(|| {
            // create next to invocation dir (small choice)
            if let Ok(c) = self.dir.canonicalize() {
                c.join(".ctxconfig")
            } else {
                self.dir.join(".ctxconfig")
            }
        });
        save_config(&target_path, &self.config).map_err(|e| e.to_string())?;
        self.message = Some(format!("saved to {}", target_path.display()));
        Ok(())
    }
}

fn draw(f: &mut ratatui::Frame, state: &SettingsState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header
            Constraint::Min(8),     // body
            Constraint::Length(3),  // footer
        ])
        .split(f.size());

    // header
    let header = Paragraph::new(Line::from(vec![
        Span::styled("✨ ctx settings", Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
        Span::styled("  —  interactive .ctxconfig editor", Style::default().fg(GRAY)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ORANGE))
            .title(Span::styled("ctx", Style::default().fg(GREEN))),
    );
    f.render_widget(header, chunks[0]);

    // body list
    let items: Vec<ListItem> = (0..N_FIELDS)
        .map(|i| {
            let label = state.field_label(i);
            let val = state.field_value_str(i);
            let is_sel = i == state.selected;
            let prefix = if is_sel { "▶ " } else { "  " };

            let mut line_spans = vec![Span::styled(
                format!("{}{}: ", prefix, label),
                if is_sel {
                    Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT)
                },
            )];

            let val_style = if is_sel {
                Style::default().fg(GREEN)
            } else {
                Style::default().fg(GRAY)
            };
            line_spans.push(Span::styled(val, val_style));

            if state.input_mode && state.input_target == Some(i) {
                line_spans.push(Span::styled(
                    format!("  [input: {}▌]", state.input_buffer),
                    Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
                ));
            }

            ListItem::new(Line::from(line_spans))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(GRAY))
                .title(Span::styled("Settings (↑↓ nav)", Style::default().fg(GRAY))),
        )
        .highlight_style(
            Style::default()
                .bg(HIGHLIGHT_BG)
                .fg(TEXT),
        );

    // we render as plain list (manual highlight via prefix) to keep very small, no ListState needed
    f.render_widget(list, chunks[1]);

    // footer help + message
    let help = "↑↓:nav  Space:toggle bool  ←→/e:cycle enum  e/Enter:edit  a/r:±exclude  s:save  q:quit";
    let mut footer_lines = vec![Span::styled(help, Style::default().fg(GRAY))];
    if let Some(ref msg) = state.message {
        footer_lines.push(Span::raw("  "));
        footer_lines.push(Span::styled(msg, Style::default().fg(GREEN)));
    }
    let footer = Paragraph::new(Line::from(footer_lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ORANGE)),
        );
    f.render_widget(footer, chunks[2]);
}

struct TermGuard;

impl Drop for TermGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            crossterm::cursor::Show
        );
    }
}

pub fn run_settings_editor(dir: PathBuf) -> Result<(), crate::error::TuiError> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let _guard = TermGuard;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = SettingsState::new(dir);

    loop {
        terminal.draw(|f| draw(f, &state))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                if state.input_mode {
                    match key.code {
                        KeyCode::Enter => {
                            state.apply_input();
                        }
                        KeyCode::Esc => {
                            state.cancel_input();
                        }
                        KeyCode::Backspace => {
                            state.input_buffer.pop();
                        }
                        KeyCode::Char(c) => {
                            // allow reasonable chars for patterns/names/numbers
                            if c.is_ascii() && !c.is_control() {
                                state.input_buffer.push(c);
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // normal mode
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        break;
                    }
                    KeyCode::Char('s') => {
                        if let Err(e) = state.save() {
                            state.message = Some(format!("save error: {}", e));
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if state.selected > 0 {
                            state.selected -= 1;
                        }
                        state.message = None;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if state.selected < N_FIELDS - 1 {
                            state.selected += 1;
                        }
                        state.message = None;
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        if state.is_enum_field(state.selected) {
                            state.cycle_enum(state.selected, false);
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        if state.is_enum_field(state.selected) {
                            state.cycle_enum(state.selected, true);
                        }
                    }
                    KeyCode::Char(' ') => {
                        if state.is_bool_field(state.selected) {
                            state.toggle_bool(state.selected);
                            state.message = Some("toggled (s to save)".into());
                        } else if state.is_enum_field(state.selected) {
                            state.cycle_enum(state.selected, true);
                        }
                    }
                    KeyCode::Char('e') | KeyCode::Enter => {
                        state.start_edit(state.selected);
                    }
                    KeyCode::Char('a') => {
                        if state.selected == EXCLUDE_IDX {
                            state.add_remove_exclude(true);
                        }
                    }
                    KeyCode::Char('r') | KeyCode::Char('d') => {
                        if state.selected == EXCLUDE_IDX {
                            state.add_remove_exclude(false);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}