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

use ctx_config::{Config, find_and_load_config, find_config, save_config};
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
    message: Option<String>,
    // cycling index for common exclude patterns when on exclude field (arrows choose preset to toggle)
    exclude_preset: usize,
}

const N_FIELDS: usize = 11;
const EXCLUDE_IDX: usize = 3;

// Common exclude patterns for arrow cycling (no free-text input)
const COMMON_EXCLUDES: &[&str] = &[
    "target",
    "node_modules",
    ".git",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    ".ctx-codegraph",
    "*.log",
];

const DEFAULT_MAX_DEPTH: usize = 10;
const DEFAULT_MAX_FILE_SIZE: u64 = 512 * 1024;
const DEFAULT_TOKEN_BUDGET: usize = 12000;

impl SettingsState {
    fn new(dir: PathBuf) -> Self {
        let config = find_and_load_config(&dir).unwrap_or_default();
        Self {
            dir,
            config,
            selected: 0,
            message: None,
            exclude_preset: 0,
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

    fn is_numeric_field(&self, idx: usize) -> bool {
        matches!(idx, 1 | 2 | 10)
    }

    fn is_cyclable_field(&self, idx: usize) -> bool {
        matches!(idx, 0 | 4 | 5 | 8 | 9)
    }

    fn cycle_value(&mut self, idx: usize, forward: bool) {
        match idx {
            0 => {
                let modes = [Mode::Smart, Mode::All, Mode::Code, Mode::Docs, Mode::Llm];
                if self.config.mode.is_none() {
                    // first cycle from implicit default: select the default choice explicitly
                    self.config.mode = if forward {
                        Some(Mode::Smart)
                    } else {
                        Some(Mode::Llm)
                    };
                    return;
                }
                let cur = self.config.mode.unwrap();
                let mut i = modes.iter().position(|&m| m == cur).unwrap_or(0);
                if forward {
                    i = (i + 1) % modes.len();
                } else {
                    i = if i == 0 { modes.len() - 1 } else { i - 1 };
                }
                self.config.mode = Some(modes[i]);
            }
            4 => {
                // default_format cycles including (default) to allow clearing without input
                let fmts = ["(default)", "yaml", "json", "markdown", "xml", "plain"];
                let cur = self
                    .config
                    .default_format
                    .clone()
                    .unwrap_or_else(|| "(default)".into());
                let mut i = fmts.iter().position(|&f| f == cur).unwrap_or(0);
                if forward {
                    i = (i + 1) % fmts.len();
                } else {
                    i = if i == 0 { fmts.len() - 1 } else { i - 1 };
                }
                let choice = fmts[i];
                self.config.default_format = if choice == "(default)" {
                    None
                } else {
                    Some(choice.to_string())
                };
            }
            5 => {
                // mcp_target: use known targets (from install etc), (default) clears
                let targets = [
                    "(default)",
                    "claude",
                    "cursor",
                    "gemini",
                    "continue",
                    "vscode",
                    "code",
                ];
                let cur = self
                    .config
                    .mcp_target
                    .clone()
                    .unwrap_or_else(|| "(default)".into());
                let mut i = targets.iter().position(|&t| t == cur).unwrap_or(0);
                if forward {
                    i = (i + 1) % targets.len();
                } else {
                    i = if i == 0 { targets.len() - 1 } else { i - 1 };
                }
                let choice = targets[i];
                self.config.mcp_target = if choice == "(default)" {
                    None
                } else {
                    Some(choice.to_string())
                };
            }
            8 => {
                let packings = ["sandwich", "frontloaded", "balanced"];
                if self.config.default_packing.is_none() {
                    self.config.default_packing = if forward {
                        Some("sandwich".into())
                    } else {
                        Some("balanced".into())
                    };
                    return;
                }
                let cur = self.config.default_packing.clone().unwrap();
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
                if self.config.default_ranking.is_none() {
                    self.config.default_ranking = if forward {
                        Some("hybrid".into())
                    } else {
                        Some("lexical".into())
                    };
                    return;
                }
                let cur = self.config.default_ranking.clone().unwrap();
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

    fn adjust_numeric(&mut self, idx: usize, delta: i64) {
        match idx {
            1 => {
                // max_depth step by 1; 0 means default (None)
                if self.config.max_depth.is_none() {
                    if delta > 0 {
                        self.config.max_depth = Some(DEFAULT_MAX_DEPTH);
                        let shown = self.field_value_str(1);
                        self.message = Some(format!("max_depth → {} (s to save)", shown));
                        return;
                    } else {
                        self.message = Some("(default) — use → to set value".into());
                        return;
                    }
                }
                let cur = self.config.max_depth.unwrap();
                let new = (cur as i64 + delta).max(0) as usize;
                self.config.max_depth = if new == 0 { None } else { Some(new) };
                let shown = self.field_value_str(1);
                self.message = Some(format!("max_depth → {} (s to save)", shown));
            }
            2 => {
                // max_file_size step by 1KB; 0 means default
                let step: i64 = 1024;
                if self.config.max_file_size.is_none() {
                    if delta > 0 {
                        self.config.max_file_size = Some(DEFAULT_MAX_FILE_SIZE);
                        let shown = self.field_value_str(2);
                        self.message = Some(format!("max_file_size → {} (s to save)", shown));
                        return;
                    } else {
                        self.message = Some("(default) — use → to set value".into());
                        return;
                    }
                }
                let cur = self.config.max_file_size.unwrap();
                let new = (cur as i64 + delta * step).max(0) as u64;
                self.config.max_file_size = if new == 0 { None } else { Some(new) };
                let shown = self.field_value_str(2);
                self.message = Some(format!("max_file_size → {} (s to save)", shown));
            }
            10 => {
                // default_token_budget step 100; 0 means default
                let step: i64 = 100;
                if self.config.default_token_budget.is_none() {
                    if delta > 0 {
                        self.config.default_token_budget = Some(DEFAULT_TOKEN_BUDGET);
                        let shown = self.field_value_str(10);
                        self.message = Some(format!("default_token_budget → {} (s to save)", shown));
                        return;
                    } else {
                        self.message = Some("(default) — use → to set value".into());
                        return;
                    }
                }
                let cur = self.config.default_token_budget.unwrap();
                let new = (cur as i64 + delta * step).max(0) as usize;
                self.config.default_token_budget = if new == 0 { None } else { Some(new) };
                let shown = self.field_value_str(10);
                self.message = Some(format!("default_token_budget → {} (s to save)", shown));
            }
            _ => {}
        }
    }

    fn cycle_exclude_preset(&mut self, forward: bool) {
        let n = COMMON_EXCLUDES.len();
        if forward {
            self.exclude_preset = (self.exclude_preset + 1) % n;
        } else {
            self.exclude_preset = if self.exclude_preset == 0 {
                n - 1
            } else {
                self.exclude_preset - 1
            };
        }
        let p = COMMON_EXCLUDES[self.exclude_preset];
        self.message = Some(format!("exclude preset: {} (Space/a toggle in list)", p));
    }

    fn toggle_exclude_preset(&mut self) {
        let p = COMMON_EXCLUDES[self.exclude_preset % COMMON_EXCLUDES.len()];
        if self.config.exclude.iter().any(|e| e == p) {
            self.config.exclude.retain(|e| e != p);
            self.message = Some(format!("removed '{}' from exclude (s to save)", p));
        } else {
            self.config.exclude.push(p.to_string());
            self.message = Some(format!("added '{}' to exclude (s to save)", p));
        }
    }

    fn remove_last_exclude(&mut self) {
        if !self.config.exclude.is_empty() {
            let removed = self.config.exclude.pop().unwrap();
            self.message = Some(format!("removed last '{}' (s to save)", removed));
        }
    }

    fn clear_field(&mut self, idx: usize) {
        match idx {
            0 => self.config.mode = None,
            1 => self.config.max_depth = None,
            2 => self.config.max_file_size = None,
            3 => self.config.exclude.clear(),
            4 => self.config.default_format = None,
            5 => self.config.mcp_target = None,
            6 => self.config.use_lsp = None,
            7 => self.config.stats_enabled = None,
            8 => self.config.default_packing = None,
            9 => self.config.default_ranking = None,
            10 => self.config.default_token_budget = None,
            _ => {}
        }
        self.message = Some("cleared to default (s to save)".into());
    }

    fn activate_forward(&mut self, idx: usize) {
        // repurposed from old "start edit": for non-input model, forward adjust/cycle/toggle
        if self.is_numeric_field(idx) {
            self.adjust_numeric(idx, 1);
        } else if self.is_bool_field(idx) {
            self.toggle_bool(idx);
            self.message = Some("toggled (s to save)".into());
        } else if self.is_cyclable_field(idx) {
            self.cycle_value(idx, true);
            self.message = Some("cycled (s to save)".into());
        } else if idx == EXCLUDE_IDX {
            self.toggle_exclude_preset();
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
            Constraint::Length(3), // header
            Constraint::Min(8),    // body
            Constraint::Length(3), // footer
        ])
        .split(f.size());

    // header
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "✨ ctx settings",
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  —  interactive .ctxconfig editor",
            Style::default().fg(GRAY),
        ),
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

            ListItem::new(Line::from(line_spans))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(GRAY))
                .title(Span::styled("Settings (↑↓ nav, ←→ adjust)", Style::default().fg(GRAY))),
        )
        .highlight_style(Style::default().bg(HIGHLIGHT_BG).fg(TEXT));

    // we render as plain list (manual highlight via prefix) to keep very small, no ListState needed
    f.render_widget(list, chunks[1]);

    // footer help + message
    // no more text input: ←→ (or h/l) cycle enums or ± numeric or choose exclude preset
    let help = "↑↓/jk:nav  ←→/hl:cycle or ±val  Space/a:toggle  c:clear  r:rm-ex  s:save  q:quit";
    let mut footer_lines = vec![Span::styled(help, Style::default().fg(GRAY))];
    if let Some(ref msg) = state.message {
        footer_lines.push(Span::raw("  "));
        footer_lines.push(Span::styled(msg, Style::default().fg(GREEN)));
    }
    let footer = Paragraph::new(Line::from(footer_lines)).block(
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

                // normal mode only: all fields use selection/cycling with arrows (no input_mode)
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
                        let idx = state.selected;
                        if state.is_numeric_field(idx) {
                            state.adjust_numeric(idx, -1);
                        } else if state.is_bool_field(idx) {
                            state.toggle_bool(idx);
                            state.message = Some("toggled (s to save)".into());
                        } else if state.is_cyclable_field(idx) {
                            state.cycle_value(idx, false);
                            state.message = Some("cycled (s to save)".into());
                        } else if idx == EXCLUDE_IDX {
                            state.cycle_exclude_preset(false);
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        let idx = state.selected;
                        if state.is_numeric_field(idx) {
                            state.adjust_numeric(idx, 1);
                        } else if state.is_bool_field(idx) {
                            state.toggle_bool(idx);
                            state.message = Some("toggled (s to save)".into());
                        } else if state.is_cyclable_field(idx) {
                            state.cycle_value(idx, true);
                            state.message = Some("cycled (s to save)".into());
                        } else if idx == EXCLUDE_IDX {
                            state.cycle_exclude_preset(true);
                        }
                    }
                    KeyCode::Char(' ') => {
                        let idx = state.selected;
                        if state.is_bool_field(idx) {
                            state.toggle_bool(idx);
                            state.message = Some("toggled (s to save)".into());
                        } else if state.is_cyclable_field(idx) {
                            state.cycle_value(idx, true);
                            state.message = Some("cycled (s to save)".into());
                        } else if idx == EXCLUDE_IDX {
                            state.toggle_exclude_preset();
                        }
                    }
                    KeyCode::Char('e') | KeyCode::Enter => {
                        // repurposed: forward action (cycle/adjust/toggle) instead of text edit
                        state.activate_forward(state.selected);
                    }
                    KeyCode::Char('a') => {
                        if state.selected == EXCLUDE_IDX {
                            state.toggle_exclude_preset();
                        }
                    }
                    KeyCode::Char('r') | KeyCode::Char('d') => {
                        if state.selected == EXCLUDE_IDX {
                            state.remove_last_exclude();
                        }
                    }
                    KeyCode::Char('c') => {
                        state.clear_field(state.selected);
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_test_state() -> SettingsState {
        // direct construct to avoid fs side effects in tests
        SettingsState {
            dir: PathBuf::from("."),
            config: Config::default(),
            selected: 0,
            message: None,
            exclude_preset: 0,
        }
    }

    #[test]
    fn test_cycle_mode_and_clear() {
        let mut s = make_test_state();
        assert!(s.config.mode.is_none());
        s.cycle_value(0, true);
        assert_eq!(s.config.mode, Some(Mode::Smart)); // first forward selects the default explicitly
        s.cycle_value(0, true);
        assert_eq!(s.config.mode, Some(Mode::All));
        s.cycle_value(0, false);
        assert_eq!(s.config.mode, Some(Mode::Smart));
        s.clear_field(0);
        assert!(s.config.mode.is_none());
    }

    #[test]
    fn test_cycle_default_format_and_mcp_target() {
        let mut s = make_test_state();
        s.cycle_value(4, true);
        assert_eq!(s.config.default_format.as_deref(), Some("yaml"));
        s.cycle_value(4, true);
        assert_eq!(s.config.default_format.as_deref(), Some("json"));
        s.cycle_value(4, false);
        assert_eq!(s.config.default_format.as_deref(), Some("yaml"));
        s.cycle_value(4, true); // json
        s.cycle_value(4, true); // markdown
        s.cycle_value(4, true); // xml
        s.cycle_value(4, true); // plain
        s.cycle_value(4, true); // (default)
        assert!(s.config.default_format.is_none());

        s.cycle_value(5, true);
        assert_eq!(s.config.mcp_target.as_deref(), Some("claude"));
        s.cycle_value(5, true);
        assert_eq!(s.config.mcp_target.as_deref(), Some("cursor"));
        s.clear_field(5);
        assert!(s.config.mcp_target.is_none());
    }

    #[test]
    fn test_numeric_adjust_and_clear() {
        let mut s = make_test_state();
        assert!(s.config.max_depth.is_none());
        s.adjust_numeric(1, 1);
        assert_eq!(s.config.max_depth, Some(DEFAULT_MAX_DEPTH)); // first + sets the default value
        s.adjust_numeric(1, 1);
        assert_eq!(s.config.max_depth, Some(DEFAULT_MAX_DEPTH + 1));
        s.adjust_numeric(1, -1);
        assert_eq!(s.config.max_depth, Some(DEFAULT_MAX_DEPTH));
        s.adjust_numeric(1, -100); // goes below 0 -> None
        assert!(s.config.max_depth.is_none());

        // left on default numeric leaves it (message set)
        s.adjust_numeric(1, -1);
        assert!(s.config.max_depth.is_none());

        s.adjust_numeric(2, 1);
        assert_eq!(s.config.max_file_size, Some(DEFAULT_MAX_FILE_SIZE)); // first + sets default bytes value
        s.adjust_numeric(2, 1);
        assert_eq!(s.config.max_file_size, Some(DEFAULT_MAX_FILE_SIZE + 1024));
        s.clear_field(2);
        assert!(s.config.max_file_size.is_none());

        s.adjust_numeric(10, 1);
        assert_eq!(s.config.default_token_budget, Some(DEFAULT_TOKEN_BUDGET)); // first +
        s.adjust_numeric(10, 1);
        assert_eq!(s.config.default_token_budget, Some(DEFAULT_TOKEN_BUDGET + 100));
        s.adjust_numeric(10, -1000);
        assert!(s.config.default_token_budget.is_none());
    }

    #[test]
    fn test_bool_toggle_and_clear() {
        let mut s = make_test_state();
        s.toggle_bool(6);
        assert_eq!(s.config.use_lsp, Some(true));
        s.toggle_bool(6);
        assert_eq!(s.config.use_lsp, Some(false));
        s.clear_field(6);
        assert!(s.config.use_lsp.is_none());

        s.toggle_bool(7);
        assert_eq!(s.config.stats_enabled, Some(false)); // default in toggle is true, ! -> false
        s.clear_field(7);
        assert!(s.config.stats_enabled.is_none());
    }

    #[test]
    fn test_exclude_preset_cycle_and_toggle_no_input() {
        let mut s = make_test_state();
        assert!(s.config.exclude.is_empty());
        // cycle preset
        s.cycle_exclude_preset(true);
        let first = COMMON_EXCLUDES[1 % COMMON_EXCLUDES.len()]; // after one cycle from 0
        // actually after true from 0 ->1
        assert_eq!(s.exclude_preset, 1);
        s.toggle_exclude_preset();
        assert!(s.config.exclude.contains(&first.to_string()));
        s.toggle_exclude_preset();
        assert!(!s.config.exclude.contains(&first.to_string()));
        // add via preset, then remove last
        s.toggle_exclude_preset();
        assert!(!s.config.exclude.is_empty());
        s.remove_last_exclude();
        assert!(s.config.exclude.is_empty());
        s.clear_field(3);
        assert!(s.config.exclude.is_empty());
    }

    #[test]
    fn test_activate_forward_dispatches() {
        let mut s = make_test_state();
        s.selected = 0;
        s.activate_forward(0);
        assert!(s.config.mode.is_some());
        s.selected = 6;
        s.activate_forward(6);
        assert!(s.config.use_lsp.is_some());
        s.selected = 1;
        s.activate_forward(1);
        assert!(s.config.max_depth.is_some());
    }
}
