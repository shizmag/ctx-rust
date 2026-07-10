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

use ctx_config::{Config, EnsureOutcome, ensure_global_config, save_global_config};
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

const N_FIELDS: usize = 21;
const EXCLUDE_IDX: usize = 3;
const ENABLE_RERANK_IDX: usize = 16;
const EMBEDDING_MODEL_IDX: usize = 17;
const RERANKER_MODEL_IDX: usize = 18;
const EMBEDDING_TOKENIZER_IDX: usize = 19;
const RERANK_TOKENIZER_IDX: usize = 20;

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
const DEFAULT_RRF_K: usize = 60;
const DEFAULT_BM25_TOP_K: usize = 50;
const DEFAULT_DENSE_TOP_K: usize = 50;
const DEFAULT_RERANK_TOP_K: usize = 20;

impl SettingsState {
    fn new(dir: PathBuf) -> Self {
        let (config_path, config, outcome) = ensure_global_config(&dir)
            .unwrap_or_else(|_| (dir.clone(), Config::default_values(), EnsureOutcome::Created));
        let message = match outcome {
            EnsureOutcome::Created => Some(format!("created {}", config_path.display())),
            EnsureOutcome::Upgraded => Some(format!(
                "upgraded {} with new default settings",
                config_path.display()
            )),
            EnsureOutcome::Unchanged => None,
        };
        Self {
            dir,
            config,
            selected: 0,
            message,
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
            11 => "default_retrieval_strategy (Search)",
            12 => "rrf_k (Search)",
            13 => "bm25_top_k (Search)",
            14 => "dense_top_k (Search)",
            15 => "rerank_top_k (Search)",
            16 => "enable_rerank (Search)",
            17 => "embedding_model (Search)",
            18 => "reranker_model (Search)",
            19 => "embedding_tokenizer (Search)",
            20 => "rerank_tokenizer (Search)",
            _ => "",
        }
    }

    /// Short explanation shown under the settings list for the selected field.
    fn field_help(&self, idx: usize) -> &'static str {
        match idx {
            0 => "Files to include when scanning: smart (auto-detect), code, docs, all, or llm-only.",
            1 => "Maximum directory depth when walking the project tree. Deeper folders are skipped.",
            2 => "Skip files larger than this size in bytes (524288 = 512 KB).",
            3 => "Comma-separated glob patterns to exclude from scans (e.g. target, node_modules).",
            4 => "Default output format for MCP tools and agents: yaml, json, markdown, xml, or plain text.",
            5 => "Preferred coding agent for `ctx mcp install`: claude, cursor, gemini, continue, vscode, or code.",
            6 => "Use language servers (rust-analyzer, pyright) to resolve call edges as LspExact. Slower but more precise.",
            7 => "Collect MCP session usage stats (calls, tokens, timings) into the codegraph index.",
            8 => "How context chunks are ordered in responses: sandwich (edges first), frontloaded, or balanced.",
            9 => "Ranking strategy for retrieved chunks: hybrid (graph+lexical), graph-only, or lexical-only.",
            10 => "Maximum tokens returned by retrieve_context / affect. Approximate; depends on chunk sizes.",
            11 => "Default retrieval mode: hybrid (BM25+embeddings), graph (symbol traversal), lexical, or dense.",
            12 => "Reciprocal Rank Fusion smoothing (RRF k). Higher values flatten combined BM25+embedding ranks. Typical: 60.",
            13 => "Number of lexical (BM25 keyword) hits to fetch before fusion with dense results.",
            14 => "Number of dense (embedding similarity) hits to fetch before fusion with lexical results.",
            15 => "How many fused candidates to pass to the reranker when enable_rerank is true.",
            16 => "Re-score top search hits with the reranker ONNX model. Requires reranker_model to be set.",
            17 => "Path to the embedding ONNX model (.onnx). Required to build and query hybrid/dense search indexes.",
            18 => "Path to the reranker ONNX model (.onnx). Optional cross-encoder that improves result ordering.",
            19 => "Directory with tokenizer.json (HuggingFace) for the embedding ONNX model. \
                    Converts text to token IDs before vector embedding. Defaults to embedding model's parent folder.",
            20 => "Directory with tokenizer.json (HuggingFace) for the reranker ONNX model. \
                    Tokenizes query+document pairs for cross-encoder scoring. Defaults to reranker model's parent folder.",
            _ => "",
        }
    }

    fn effective(&self) -> Config {
        self.config.clone().apply_defaults()
    }

    fn field_value_str(&self, idx: usize) -> String {
        let c = self.effective();
        match idx {
            0 => match c.mode {
                Some(Mode::Smart) => "smart".into(),
                Some(Mode::All) => "all".into(),
                Some(Mode::Code) => "code".into(),
                Some(Mode::Docs) => "docs".into(),
                Some(Mode::Llm) => "llm".into(),
                None => "smart".into(),
            },
            1 => c.max_depth.map(|d| d.to_string()).unwrap_or_else(|| "10".into()),
            2 => c.max_file_size
                .map(|s| s.to_string())
                .unwrap_or_else(|| "524288".into()),
            3 => {
                if c.exclude.is_empty() {
                    "(none)".into()
                } else {
                    c.exclude.join(", ")
                }
            }
            4 => c.default_format.unwrap_or_else(|| "yaml".into()),
            5 => c.mcp_target.unwrap_or_else(|| "(none)".into()),
            6 => match c.use_lsp {
                Some(true) => "true".into(),
                Some(false) => "false".into(),
                None => "true".into(),
            },
            7 => match c.stats_enabled {
                Some(true) => "true".into(),
                Some(false) => "false".into(),
                None => "true".into(),
            },
            8 => c.default_packing.unwrap_or_else(|| "sandwich".into()),
            9 => c.default_ranking.unwrap_or_else(|| "hybrid".into()),
            10 => c.default_token_budget
                .map(|b| b.to_string())
                .unwrap_or_else(|| "12000".into()),
            11 => c.default_retrieval_strategy.unwrap_or_else(|| "hybrid".into()),
            12 => c.rrf_k.map(|v| v.to_string()).unwrap_or_else(|| "60".into()),
            13 => c.bm25_top_k.map(|v| v.to_string()).unwrap_or_else(|| "50".into()),
            14 => c.dense_top_k.map(|v| v.to_string()).unwrap_or_else(|| "50".into()),
            15 => c.rerank_top_k.map(|v| v.to_string()).unwrap_or_else(|| "20".into()),
            16 => match c.enable_rerank {
                Some(true) => "true".into(),
                Some(false) => "false".into(),
                None => "false".into(),
            },
            17 => self
                .config
                .embedding_model
                .clone()
                .unwrap_or_else(|| "(not set — edit config file)".into()),
            18 => self
                .config
                .reranker_model
                .clone()
                .unwrap_or_else(|| "(not set — edit config file)".into()),
            19 => self
                .config
                .embedding_tokenizer
                .clone()
                .unwrap_or_else(|| "(default: embedding model dir)".into()),
            20 => self
                .config
                .rerank_tokenizer
                .clone()
                .unwrap_or_else(|| "(default: reranker model dir)".into()),
            _ => String::new(),
        }
    }

    fn is_bool_field(&self, idx: usize) -> bool {
        matches!(idx, 6 | 7 | 16)
    }

    fn is_numeric_field(&self, idx: usize) -> bool {
        matches!(idx, 1 | 2 | 10 | 12 | 13 | 14 | 15)
    }

    fn is_cyclable_field(&self, idx: usize) -> bool {
        matches!(idx, 0 | 4 | 5 | 8 | 9 | 11)
    }

    fn is_path_field(&self, idx: usize) -> bool {
        matches!(
            idx,
            EMBEDDING_MODEL_IDX
                | RERANKER_MODEL_IDX
                | EMBEDDING_TOKENIZER_IDX
                | RERANK_TOKENIZER_IDX
        )
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
                let fmts = ["yaml", "json", "markdown", "xml", "plain"];
                let cur = self.effective().default_format.unwrap_or_else(|| "yaml".into());
                let mut i = fmts.iter().position(|&f| f == cur).unwrap_or(0);
                if forward {
                    i = (i + 1) % fmts.len();
                } else {
                    i = if i == 0 { fmts.len() - 1 } else { i - 1 };
                }
                self.config.default_format = Some(fmts[i].to_string());
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
                let cur = self
                    .effective()
                    .default_ranking
                    .unwrap_or_else(|| "hybrid".into());
                let mut i = rankings.iter().position(|&r| r == cur).unwrap_or(0);
                if forward {
                    i = (i + 1) % rankings.len();
                } else {
                    i = if i == 0 { rankings.len() - 1 } else { i - 1 };
                }
                self.config.default_ranking = Some(rankings[i].to_string());
            }
            11 => {
                let strategies = ["hybrid", "graph", "lexical", "dense"];
                let cur = self
                    .effective()
                    .default_retrieval_strategy
                    .unwrap_or_else(|| "hybrid".into());
                let mut i = strategies.iter().position(|&s| s == cur).unwrap_or(0);
                if forward {
                    i = (i + 1) % strategies.len();
                } else {
                    i = if i == 0 { strategies.len() - 1 } else { i - 1 };
                }
                self.config.default_retrieval_strategy = Some(strategies[i].to_string());
            }
            _ => {}
        }
    }

    fn toggle_bool(&mut self, idx: usize) {
        let c = self.effective();
        match idx {
            6 => self.config.use_lsp = Some(!c.use_lsp.unwrap_or(true)),
            7 => self.config.stats_enabled = Some(!c.stats_enabled.unwrap_or(true)),
            16 => self.config.enable_rerank = Some(!c.enable_rerank.unwrap_or(false)),
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
                let step: i64 = 100;
                let cur = self
                    .effective()
                    .default_token_budget
                    .unwrap_or(DEFAULT_TOKEN_BUDGET);
                let new = (cur as i64 + delta * step).max(100) as usize;
                self.config.default_token_budget = Some(new);
                let shown = self.field_value_str(10);
                self.message = Some(format!("default_token_budget → {} (s to save)", shown));
            }
            12 => {
                let cur = self.effective().rrf_k.unwrap_or(DEFAULT_RRF_K);
                let new = (cur as i64 + delta).max(1) as usize;
                self.config.rrf_k = Some(new);
            }
            13 => {
                let step: i64 = 5;
                let cur = self.effective().bm25_top_k.unwrap_or(DEFAULT_BM25_TOP_K);
                let new = (cur as i64 + delta * step).max(1) as usize;
                self.config.bm25_top_k = Some(new);
            }
            14 => {
                let step: i64 = 5;
                let cur = self.effective().dense_top_k.unwrap_or(DEFAULT_DENSE_TOP_K);
                let new = (cur as i64 + delta * step).max(1) as usize;
                self.config.dense_top_k = Some(new);
            }
            15 => {
                let step: i64 = 5;
                let cur = self.effective().rerank_top_k.unwrap_or(DEFAULT_RERANK_TOP_K);
                let new = (cur as i64 + delta * step).max(1) as usize;
                self.config.rerank_top_k = Some(new);
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
        let d = Config::default_values();
        match idx {
            0 => self.config.mode = d.mode,
            1 => self.config.max_depth = d.max_depth,
            2 => self.config.max_file_size = d.max_file_size,
            3 => self.config.exclude.clear(),
            4 => self.config.default_format = d.default_format,
            5 => self.config.mcp_target = d.mcp_target,
            6 => self.config.use_lsp = d.use_lsp,
            7 => self.config.stats_enabled = d.stats_enabled,
            8 => self.config.default_packing = d.default_packing,
            9 => self.config.default_ranking = d.default_ranking,
            10 => self.config.default_token_budget = d.default_token_budget,
            11 => self.config.default_retrieval_strategy = d.default_retrieval_strategy,
            12 => self.config.rrf_k = d.rrf_k,
            13 => self.config.bm25_top_k = d.bm25_top_k,
            14 => self.config.dense_top_k = d.dense_top_k,
            15 => self.config.rerank_top_k = d.rerank_top_k,
            16 => self.config.enable_rerank = d.enable_rerank,
            EMBEDDING_MODEL_IDX => self.config.embedding_model = None,
            RERANKER_MODEL_IDX => self.config.reranker_model = None,
            EMBEDDING_TOKENIZER_IDX => self.config.embedding_tokenizer = None,
            RERANK_TOKENIZER_IDX => self.config.rerank_tokenizer = None,
            _ => {}
        }
        self.message = Some("reset to default (s to save)".into());
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
        let target_path = save_global_config(&self.config).map_err(|e| e.to_string())?;
        self.config = self.config.clone().apply_defaults();
        self.message = Some(format!("saved to {}", target_path.display()));
        Ok(())
    }
}

fn wrap_help(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
        } else if current.len() + 1 + word.len() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(Line::from(Span::styled(
                std::mem::take(&mut current),
                Style::default().fg(GRAY),
            )));
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(Line::from(Span::styled(current, Style::default().fg(GRAY))));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::raw("")));
    }
    lines
}

fn draw(f: &mut ratatui::Frame, state: &SettingsState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header
            Constraint::Min(6),     // body
            Constraint::Length(4),  // field description
            Constraint::Length(3),  // footer
        ])
        .split(f.size());

    // header
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "✨ ctx settings",
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  —  ~/.config/ctx/config",
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

    let help_width = chunks[2].width.saturating_sub(4) as usize;
    let mut desc_lines = vec![Line::from(Span::styled(
        state.field_label(state.selected),
        Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
    ))];
    desc_lines.extend(wrap_help(state.field_help(state.selected), help_width.max(20)));
    if state.is_path_field(state.selected) {
        desc_lines.push(Line::from(Span::styled(
            "Edit path fields in ~/.config/ctx/config",
            Style::default().fg(GREEN),
        )));
    }
    let description = Paragraph::new(desc_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(GRAY))
            .title(Span::styled("About this setting", Style::default().fg(GRAY))),
    );
    f.render_widget(description, chunks[2]);

    // footer help + message
    let help = if state.is_path_field(state.selected) {
        "path fields: edit config file  c:reset  s:save  q:quit"
    } else {
        "↑↓/jk:nav  ←→/hl:cycle or ±val  Space/a:toggle  c:reset  r:rm-ex  s:save  q:quit"
    };
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
    use ctx_config::{ensure_global_config, EnsureOutcome, CONFIG_DIR_NAME, CONFIG_FILE_NAME};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn with_xdg_config_home<F: FnOnce(&PathBuf)>(f: F) {
        let _guard = env_lock();
        let temp_dir = tempfile::tempdir().unwrap();
        let xdg = temp_dir.path().join("xdg-config");
        fs::create_dir_all(&xdg).unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &xdg) };
        f(&xdg);
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    }

    fn make_test_state() -> SettingsState {
        SettingsState {
            dir: PathBuf::from("."),
            config: Config::default_values(),
            selected: 0,
            message: None,
            exclude_preset: 0,
        }
    }

    #[test]
    fn all_fields_have_help_text() {
        let s = make_test_state();
        for idx in 0..N_FIELDS {
            assert!(
                !s.field_help(idx).is_empty(),
                "missing help for field {idx}: {}",
                s.field_label(idx)
            );
        }
    }

    #[test]
    fn embedding_tokenizer_help_mentions_format() {
        let s = make_test_state();
        let help = s.field_help(EMBEDDING_TOKENIZER_IDX);
        assert!(help.contains("tokenizer.json"));
        assert!(help.contains("embedding"));
    }

    #[test]
    fn rerank_tokenizer_help_mentions_reranker() {
        let s = make_test_state();
        let help = s.field_help(RERANK_TOKENIZER_IDX);
        assert!(help.contains("tokenizer.json"));
        assert!(help.contains("reranker"));
    }

    #[test]
    fn test_cycle_mode_and_clear() {
        let mut s = make_test_state();
        assert_eq!(s.config.mode, Some(Mode::Smart));
        s.cycle_value(0, true);
        assert_eq!(s.config.mode, Some(Mode::All));
        s.cycle_value(0, false);
        assert_eq!(s.config.mode, Some(Mode::Smart));
        s.clear_field(0);
        assert_eq!(s.config.mode, Some(Mode::Smart));
    }

    #[test]
    fn test_cycle_default_format_and_mcp_target() {
        let mut s = make_test_state();
        assert_eq!(s.config.default_format.as_deref(), Some("yaml"));
        s.cycle_value(4, true);
        assert_eq!(s.config.default_format.as_deref(), Some("json"));
        s.cycle_value(4, false);
        assert_eq!(s.config.default_format.as_deref(), Some("yaml"));
        s.cycle_value(4, true); // json
        s.cycle_value(4, true); // markdown
        s.cycle_value(4, true); // xml
        s.cycle_value(4, true); // plain
        s.cycle_value(4, true); // yaml
        assert_eq!(s.config.default_format.as_deref(), Some("yaml"));

        s.cycle_value(5, true);
        assert_eq!(s.config.mcp_target.as_deref(), Some("claude"));
        s.cycle_value(5, true);
        assert_eq!(s.config.mcp_target.as_deref(), Some("cursor"));
        s.clear_field(5);
        assert!(s.config.mcp_target.is_none()); // factory default is none
    }

    #[test]
    fn test_numeric_adjust_and_clear() {
        let mut s = make_test_state();
        assert_eq!(s.config.max_depth, Some(DEFAULT_MAX_DEPTH));
        s.adjust_numeric(1, 1);
        assert_eq!(s.config.max_depth, Some(DEFAULT_MAX_DEPTH + 1));
        s.adjust_numeric(1, -1);
        assert_eq!(s.config.max_depth, Some(DEFAULT_MAX_DEPTH));
        s.clear_field(1);
        assert_eq!(s.config.max_depth, Some(DEFAULT_MAX_DEPTH));

        assert_eq!(s.config.max_file_size, Some(DEFAULT_MAX_FILE_SIZE));
        s.adjust_numeric(2, 1);
        assert_eq!(s.config.max_file_size, Some(DEFAULT_MAX_FILE_SIZE + 1024));
        s.clear_field(2);
        assert_eq!(s.config.max_file_size, Some(DEFAULT_MAX_FILE_SIZE));

        assert_eq!(s.config.default_token_budget, Some(DEFAULT_TOKEN_BUDGET));
        s.adjust_numeric(10, 1);
        assert_eq!(s.config.default_token_budget, Some(DEFAULT_TOKEN_BUDGET + 100));
        s.clear_field(10);
        assert_eq!(s.config.default_token_budget, Some(DEFAULT_TOKEN_BUDGET));
    }

    #[test]
    fn test_bool_toggle_and_clear() {
        let mut s = make_test_state();
        s.toggle_bool(6);
        assert_eq!(s.config.use_lsp, Some(false));
        s.toggle_bool(6);
        assert_eq!(s.config.use_lsp, Some(true));
        s.clear_field(6);
        assert_eq!(s.config.use_lsp, Some(true));

        s.toggle_bool(7);
        assert_eq!(s.config.stats_enabled, Some(false)); // default in toggle is true, ! -> false
        s.clear_field(7);
        assert_eq!(s.config.stats_enabled, Some(true));
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

    #[test]
    fn settings_state_creates_global_config_on_first_open() {
        with_xdg_config_home(|xdg| {
            let empty_project = tempfile::tempdir().unwrap();
            let state = SettingsState::new(empty_project.path().to_path_buf());
            let path = xdg.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME);
            assert!(path.exists());
            assert_eq!(state.config, ctx_config::Config::default_values());
            assert!(
                state
                    .message
                    .as_deref()
                    .is_some_and(|m| m.contains("created"))
            );
        });
    }

    #[test]
    fn settings_state_upgrades_existing_global_config() {
        with_xdg_config_home(|xdg| {
            let empty_project = tempfile::tempdir().unwrap();
            let path = xdg.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "mode = docs\n").unwrap();

            let state = SettingsState::new(empty_project.path().to_path_buf());
            assert_eq!(state.config.mode, Some(Mode::Docs));
            assert_eq!(state.config.default_format, Some("yaml".into()));
            assert!(
                state
                    .message
                    .as_deref()
                    .is_some_and(|m| m.contains("upgraded"))
            );

            let content = fs::read_to_string(&path).unwrap();
            assert!(content.contains("default_format = yaml"));
            assert!(content.contains("# embedding_tokenizer ="));
            assert!(content.contains("# embedding_model ="));
        });
    }

    #[test]
    fn settings_state_upgrades_legacy_config_missing_search_settings() {
        const LEGACY: &str = "mode = smart\nmax_depth = 5\nmax_file_size = 532480\nexclude =\n\
            default_format = yaml\nuse_lsp = true\nstats_enabled = true\n\
            default_packing = sandwich\ndefault_ranking = hybrid\n\
            default_token_budget = 12000\n";

        with_xdg_config_home(|xdg| {
            let empty_project = tempfile::tempdir().unwrap();
            let path = xdg.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, LEGACY).unwrap();

            let state = SettingsState::new(empty_project.path().to_path_buf());
            assert_eq!(state.config.rrf_k, Some(60));
            assert_eq!(state.config.enable_rerank, Some(false));
            assert!(
                state
                    .message
                    .as_deref()
                    .is_some_and(|m| m.contains("upgraded"))
            );

            let content = fs::read_to_string(&path).unwrap();
            assert!(content.contains("enable_rerank = false"));
            assert!(content.contains("default_retrieval_strategy = hybrid"));
            assert!(content.contains("# embedding_model ="));
        });
    }

    #[test]
    fn settings_save_writes_full_global_config() {
        with_xdg_config_home(|xdg| {
            let empty_project = tempfile::tempdir().unwrap();
            let mut state = SettingsState::new(empty_project.path().to_path_buf());
            state.config.mode = Some(Mode::Code);
            state.save().unwrap();

            let path = xdg.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME);
            let content = fs::read_to_string(&path).unwrap();
            assert!(content.contains("mode = code"));
            assert!(content.contains("default_retrieval_strategy = hybrid"));
            assert!(content.contains("# embedding_tokenizer ="));
            assert!(content.contains("# embedding_model ="));

            let (_, _, outcome) = ensure_global_config(empty_project.path()).unwrap();
            assert_eq!(outcome, EnsureOutcome::Unchanged);
        });
    }
}
