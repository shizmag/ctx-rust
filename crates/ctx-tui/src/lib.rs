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
    search_active: bool,
    search_query: String,
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
        let checked_paths = HashSet::new();
        
        expanded_dirs.insert(scan_result.root.path.clone());

        let mut app = Self {
            path,
            scan_result,
            expanded_dirs,
            checked_paths,
            visible_items: Vec::new(),
            list_state: ListState::default(),
            message: None,
            search_active: false,
            search_query: String::new(),
        };
        
        app.update_visible_items();
        
        if !app.visible_items.is_empty() {
            app.list_state.select(Some(0));
        }

        Ok(app)
    }

    fn update_visible_items(&mut self) {
        if !self.search_query.is_empty() {
            let mut matches = Vec::new();
            collect_matching_files(&self.scan_result.root, &self.search_query, &mut matches);
            
            self.visible_items = matches
                .into_iter()
                .map(|node| {
                    let checked = self.checked_paths.contains(&node.path);
                    let rel_path = match node.path.strip_prefix(&self.path) {
                        Ok(rel) => rel.to_string_lossy().to_string(),
                        Err(_) => node.path.to_string_lossy().to_string(),
                    };
                    VisibleTuiNode {
                        path: node.path.clone(),
                        name: rel_path,
                        kind: node.kind,
                        is_expanded: false,
                        checked,
                        lines: node.stats.lines,
                        tokens: node.stats.tokens,
                        bytes: node.stats.bytes,
                        tree_line_prefix: String::new(),
                        is_text: node.stats.lines > 0 || node.stats.bytes == 0,
                    }
                })
                .collect();
        } else {
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

    fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.update_visible_items();
        
        let selected = self.list_state.selected().unwrap_or(0);
        if self.visible_items.is_empty() {
            self.list_state.select(None);
        } else if selected >= self.visible_items.len() {
            self.list_state.select(Some(self.visible_items.len() - 1));
        } else {
            self.list_state.select(Some(selected));
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
    if node.kind == NodeKind::Directory && old_expanded.contains(&node.path) {
        new_expanded.insert(node.path.clone());
    }
    
    if old_checked.contains(&node.path) {
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

fn collect_matching_files<'a>(
    node: &'a TreeNode,
    query: &str,
    matches: &mut Vec<&'a TreeNode>,
) {
    let query_lower = query.to_lowercase();
    collect_matching_files_impl(node, &query_lower, matches);
}

fn collect_matching_files_impl<'a>(
    node: &'a TreeNode,
    query_lower: &str,
    matches: &mut Vec<&'a TreeNode>,
) {
    if node.kind == NodeKind::File {
        let name_matches = node.name.to_lowercase().contains(query_lower);
        let mut content_matches = false;
        if !name_matches && node.stats.lines > 0 && node.stats.bytes <= 512 * 1024 {
            if let Ok(content) = std::fs::read_to_string(&node.path) {
                if content.to_lowercase().contains(query_lower) {
                    content_matches = true;
                }
            }
        }
        if name_matches || content_matches {
            matches.push(node);
        }
    }
    for child in &node.children {
        collect_matching_files_impl(child, query_lower, matches);
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
                    for ch in chars.by_ref() {
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
            for ch in chars.by_ref() {
                comment.push(ch);
            }
            spans.push(Span::styled(comment, Style::default().fg(comment_color)));
            break;
        } else if c == '"' || c == '\'' {
            let quote = c;
            chars.next();
            let mut s = quote.to_string();
            let mut escaped = false;
            for ch in chars.by_ref() {
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

fn highlight_search_matches<'a>(
    text: &'a str,
    query: &str,
    base_style: Style,
    highlight_style: Style,
) -> Vec<Span<'a>> {
    if query.is_empty() {
        return vec![Span::styled(text, base_style)];
    }
    
    let mut spans = Vec::new();
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();
    
    let mut last_idx = 0;
    while let Some(start_idx) = text_lower[last_idx..].find(&query_lower).map(|i| last_idx + i) {
        if start_idx > last_idx {
            spans.push(Span::styled(&text[last_idx..start_idx], base_style));
        }
        let end_idx = start_idx + query_lower.len();
        spans.push(Span::styled(&text[start_idx..end_idx], highlight_style));
        last_idx = end_idx;
    }
    
    if last_idx < text.len() {
        spans.push(Span::styled(&text[last_idx..], base_style));
    }
    
    spans
}

fn highlight_line_matches(line: Line<'static>, query: &str) -> Line<'static> {
    if query.is_empty() {
        return line;
    }
    
    let query_lower = query.to_lowercase();
    let mut new_spans = Vec::new();
    
    for span in line.spans {
        let text = span.content.to_string();
        let text_lower = text.to_lowercase();
        
        if text_lower.contains(&query_lower) {
            let mut last_idx = 0;
            while let Some(start_idx) = text_lower[last_idx..].find(&query_lower).map(|i| last_idx + i) {
                if start_idx > last_idx {
                    new_spans.push(Span::styled(
                        text[last_idx..start_idx].to_string(),
                        span.style,
                    ));
                }
                let end_idx = start_idx + query_lower.len();
                let highlight_style = span.style
                    .bg(Color::Rgb(224, 175, 104))
                    .fg(Color::Rgb(36, 40, 59))
                    .add_modifier(Modifier::BOLD);
                new_spans.push(Span::styled(
                    text[start_idx..end_idx].to_string(),
                    highlight_style,
                ));
                last_idx = end_idx;
            }
            if last_idx < text.len() {
                new_spans.push(Span::styled(
                    text[last_idx..].to_string(),
                    span.style,
                ));
            }
        } else {
            new_spans.push(span);
        }
    }
    
    Line::from(new_spans)
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

fn open_file<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    path: &Path,
    is_text: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if is_text {
        // Suspend TUI
        disable_raw_mode()?;
        execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        
        let editor = std::env::var("EDITOR")
            .unwrap_or_else(|_| "nvim".to_string());
            
        let mut child = std::process::Command::new(editor)
            .arg(path)
            .spawn()?;
            
        child.wait()?;
        
        // Restore TUI
        enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableMouseCapture
        )?;
        terminal.clear()?;
    } else {
        // Open using default system app (background)
        #[cfg(target_os = "macos")]
        std::process::Command::new("open").arg(path).spawn()?;
        #[cfg(target_os = "windows")]
        std::process::Command::new("cmd").args(["/C", "start"]).arg(path).spawn()?;
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        std::process::Command::new("xdg-open").arg(path).spawn()?;
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
                    if app.search_active {
                        match key.code {
                            KeyCode::Esc => {
                                app.search_active = false;
                                app.set_search_query(String::new());
                            }
                            KeyCode::Enter => {
                                app.search_active = false;
                            }
                            KeyCode::Backspace => {
                                let mut q = app.search_query.clone();
                                q.pop();
                                app.set_search_query(q);
                            }
                            KeyCode::Down => {
                                if !app.visible_items.is_empty() {
                                    let selected = app.list_state.selected().unwrap_or(0);
                                    let next = (selected + 1) % app.visible_items.len();
                                    app.list_state.select(Some(next));
                                }
                            }
                            KeyCode::Up => {
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
                            KeyCode::Char(c) => {
                                let mut q = app.search_query.clone();
                                q.push(c);
                                app.set_search_query(q);
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char('q') => {
                                return Ok(());
                            }
                            KeyCode::Esc => {
                                if !app.search_query.is_empty() {
                                    app.set_search_query(String::new());
                                } else {
                                    return Ok(());
                                }
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
                            KeyCode::Char('C') => {
                                if let Some(selected) = app.list_state.selected() {
                                    if let Some(item) = app.visible_items.get(selected) {
                                        let abs_path = if item.path.is_absolute() {
                                            item.path.clone()
                                        } else {
                                            std::env::current_dir()
                                                .map(|cwd| cwd.join(&item.path))
                                                .unwrap_or_else(|_| item.path.clone())
                                        };
                                        let clean_path = std::fs::canonicalize(&abs_path)
                                            .unwrap_or(abs_path);
                                        let path_str = clean_path.to_string_lossy().to_string();
                                        match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(path_str.clone())) {
                                            Ok(_) => {
                                                app.message = Some((
                                                    format!("Copied path to clipboard: {}", path_str),
                                                    std::time::Instant::now(),
                                                ));
                                            }
                                            Err(e) => {
                                                app.message = Some((
                                                    format!("Error copying path: {}", e),
                                                    std::time::Instant::now(),
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('f') => {
                                app.search_active = true;
                            }
                            KeyCode::Enter => {
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
                                        } else if let Err(e) = open_file(terminal, &item.path, item.is_text) {
                                            app.message = Some((format!("Error opening file: {}", e), std::time::Instant::now()));
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('o') => {
                                if let Some(selected) = app.list_state.selected() {
                                    if let Some(item) = app.visible_items.get(selected) {
                                        if let Err(e) = open_file(terminal, &item.path, item.is_text && item.kind == NodeKind::File) {
                                            app.message = Some((format!("Error opening: {}", e), std::time::Instant::now()));
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
                            KeyCode::Char(' ') | KeyCode::Char('x') => {
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
}

fn ui(f: &mut ratatui::Frame, app: &mut TuiApp) {
    let size = f.size();

    let show_search_bar = app.search_active || !app.search_query.is_empty();
    let constraints = if show_search_bar {
        vec![Constraint::Min(0), Constraint::Length(1), Constraint::Length(1)]
    } else {
        vec![Constraint::Min(0), Constraint::Length(1)]
    };

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
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

        let mut spans = vec![
            Span::styled(highlight, Style::default().fg(Color::Rgb(224, 175, 104))),
            Span::styled(checkbox, Style::default().fg(checkbox_color)),
            Span::styled(item.tree_line_prefix.clone(), Style::default().fg(Color::Rgb(86, 95, 137))),
            Span::styled(expand_indicator, Style::default().fg(expand_indicator_color)),
            Span::raw(icon),
        ];
        
        let highlight_style = Style::default()
            .fg(Color::Rgb(36, 40, 59))
            .bg(Color::Rgb(224, 175, 104))
            .add_modifier(Modifier::BOLD);
            
        spans.extend(highlight_search_matches(&item.name, &app.search_query, text_style, highlight_style));
        spans.push(Span::styled(stats_str, stats_style));

        ListItem::new(Line::from(spans))
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
                            .map(|l| {
                                let hl = highlight_line(l, extension);
                                highlight_line_matches(hl, &app.search_query)
                            })
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

    let help_spans = if app.search_active {
        vec![
            Span::styled(" Esc", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
            Span::styled(" Cancel/Clear ", Style::default().fg(Color::Rgb(192, 202, 245))),
            Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
            Span::styled(" Enter", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
            Span::styled(" Accept & Navigate ", Style::default().fg(Color::Rgb(192, 202, 245))),
            Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
            Span::styled(" ▲/▼", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
            Span::styled(" Move selection ", Style::default().fg(Color::Rgb(192, 202, 245))),
        ]
    } else {
        let mut spans = vec![
            Span::styled(" j/k / ▲▼", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
            Span::styled(" Move ", Style::default().fg(Color::Rgb(192, 202, 245))),
            Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
            Span::styled(" Space/x", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
            Span::styled(" Toggle ", Style::default().fg(Color::Rgb(192, 202, 245))),
            Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
            Span::styled(" Enter/h/l/◄►", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
            Span::styled(" Expand/Collapse ", Style::default().fg(Color::Rgb(192, 202, 245))),
            Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
            Span::styled(" o", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
            Span::styled(" Open ", Style::default().fg(Color::Rgb(192, 202, 245))),
            Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
            Span::styled(" c", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
            Span::styled(" Copy Context ", Style::default().fg(Color::Rgb(192, 202, 245))),
            Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
            Span::styled(" C", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
            Span::styled(" Copy Path ", Style::default().fg(Color::Rgb(192, 202, 245))),
            Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
            Span::styled(" f", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
            Span::styled(" Search ", Style::default().fg(Color::Rgb(192, 202, 245))),
            Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
            Span::styled(" r", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
            Span::styled(" Rescan ", Style::default().fg(Color::Rgb(192, 202, 245))),
            Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
            Span::styled(" q / Esc", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
            Span::styled(" Quit", Style::default().fg(Color::Rgb(192, 202, 245))),
        ];
        if !app.search_query.is_empty() {
            spans.insert(0, Span::styled(" Esc", Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)));
            spans.insert(1, Span::styled(" Clear Search ", Style::default().fg(Color::Rgb(192, 202, 245))));
            spans.insert(2, Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))));
        }
        spans
    };
    let help_paragraph = Paragraph::new(Line::from(help_spans));

    if show_search_bar {
        let prefix = if app.search_active { "🔍 Search (type to filter): " } else { "🔍 Search (filtered): " };
        let search_text = Line::from(vec![
            Span::styled(prefix, Style::default().fg(Color::Rgb(125, 207, 255)).add_modifier(Modifier::BOLD)),
            Span::styled(app.search_query.clone(), Style::default().fg(Color::Rgb(224, 175, 104)).add_modifier(Modifier::BOLD)),
            if app.search_active {
                Span::styled("█", Style::default().fg(Color::Rgb(192, 202, 245)))
            } else {
                Span::raw("")
            }
        ]);
        let search_paragraph = Paragraph::new(search_text);
        f.render_widget(search_paragraph, main_chunks[1]);
        f.render_widget(help_paragraph, main_chunks[2]);
    } else {
        f.render_widget(help_paragraph, main_chunks[1]);
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_models::{TreeNode, NodeKind, NodeStats};

    #[test]
    fn test_collect_matching_files() {
        let root = TreeNode {
            name: "root".to_string(),
            path: PathBuf::from("."),
            kind: NodeKind::Directory,
            stats: NodeStats::default(),
            children: vec![
                TreeNode {
                    name: "foo.txt".to_string(),
                    path: PathBuf::from("foo.txt"),
                    kind: NodeKind::File,
                    stats: NodeStats {
                        lines: 10,
                        bytes: 100,
                        ..Default::default()
                    },
                    children: vec![],
                },
                TreeNode {
                    name: "bar.rs".to_string(),
                    path: PathBuf::from("bar.rs"),
                    kind: NodeKind::File,
                    stats: NodeStats {
                        lines: 5,
                        bytes: 50,
                        ..Default::default()
                    },
                    children: vec![],
                },
            ],
        };

        // Search by name
        let mut matches = Vec::new();
        println!("Root children: {:?}", root.children.iter().map(|c| (&c.name, c.kind)).collect::<Vec<_>>());
        collect_matching_files(&root, "foo", &mut matches);
        println!("Matches found: {:?}", matches.iter().map(|m| &m.name).collect::<Vec<_>>());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "foo.txt");

        // Case insensitivity
        let mut matches = Vec::new();
        collect_matching_files(&root, "FOO", &mut matches);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "foo.txt");

        // Search for non-existent
        let mut matches = Vec::new();
        collect_matching_files(&root, "baz", &mut matches);
        assert!(matches.is_empty());

        // Search by content
        let temp_file_path = PathBuf::from("test_content_match.txt");
        std::fs::write(&temp_file_path, "Hello search world!").unwrap();
        
        let root_content = TreeNode {
            name: "root".to_string(),
            path: PathBuf::from("."),
            kind: NodeKind::Directory,
            stats: NodeStats::default(),
            children: vec![
                TreeNode {
                    name: "test_content_match.txt".to_string(),
                    path: temp_file_path.clone(),
                    kind: NodeKind::File,
                    stats: NodeStats {
                        lines: 1,
                        bytes: 20,
                        ..Default::default()
                    },
                    children: vec![],
                },
            ],
        };
        
        let mut matches = Vec::new();
        collect_matching_files(&root_content, "search world", &mut matches);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "test_content_match.txt");
        
        let _ = std::fs::remove_file(temp_file_path);
    }

    #[test]
    fn test_highlighting() {
        let base_style = Style::default().fg(Color::White);
        let highlight_style = Style::default().fg(Color::Black).bg(Color::Yellow);

        // Test highlight_search_matches
        let spans = highlight_search_matches("hello world", "world", base_style, highlight_style);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "hello ");
        assert_eq!(spans[0].style, base_style);
        assert_eq!(spans[1].content, "world");
        assert_eq!(spans[1].style, highlight_style);

        // Case insensitivity of highlight_search_matches
        let spans = highlight_search_matches("Hello World", "world", base_style, highlight_style);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "Hello ");
        assert_eq!(spans[1].content, "World");

        // Test highlight_line_matches
        let line = Line::from(vec![
            Span::styled("fn main()", base_style),
        ]);
        let line_hl = highlight_line_matches(line, "main");
        assert_eq!(line_hl.spans.len(), 3);
        assert_eq!(line_hl.spans[0].content, "fn ");
        assert_eq!(line_hl.spans[1].content, "main");
        assert_eq!(line_hl.spans[2].content, "()");
    }
}
