use crossterm::event::{self, Event, KeyCode};
use ctx_models::NodeKind;
use ratatui::Terminal;
use std::io;

use crate::app::{TuiApp, find_node, set_checked_recursive};
use crate::clipboard::copy_selection_to_clipboard;
use crate::terminal::open_file;
use crate::ui::ui;

pub(crate) fn run_app<B: ratatui::backend::Backend>(
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
                            KeyCode::Char('c') => match copy_selection_to_clipboard(app) {
                                Ok(success_msg) => {
                                    app.message = Some((success_msg, std::time::Instant::now()));
                                }
                                Err(e) => {
                                    app.message =
                                        Some((format!("Error: {}", e), std::time::Instant::now()));
                                }
                            },
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
                                        let clean_path =
                                            std::fs::canonicalize(&abs_path).unwrap_or(abs_path);
                                        let path_str = clean_path.to_string_lossy().to_string();
                                        match arboard::Clipboard::new()
                                            .and_then(|mut cb| cb.set_text(path_str.clone()))
                                        {
                                            Ok(_) => {
                                                app.message = Some((
                                                    format!(
                                                        "Copied path to clipboard: {}",
                                                        path_str
                                                    ),
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
                                        } else if let Err(e) =
                                            open_file(terminal, &item.path, item.is_text)
                                        {
                                            app.message = Some((
                                                format!("Error opening file: {}", e),
                                                std::time::Instant::now(),
                                            ));
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('o') => {
                                if let Some(selected) = app.list_state.selected() {
                                    if let Some(item) = app.visible_items.get(selected) {
                                        if let Err(e) = open_file(
                                            terminal,
                                            &item.path,
                                            item.is_text && item.kind == NodeKind::File,
                                        ) {
                                            app.message = Some((
                                                format!("Error opening: {}", e),
                                                std::time::Instant::now(),
                                            ));
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
                                    app.message =
                                        Some((format!("Error: {}", e), std::time::Instant::now()));
                                }
                            }
                            KeyCode::Char(' ') | KeyCode::Char('x') => {
                                if let Some(selected) = app.list_state.selected() {
                                    if let Some(item) = app.visible_items.get(selected) {
                                        let path = item.path.clone();
                                        let new_checked = !item.checked;

                                        if let Some(node) = find_node(&app.scan_result.root, &path)
                                        {
                                            set_checked_recursive(
                                                node,
                                                new_checked,
                                                &mut app.checked_paths,
                                            );
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
