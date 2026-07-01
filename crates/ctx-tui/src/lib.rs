use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::collections::HashSet;
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
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use ctx_models::{NodeKind, TreeNode};

struct TuiApp {
    path: PathBuf,
    scan_result: ctx_models::ScanResult,
    expanded_dirs: HashSet<PathBuf>,
    checked_paths: HashSet<PathBuf>,
    visible_items: Vec<VisibleTuiNode>,
    list_state: ListState,
    message: Option<(String, std::time::Instant)>,
}

struct VisibleTuiNode {
    path: PathBuf,
    name: String,
    kind: NodeKind,
    is_expanded: bool,
    checked: bool,
    lines: usize,
    tokens: usize,
    bytes: u64,
    tree_line_prefix: String,
    is_text: bool,
}

impl TuiApp {
    fn new(path: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let scan_result = ctx_core::scan(&path, ctx_models::ScanOptions::default())?;
        
        let mut expanded_dirs = HashSet::new();
        let mut checked_paths = HashSet::new();
        
        initialize_tree_states(&scan_result.root, &mut expanded_dirs, &mut checked_paths);

        let mut app = Self {
            path,
            scan_result,
            expanded_dirs,
            checked_paths,
            visible_items: Vec::new(),
            list_state: ListState::default(),
            message: None,
        };
        
        app.update_visible_items();
        
        if !app.visible_items.is_empty() {
            app.list_state.select(Some(0));
        }

        Ok(app)
    }

    fn update_visible_items(&mut self) {
        let mut visible = Vec::new();
        traverse_build_visible(
            &self.scan_result.root,
            0,
            true,
            "",
            &self.expanded_dirs,
            &self.checked_paths,
            &mut visible,
        );
        self.visible_items = visible;
    }

    fn rescan(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let scan_result = ctx_core::scan(&self.path, ctx_models::ScanOptions::default())?;
        self.scan_result = scan_result;
        
        let mut new_expanded = HashSet::new();
        let mut new_checked = HashSet::new();
        
        merge_tree_states(
            &self.scan_result.root,
            &self.expanded_dirs,
            &self.checked_paths,
            &mut new_expanded,
            &mut new_checked,
        );
        
        self.expanded_dirs = new_expanded;
        self.checked_paths = new_checked;
        
        self.update_visible_items();
        
        let selected = self.list_state.selected().unwrap_or(0);
        if self.visible_items.is_empty() {
            self.list_state.select(None);
        } else if selected >= self.visible_items.len() {
            self.list_state.select(Some(self.visible_items.len() - 1));
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

fn initialize_tree_states(
    node: &TreeNode,
    expanded_dirs: &mut HashSet<PathBuf>,
    checked_paths: &mut HashSet<PathBuf>,
) {
    checked_paths.insert(node.path.clone());
    if node.kind == NodeKind::Directory {
        expanded_dirs.insert(node.path.clone());
        for child in &node.children {
            initialize_tree_states(child, expanded_dirs, checked_paths);
        }
    }
}

fn traverse_build_visible(
    node: &TreeNode,
    depth: usize,
    is_last_child: bool,
    parent_prefixes: &str,
    expanded_dirs: &HashSet<PathBuf>,
    checked_paths: &HashSet<PathBuf>,
    visible: &mut Vec<VisibleTuiNode>,
) {
    let is_dir = node.kind == NodeKind::Directory;
    
    let prefix = if depth == 0 {
        "".to_string()
    } else {
        parent_prefixes.to_string()
    };

    let checked = checked_paths.contains(&node.path);
    let is_expanded = expanded_dirs.contains(&node.path);

    visible.push(VisibleTuiNode {
        path: node.path.clone(),
        name: node.name.clone(),
        kind: node.kind,
        is_expanded,
        checked,
        lines: node.stats.lines,
        tokens: node.stats.tokens,
        bytes: node.stats.bytes,
        tree_line_prefix: prefix.clone(),
        is_text: node.stats.lines > 0 || node.stats.bytes == 0,
    });

    if is_dir && is_expanded {
        let child_count = node.children.len();
        let next_parent_prefix = if depth == 0 {
            "".to_string()
        } else {
            format!("{}{}", parent_prefixes, if is_last_child { "    " } else { "│   " })
        };
        for (i, child) in node.children.iter().enumerate() {
            let is_last = i == child_count - 1;
            traverse_build_visible(
                child,
                depth + 1,
                is_last,
                &next_parent_prefix,
                expanded_dirs,
                checked_paths,
                visible,
            );
        }
    }
}

fn set_checked_recursive(node: &TreeNode, checked: bool, checked_paths: &mut HashSet<PathBuf>) {
    if checked {
        checked_paths.insert(node.path.clone());
    } else {
        checked_paths.remove(&node.path);
    }
    for child in &node.children {
        set_checked_recursive(child, checked, checked_paths);
    }
}

fn find_node<'a>(node: &'a TreeNode, path: &Path) -> Option<&'a TreeNode> {
    if node.path == path {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_node(child, path) {
            return Some(found);
        }
    }
    None
}

fn merge_tree_states(
    node: &TreeNode,
    old_expanded: &HashSet<PathBuf>,
    old_checked: &HashSet<PathBuf>,
    new_expanded: &mut HashSet<PathBuf>,
    new_checked: &mut HashSet<PathBuf>,
) {
    if node.kind == NodeKind::Directory {
        if old_expanded.contains(&node.path) || old_expanded.is_empty() {
            new_expanded.insert(node.path.clone());
        }
    }
    
    if old_checked.is_empty() || old_checked.contains(&node.path) {
        new_checked.insert(node.path.clone());
    }

    for child in &node.children {
        merge_tree_states(child, old_expanded, old_checked, new_expanded, new_checked);
    }
}

fn collect_checked_files<'a>(
    node: &'a TreeNode,
    checked_paths: &HashSet<PathBuf>,
    files: &mut Vec<&'a TreeNode>,
) {
    if node.kind == NodeKind::File && checked_paths.contains(&node.path) {
        files.push(node);
    }
    for child in &node.children {
        collect_checked_files(child, checked_paths, files);
    }
}

fn count_all_files(node: &TreeNode) -> usize {
    if node.kind == NodeKind::File {
        1
    } else {
        node.children.iter().map(count_all_files).sum()
    }
}

fn sum_all_tokens(node: &TreeNode) -> usize {
    if node.kind == NodeKind::File {
        node.stats.tokens
    } else {
        node.children.iter().map(sum_all_tokens).sum()
    }
}

fn sum_all_bytes(node: &TreeNode) -> u64 {
    if node.kind == NodeKind::File {
        node.stats.bytes
    } else {
        node.children.iter().map(sum_all_bytes).sum()
    }
}

fn highlight_line(line: &str, ext: &str) -> Line<'static> {
    let ext = ext.to_lowercase();
    let trimmed = line.trim_start();
    if (ext == "rs" || ext == "go" || ext == "js" || ext == "ts" || ext == "tsx" || ext == "jsx" || ext == "c" || ext == "cpp") && trimmed.starts_with("//") {
        return Line::from(Span::styled(line.to_string(), Style::default().fg(Color::Rgb(86, 95, 137))));
    }
    if (ext == "py" || ext == "sh" || ext == "bash" || ext == "yaml" || ext == "yml" || ext == "toml") && trimmed.starts_with('#') {
        return Line::from(Span::styled(line.to_string(), Style::default().fg(Color::Rgb(86, 95, 137))));
    }

    let keyword_color = Color::Rgb(187, 154, 247);
    let type_color = Color::Rgb(125, 207, 255);
    let string_color = Color::Rgb(158, 206, 106);
    let comment_color = Color::Rgb(86, 95, 137);
    let text_color = Color::Rgb(192, 202, 245);
    let number_color = Color::Rgb(224, 175, 104);

    let mut spans = Vec::new();
    let mut chars = line.chars().peekable();
    let mut word = String::new();

    while let Some(&c) = chars.peek() {
        if c == '/' {
            chars.next();
            if let Some(&c2) = chars.peek() {
                if c2 == '/' && (ext == "rs" || ext == "go" || ext == "js" || ext == "ts" || ext == "tsx" || ext == "jsx" || ext == "c" || ext == "cpp") {
                    let mut comment = "/".to_string();
                    while let Some(ch) = chars.next() {
                        comment.push(ch);
                    }
                    spans.push(Span::styled(comment, Style::default().fg(comment_color)));
                    break;
                } else {
                    spans.push(Span::styled("/", Style::default().fg(text_color)));
                }
            } else {
                spans.push(Span::styled("/", Style::default().fg(text_color)));
            }
        } else if c == '#' && (ext == "py" || ext == "sh" || ext == "bash" || ext == "yaml" || ext == "yml" || ext == "toml") {
            let mut comment = String::new();
            while let Some(ch) = chars.next() {
                comment.push(ch);
            }
            spans.push(Span::styled(comment, Style::default().fg(comment_color)));
            break;
        } else if c == '"' || c == '\'' {
            let quote = c;
            chars.next();
            let mut s = quote.to_string();
            let mut escaped = false;
            while let Some(ch) = chars.next() {
                s.push(ch);
                if ch == '\\' && !escaped {
                    escaped = true;
                } else {
                    if ch == quote && !escaped {
                        break;
                    }
                    escaped = false;
                }
            }
            spans.push(Span::styled(s, Style::default().fg(string_color)));
        } else if c.is_alphabetic() || c == '_' {
            word.clear();
            while let Some(&ch) = chars.peek() {
                if ch.is_alphanumeric() || ch == '_' {
                    word.push(ch);
                    chars.next();
                } else {
                    break;
                }
            }
            let style = if is_keyword(&word) {
                Style::default().fg(keyword_color).add_modifier(Modifier::BOLD)
            } else if is_type(&word) {
                Style::default().fg(type_color)
            } else {
                Style::default().fg(text_color)
            };
            spans.push(Span::styled(word.clone(), style));
        } else if c.is_numeric() {
            let mut num = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_numeric() || ch == '.' || ch == 'x' || ch == 'f' {
                    num.push(ch);
                    chars.next();
                } else {
                    break;
                }
            }
            spans.push(Span::styled(num, Style::default().fg(number_color)));
        } else {
            let mut punct = String::new();
            punct.push(c);
            chars.next();
            spans.push(Span::styled(punct, Style::default().fg(text_color)));
        }
    }

    Line::from(spans)
}

fn is_keyword(w: &str) -> bool {
    matches!(
        w,
        "fn" | "def" | "let" | "mut" | "pub" | "use" | "import" | "from" | "struct" | "enum" | "impl" | "if" | "else" | "match" | "for" | "in" | "while" | "return" | "class" | "const" | "var" | "function" | "package" | "type" | "as" | "break" | "continue" | "crate" | "extern" | "false" | "true" | "loop" | "mod" | "static" | "trait" | "where" | "async" | "await" | "dyn"
    )
}

fn is_type(w: &str) -> bool {
    matches!(
        w,
        "i32" | "u32" | "i64" | "u64" | "usize" | "f64" | "String" | "str" | "Option" | "Result" | "bool" | "Self" | "self" | "Vec" | "Box" | "HashMap" | "HashSet" | "Path" | "PathBuf" | "std" | "io" | "fs"
    )
}

fn copy_selection_to_clipboard(app: &TuiApp) -> Result<String, Box<dyn std::error::Error>> {
    let mut out = String::new();
    let root_name = app.path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    let mut checked_files = Vec::new();
    collect_checked_files(&app.scan_result.root, &app.checked_paths, &mut checked_files);
    let total_tokens: usize = checked_files.iter().map(|f| f.stats.tokens).sum();

    out.push_str(&format!("Project Context: {}\n", root_name));
    out.push_str(&format!("Selected files: {} | Total tokens: {}\n\n", checked_files.len(), total_tokens));

    out.push_str("=== DIRECTORY STRUCTURE (SELECTED FILES) ===\n");
    for f in &checked_files {
        let rel_path = match f.path.strip_prefix(&app.path) {
            Ok(rel) => rel.to_string_lossy().to_string(),
            Err(_) => f.path.to_string_lossy().to_string(),
        };
        out.push_str(&format!("├── {} ({} tokens)\n", rel_path, f.stats.tokens));
    }
    out.push_str("\n=== FILE CONTENTS ===\n\n");

    for f in &checked_files {
        let rel_path = match f.path.strip_prefix(&app.path) {
            Ok(rel) => rel.to_string_lossy().to_string(),
            Err(_) => f.path.to_string_lossy().to_string(),
        };
        out.push_str(&format!("--- FILE: {} ({} tokens) ---\n", rel_path, f.stats.tokens));
        if f.stats.lines == 0 && f.stats.bytes > 0 {
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
                        KeyCode::Char('c') => {
                            match copy_selection_to_clipboard(app) {
                                Ok(success_msg) => {
                                    app.message = Some((success_msg, std::time::Instant::now()));
                                }
                                Err(e) => {
                                    app.message = Some((format!("Error: {}", e), std::time::Instant::now()));
                                }
                            }
                        }
                        KeyCode::Enter | KeyCode::Char('o') => {
                            if let Some(selected) = app.list_state.selected() {
                                if let Some(item) = app.visible_items.get(selected) {
                                    if item.kind == NodeKind::Directory {
                                        let path = item.path.clone();
                                        if app.expanded_dirs.contains(&path) {
                                            app.expanded_dirs.remove(&path);
                                        } else {
                                            app.expanded_dirs.insert(path);
                                        }
                                        app.update_visible_items();
                                    }
                                }
                            }
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            if let Some(selected) = app.list_state.selected() {
                                if let Some(item) = app.visible_items.get(selected) {
                                    if item.kind == NodeKind::Directory {
                                        let path = item.path.clone();
                                        if app.expanded_dirs.contains(&path) {
                                            app.expanded_dirs.remove(&path);
                                            app.update_visible_items();
                                        }
                                    }
                                }
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            if let Some(selected) = app.list_state.selected() {
                                if let Some(item) = app.visible_items.get(selected) {
                                    if item.kind == NodeKind::Directory {
                                        let path = item.path.clone();
                                        if !app.expanded_dirs.contains(&path) {
                                            app.expanded_dirs.insert(path);
                                            app.update_visible_items();
                                        }
                                    }
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
                                if let Some(item) = app.visible_items.get(selected) {
                                    let path = item.path.clone();
                                    let new_checked = !item.checked;
                                    
                                    if let Some(node) = find_node(&app.scan_result.root, &path) {
                                        set_checked_recursive(node, new_checked, &mut app.checked_paths);
                                    }
                                    
                                    app.update_visible_items();
                                }
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if !app.visible_items.is_empty() {
                                let selected = app.list_state.selected().unwrap_or(0);
                                let next = (selected + 1) % app.visible_items.len();
                                app.list_state.select(Some(next));
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if !app.visible_items.is_empty() {
                                let selected = app.list_state.selected().unwrap_or(0);
                                let prev = if selected == 0 {
                                    app.visible_items.len() - 1
                                } else {
                                    selected - 1
                                };
                                app.list_state.select(Some(prev));
                            }
                        }
                        KeyCode::Char('g') => {
                            if !app.visible_items.is_empty() {
                                app.list_state.select(Some(0));
                            }
                        }
                        KeyCode::Char('G') => {
                            if !app.visible_items.is_empty() {
                                app.list_state.select(Some(app.visible_items.len() - 1));
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
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(main_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(0)])
        .split(body_chunks[1]);

    let items: Vec<ListItem> = app.visible_items.iter().enumerate().map(|(idx, item)| {
        let is_selected = app.list_state.selected() == Some(idx);
        let checkbox = if item.checked { "☑ " } else { "☐ " };
        let highlight = if is_selected { "❯ " } else { "  " };
        
        let checkbox_color = if item.checked {
            Color::Rgb(158, 206, 106)
        } else {
            Color::Rgb(86, 95, 137)
        };

        let icon = ctx_render::get_node_icon(&item.name, item.kind == NodeKind::Directory);

        let text_style = if is_selected {
            Style::default().fg(Color::Rgb(192, 202, 245)).add_modifier(Modifier::BOLD)
        } else if item.checked {
            Style::default().fg(Color::Rgb(192, 202, 245))
        } else {
            Style::default().fg(Color::Rgb(86, 95, 137))
        };

        let expand_indicator = if item.kind == NodeKind::Directory {
            if item.is_expanded { "▼ " } else { "▶ " }
        } else {
            "  "
        };
        let expand_indicator_color = if is_selected {
            Color::Rgb(224, 175, 104)
        } else {
            Color::Rgb(86, 95, 137)
        };

        let stats_str = format!(" ({} lines, {} tokens)", item.lines, item.tokens);
        let stats_style = Style::default().fg(Color::Rgb(86, 95, 137));

        ListItem::new(Line::from(vec![
            Span::styled(highlight, Style::default().fg(Color::Rgb(224, 175, 104))),
            Span::styled(checkbox, Style::default().fg(checkbox_color)),
            Span::styled(item.tree_line_prefix.clone(), Style::default().fg(Color::Rgb(86, 95, 137))),
            Span::styled(expand_indicator, Style::default().fg(expand_indicator_color)),
            Span::raw(icon),
            Span::styled(&item.name, text_style),
            Span::styled(stats_str, stats_style),
        ]))
    }).collect();

    let files_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Workspace Tree Picker ", Style::default().fg(Color::Rgb(122, 162, 247)).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::Rgb(86, 95, 137)));
    let files_list = List::new(items)
        .block(files_block)
        .highlight_style(Style::default().bg(Color::Rgb(36, 40, 59)));
    f.render_stateful_widget(files_list, body_chunks[0], &mut app.list_state);

    let mut checked_files_nodes = Vec::new();
    collect_checked_files(&app.scan_result.root, &app.checked_paths, &mut checked_files_nodes);

    let total_files = count_all_files(&app.scan_result.root);
    let checked_files = checked_files_nodes.len();
    let total_tokens: usize = sum_all_tokens(&app.scan_result.root);
    let checked_tokens: usize = checked_files_nodes.iter().map(|f| f.stats.tokens).sum();
    let checked_lines: usize = checked_files_nodes.iter().map(|f| f.stats.lines).sum();
    let total_bytes: u64 = sum_all_bytes(&app.scan_result.root);
    let checked_bytes: u64 = checked_files_nodes.iter().map(|f| f.stats.bytes).sum();

    let stats_text = vec![
        Line::from(vec![
            Span::styled("Scan Path: ", Style::default().fg(Color::Rgb(125, 207, 255))),
            Span::styled(app.path.to_string_lossy().to_string(), Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Files: ", Style::default().fg(Color::Rgb(125, 207, 255))),
            Span::styled(format!("{} / {}", checked_files, total_files), Style::default().fg(Color::Rgb(192, 202, 245))),
            Span::styled("  │  Tokens: ", Style::default().fg(Color::Rgb(125, 207, 255))),
            Span::styled(format!("{} / {}", checked_tokens, total_tokens), Style::default().fg(Color::Rgb(158, 206, 106)).add_modifier(Modifier::BOLD)),
            Span::styled("  │  Lines: ", Style::default().fg(Color::Rgb(125, 207, 255))),
            Span::styled(format!("{}", checked_lines), Style::default().fg(Color::Rgb(187, 154, 247))),
            Span::styled("  │  Size: ", Style::default().fg(Color::Rgb(125, 207, 255))),
            Span::styled(format!("{} / {}", format_bytes(checked_bytes), format_bytes(total_bytes)), Style::default().fg(Color::Rgb(192, 202, 245))),
        ]),
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::Rgb(125, 207, 255))),
            match &app.message {
                Some((msg, ts)) => {
                    if ts.elapsed().as_secs() < 4 {
                        Span::styled(msg, Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD))
                    } else {
                        Span::styled("Idle", Style::default().fg(Color::Rgb(86, 95, 137)))
                    }
                }
                None => Span::styled("Idle", Style::default().fg(Color::Rgb(86, 95, 137))),
            }
        ]),
    ];

    let stats_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Context Summary ", Style::default().fg(Color::Rgb(122, 162, 247)).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::Rgb(86, 95, 137)));
    let stats_paragraph = Paragraph::new(stats_text)
        .block(stats_block);
    f.render_widget(stats_paragraph, right_chunks[0]);

    let preview_text = if let Some(selected_idx) = app.list_state.selected() {
        if let Some(item) = app.visible_items.get(selected_idx) {
            if item.kind == NodeKind::Directory {
                let size_str = format_bytes(item.bytes);
                vec![
                    Line::from(Span::styled(format!("📁 Directory: {}", item.name), Style::default().fg(Color::Rgb(122, 162, 247)).add_modifier(Modifier::BOLD))),
                    Line::from(""),
                    Line::from(vec![
                        Span::raw("  Path        : "),
                        Span::styled(item.path.to_string_lossy().to_string(), Style::default().fg(Color::Rgb(224, 175, 104))),
                    ]),
                    Line::from(vec![
                        Span::raw("  Total Lines : "),
                        Span::styled(format!("{}", item.lines), Style::default().fg(Color::Rgb(158, 206, 106))),
                    ]),
                    Line::from(vec![
                        Span::raw("  Total Tokens: "),
                        Span::styled(format!("{}", item.tokens), Style::default().fg(Color::Rgb(187, 154, 247))),
                    ]),
                    Line::from(vec![
                        Span::raw("  Total Size  : "),
                        Span::styled(size_str, Style::default().fg(Color::Rgb(125, 207, 255))),
                    ]),
                ]
            } else if !item.is_text {
                vec![Line::from(Span::styled("[Binary or Non-UTF8 File - No Preview]", Style::default().fg(Color::Rgb(247, 118, 142))))]
            } else {
                let extension = std::path::Path::new(&item.name)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("");
                
                match std::fs::read_to_string(&item.path) {
                    Ok(content) => {
                        content.lines()
                            .take(40)
                            .map(|l| highlight_line(l, extension))
                            .collect()
                    }
                    Err(e) => vec![Line::from(Span::styled(format!("Read error: {}", e), Style::default().fg(Color::Rgb(247, 118, 142))))],
                }
            }
        } else {
            vec![]
        }
    } else {
        vec![Line::from(Span::styled("No file selected", Style::default().fg(Color::Rgb(86, 95, 137))))]
    };

    let preview_title = if let Some(selected_idx) = app.list_state.selected() {
        if let Some(item) = app.visible_items.get(selected_idx) {
            format!(" Preview: {} ", item.name)
        } else {
            " Preview ".to_string()
        }
    } else {
        " Preview ".to_string()
    };

    let preview_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(preview_title, Style::default().fg(Color::Rgb(122, 162, 247)).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::Rgb(86, 95, 137)));
    let preview_paragraph = Paragraph::new(preview_text)
        .block(preview_block)
        .wrap(Wrap { trim: false });
    f.render_widget(preview_paragraph, right_chunks[1]);

    let help_spans = vec![
        Span::styled(" j/k / ▲▼", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
        Span::styled(" Move ", Style::default().fg(Color::Rgb(192, 202, 245))),
        Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
        Span::styled(" Space", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
        Span::styled(" Toggle ", Style::default().fg(Color::Rgb(192, 202, 245))),
        Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
        Span::styled(" Enter/o/h/l/◄►", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
        Span::styled(" Expand/Collapse ", Style::default().fg(Color::Rgb(192, 202, 245))),
        Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
        Span::styled(" c", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
        Span::styled(" Copy Context ", Style::default().fg(Color::Rgb(192, 202, 245))),
        Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
        Span::styled(" r", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
        Span::styled(" Rescan ", Style::default().fg(Color::Rgb(192, 202, 245))),
        Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
        Span::styled(" q / Esc", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
        Span::styled(" Quit", Style::default().fg(Color::Rgb(192, 202, 245))),
    ];
    let help_paragraph = Paragraph::new(Line::from(help_spans));
    f.render_widget(help_paragraph, main_chunks[1]);

    if let Some((msg, ts)) = &app.message {
        if ts.elapsed().as_secs() < 3 {
            draw_popup(f, "Copy Status", msg, 50, 15);
        }
    }
}

fn draw_popup(
    f: &mut ratatui::Frame,
    title: &str,
    message: &str,
    percent_x: u16,
    percent_y: u16,
) {
    let size = f.size();
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(size);

    let area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1];

    let block = Block::default()
        .title(Span::styled(format!(" {} ", title), Style::default().fg(Color::Rgb(158, 206, 106)).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(158, 206, 106)));

    let paragraph = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(format!("  {}", message), Style::default().fg(Color::Rgb(192, 202, 245)))),
        Line::from(""),
    ])
    .block(block)
    .wrap(Wrap { trim: false });

    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);
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
