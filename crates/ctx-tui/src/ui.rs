use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use ctx_models::{NodeKind, format_bytes};

use crate::app::{
    TuiApp, collect_checked_files, count_all_files, sum_all_tokens, sum_all_bytes,
};
use crate::preview::{highlight_line, highlight_line_matches, highlight_search_matches};

pub(crate) fn ui(f: &mut ratatui::Frame, app: &mut TuiApp) {
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
                
                match ctx_models::read_file_content(&item.path, u64::MAX) {
                    ctx_models::FileContentResult::Text(content) => {
                        content.lines()
                            .take(40)
                            .map(|l| {
                                let hl = highlight_line(l, extension);
                                highlight_line_matches(hl, &app.search_query)
                            })
                            .collect()
                    }
                    ctx_models::FileContentResult::Skipped(reason) => {
                        let msg = match reason {
                            ctx_models::FileSkipReason::TooLarge => "[File skipped: Too large]".to_string(),
                            ctx_models::FileSkipReason::NonUtf8 => "[File skipped: Binary or non-UTF8]".to_string(),
                        };
                        vec![Line::from(Span::styled(msg, Style::default().fg(Color::Rgb(247, 118, 142))))]
                    }
                    ctx_models::FileContentResult::ReadError(e) => {
                        vec![Line::from(Span::styled(format!("Read error: {}", e), Style::default().fg(Color::Rgb(247, 118, 142))))]
                    }
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
