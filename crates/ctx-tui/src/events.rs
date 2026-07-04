use crossterm::event::{self, Event, KeyCode};
use ctx_models::NodeKind;
use ratatui::Terminal;
use std::io;

use crate::app::{FocusedPanel, TuiApp, TuiScreen, find_node, set_checked_recursive};
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
                    if app.screen == TuiScreen::GraphContext {
                        match key.code {
                            KeyCode::Char('q') => {
                                return Ok(());
                            }
                            KeyCode::Esc | KeyCode::Char('s') => {
                                app.screen = TuiScreen::SymbolSearch;
                            }
                            KeyCode::Char('c') => {
                                match crate::clipboard::copy_graph_context_to_clipboard(app) {
                                    Ok(success_msg) => {
                                        app.message =
                                            Some((success_msg, std::time::Instant::now()));
                                    }
                                    Err(e) => {
                                        app.message = Some((
                                            format!("Error: {}", e),
                                            std::time::Instant::now(),
                                        ));
                                    }
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                app.graph_selected_option = (app.graph_selected_option + 1) % 4;
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.graph_selected_option = if app.graph_selected_option == 0 {
                                    3
                                } else {
                                    app.graph_selected_option - 1
                                };
                            }
                            KeyCode::Left
                            | KeyCode::Char('h')
                            | KeyCode::Right
                            | KeyCode::Char('l')
                            | KeyCode::Enter => {
                                let cycle_forward = matches!(
                                    key.code,
                                    KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter
                                );
                                match app.graph_selected_option {
                                    0 => {
                                        // Mode
                                        const MODES: &[ctx_codegraph::GraphContextMode] = &[
                                            ctx_codegraph::GraphContextMode::Callers,
                                            ctx_codegraph::GraphContextMode::Callees,
                                            ctx_codegraph::GraphContextMode::Dependencies,
                                            ctx_codegraph::GraphContextMode::Dependents,
                                            ctx_codegraph::GraphContextMode::ForwardSlice,
                                            ctx_codegraph::GraphContextMode::ReverseSlice,
                                            ctx_codegraph::GraphContextMode::Neighborhood,
                                        ];
                                        let current_idx = MODES
                                            .iter()
                                            .position(|&m| m == app.graph_mode)
                                            .unwrap_or(0);
                                        let next_idx = if cycle_forward {
                                            (current_idx + 1) % MODES.len()
                                        } else {
                                            if current_idx == 0 {
                                                MODES.len() - 1
                                            } else {
                                                current_idx - 1
                                            }
                                        };
                                        app.graph_mode = MODES[next_idx];
                                    }
                                    1 => {
                                        // Depth
                                        const DEPTHS: &[usize] = &[1, 2, 3];
                                        let current_idx = DEPTHS
                                            .iter()
                                            .position(|&d| d == app.graph_depth)
                                            .unwrap_or(1);
                                        let next_idx = if cycle_forward {
                                            (current_idx + 1) % DEPTHS.len()
                                        } else {
                                            if current_idx == 0 {
                                                DEPTHS.len() - 1
                                            } else {
                                                current_idx - 1
                                            }
                                        };
                                        app.graph_depth = DEPTHS[next_idx];
                                    }
                                    2 => {
                                        // Max nodes
                                        const MAX_NODES: &[usize] = &[20, 50, 100];
                                        let current_idx = MAX_NODES
                                            .iter()
                                            .position(|&n| n == app.graph_max_nodes)
                                            .unwrap_or(1);
                                        let next_idx = if cycle_forward {
                                            (current_idx + 1) % MAX_NODES.len()
                                        } else {
                                            if current_idx == 0 {
                                                MAX_NODES.len() - 1
                                            } else {
                                                current_idx - 1
                                            }
                                        };
                                        app.graph_max_nodes = MAX_NODES[next_idx];
                                    }
                                    3 => {
                                        // Include root
                                        app.graph_include_root = !app.graph_include_root;
                                    }
                                    _ => {}
                                }
                                app.update_graph_preview();
                            }
                            _ => {}
                        }
                    } else if app.screen == TuiScreen::SymbolSearch {
                        if app.symbol_search_active {
                            match key.code {
                                KeyCode::Esc | KeyCode::Enter => {
                                    app.symbol_search_active = false;
                                }
                                KeyCode::Backspace => {
                                    let mut q = app.symbol_search_query.clone();
                                    q.pop();
                                    app.set_symbol_search_query(q);
                                }
                                KeyCode::Down => {
                                    if !app.symbol_search_results.is_empty() {
                                        let selected =
                                            app.symbol_list_state.selected().unwrap_or(0);
                                        let next = (selected + 1) % app.symbol_search_results.len();
                                        app.symbol_list_state.select(Some(next));

                                        let sym = &app.symbol_search_results[next];
                                        app.preview_scroll_offset = if sym.range.start_line > 3 {
                                            sym.range.start_line - 3
                                        } else {
                                            0
                                        };
                                    }
                                }
                                KeyCode::Up => {
                                    if !app.symbol_search_results.is_empty() {
                                        let selected =
                                            app.symbol_list_state.selected().unwrap_or(0);
                                        let prev = if selected == 0 {
                                            app.symbol_search_results.len() - 1
                                        } else {
                                            selected - 1
                                        };
                                        app.symbol_list_state.select(Some(prev));

                                        let sym = &app.symbol_search_results[prev];
                                        app.preview_scroll_offset = if sym.range.start_line > 3 {
                                            sym.range.start_line - 3
                                        } else {
                                            0
                                        };
                                    }
                                }
                                KeyCode::Char(c) => {
                                    let mut q = app.symbol_search_query.clone();
                                    q.push(c);
                                    app.set_symbol_search_query(q);
                                }
                                _ => {}
                            }
                        } else {
                            match key.code {
                                KeyCode::Char('q') => {
                                    return Ok(());
                                }
                                KeyCode::Esc | KeyCode::Char('s') => {
                                    app.screen = TuiScreen::TreePicker;
                                }
                                KeyCode::Tab => {
                                    app.focused_panel = match app.focused_panel {
                                        FocusedPanel::Left => FocusedPanel::Right,
                                        FocusedPanel::Right => FocusedPanel::Left,
                                    };
                                }
                                KeyCode::Char('i') | KeyCode::Char('a') => {
                                    app.symbol_search_active = true;
                                    app.focused_panel = FocusedPanel::Left;
                                }
                                _ => match app.focused_panel {
                                    FocusedPanel::Left => match key.code {
                                        KeyCode::Down | KeyCode::Char('j') => {
                                            if !app.symbol_search_results.is_empty() {
                                                let selected =
                                                    app.symbol_list_state.selected().unwrap_or(0);
                                                let next = (selected + 1)
                                                    % app.symbol_search_results.len();
                                                app.symbol_list_state.select(Some(next));

                                                let sym = &app.symbol_search_results[next];
                                                app.preview_scroll_offset =
                                                    if sym.range.start_line > 3 {
                                                        sym.range.start_line - 3
                                                    } else {
                                                        0
                                                    };
                                            }
                                        }
                                        KeyCode::Up | KeyCode::Char('k') => {
                                            if !app.symbol_search_results.is_empty() {
                                                let selected =
                                                    app.symbol_list_state.selected().unwrap_or(0);
                                                let prev = if selected == 0 {
                                                    app.symbol_search_results.len() - 1
                                                } else {
                                                    selected - 1
                                                };
                                                app.symbol_list_state.select(Some(prev));

                                                let sym = &app.symbol_search_results[prev];
                                                app.preview_scroll_offset =
                                                    if sym.range.start_line > 3 {
                                                        sym.range.start_line - 3
                                                    } else {
                                                        0
                                                    };
                                            }
                                        }
                                        KeyCode::Enter | KeyCode::Char('g') => {
                                            if let Some(selected) = app.symbol_list_state.selected()
                                            {
                                                if let Some(sym) =
                                                    app.symbol_search_results.get(selected)
                                                {
                                                    let sym_clone = sym.clone();
                                                    app.selected_symbol = Some(sym_clone.clone());
                                                    app.screen = TuiScreen::GraphContext;
                                                    app.focused_panel = FocusedPanel::Left;
                                                    app.update_graph_preview();
                                                    app.message = Some((
                                                        format!(
                                                            "Opened graph context for: {}",
                                                            sym_clone.qualified_name
                                                        ),
                                                        std::time::Instant::now(),
                                                    ));
                                                }
                                            }
                                        }
                                        _ => {}
                                    },
                                    FocusedPanel::Right => match key.code {
                                        KeyCode::Up | KeyCode::Char('k') => {
                                            app.preview_scroll_up();
                                        }
                                        KeyCode::Down | KeyCode::Char('j') => {
                                            app.preview_scroll_down(
                                                app.last_preview_total_lines,
                                                app.last_preview_height,
                                            );
                                        }
                                        KeyCode::PageUp => {
                                            app.preview_page_up(
                                                app.last_preview_height.saturating_sub(2),
                                            );
                                        }
                                        KeyCode::PageDown => {
                                            app.preview_page_down(
                                                app.last_preview_height.saturating_sub(2),
                                                app.last_preview_total_lines,
                                                app.last_preview_height,
                                            );
                                        }
                                        _ => {}
                                    },
                                },
                            }
                        }
                    } else {
                        // TreePicker screen
                        if app.focused_panel == FocusedPanel::Right {
                            match key.code {
                                KeyCode::Char('q') => {
                                    return Ok(());
                                }
                                KeyCode::Tab => {
                                    app.focused_panel = FocusedPanel::Left;
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    app.preview_scroll_up();
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    app.preview_scroll_down(
                                        app.last_preview_total_lines,
                                        app.last_preview_height,
                                    );
                                }
                                KeyCode::PageUp => {
                                    app.preview_page_up(app.last_preview_height.saturating_sub(2));
                                }
                                KeyCode::PageDown => {
                                    app.preview_page_down(
                                        app.last_preview_height.saturating_sub(2),
                                        app.last_preview_total_lines,
                                        app.last_preview_height,
                                    );
                                }
                                _ => {}
                            }
                        } else if app.search_active {
                            match key.code {
                                KeyCode::Esc => {
                                    app.search_active = false;
                                    app.set_search_query(String::new());
                                    app.preview_scroll_offset = 0;
                                }
                                KeyCode::Enter => {
                                    app.search_active = false;
                                    app.preview_scroll_offset = 0;
                                }
                                KeyCode::Backspace => {
                                    let mut q = app.search_query.clone();
                                    q.pop();
                                    app.set_search_query(q);
                                    app.preview_scroll_offset = 0;
                                }
                                KeyCode::Down => {
                                    if !app.visible_items.is_empty() {
                                        let selected = app.list_state.selected().unwrap_or(0);
                                        let next = (selected + 1) % app.visible_items.len();
                                        app.list_state.select(Some(next));
                                        app.preview_scroll_offset = 0;
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
                                        app.preview_scroll_offset = 0;
                                    }
                                }
                                KeyCode::Char(c) => {
                                    let mut q = app.search_query.clone();
                                    q.push(c);
                                    app.set_search_query(q);
                                    app.preview_scroll_offset = 0;
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
                                        app.preview_scroll_offset = 0;
                                    } else {
                                        return Ok(());
                                    }
                                }
                                KeyCode::Tab => {
                                    app.focused_panel = FocusedPanel::Right;
                                }
                                KeyCode::Char('s') => {
                                    app.screen = TuiScreen::SymbolSearch;
                                    app.symbol_search_active = true;
                                    app.preview_scroll_offset = 0;
                                }
                                KeyCode::Char('c') => match copy_selection_to_clipboard(app) {
                                    Ok(success_msg) => {
                                        app.message =
                                            Some((success_msg, std::time::Instant::now()));
                                    }
                                    Err(e) => {
                                        app.message = Some((
                                            format!("Error: {}", e),
                                            std::time::Instant::now(),
                                        ));
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
                                            let clean_path = std::fs::canonicalize(&abs_path)
                                                .unwrap_or(abs_path);
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
                                        app.message = Some((
                                            format!("Error: {}", e),
                                            std::time::Instant::now(),
                                        ));
                                    }
                                }
                                KeyCode::Char(' ') | KeyCode::Char('x') => {
                                    if let Some(selected) = app.list_state.selected() {
                                        if let Some(item) = app.visible_items.get(selected) {
                                            let path = item.path.clone();
                                            let new_checked = !item.checked;

                                            if let Some(node) =
                                                find_node(&app.scan_result.root, &path)
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
                                        app.preview_scroll_offset = 0;
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
                                        app.preview_scroll_offset = 0;
                                    }
                                }
                                KeyCode::Char('g') => {
                                    if !app.visible_items.is_empty() {
                                        app.list_state.select(Some(0));
                                        app.preview_scroll_offset = 0;
                                    }
                                }
                                KeyCode::Char('G') => {
                                    if !app.visible_items.is_empty() {
                                        app.list_state.select(Some(app.visible_items.len() - 1));
                                        app.preview_scroll_offset = 0;
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
}
