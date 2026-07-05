use ctx_models::{NodeKind, format_bytes};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

use crate::app::{
    FocusedPanel, TuiApp, TuiScreen, collect_checked_files, count_all_files, sum_all_bytes,
    sum_all_tokens,
};
use crate::preview::{highlight_line, highlight_line_matches, highlight_search_matches};

pub(crate) fn build_file_preview(
    app: &TuiApp,
    path: &std::path::Path,
    symbol_range: Option<ctx_codegraph::SourceRange>,
    visible_height: usize,
) -> (Vec<Line<'static>>, usize) {
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

    match ctx_models::read_file_content(path, u64::MAX) {
        ctx_models::FileContentResult::Text(content) => {
            let line_coverage = app.test_ctx.get_file_line_coverage(path);
            let all_lines: Vec<&str> = content.lines().collect();
            let total_lines = all_lines.len();

            let lines = all_lines
                .into_iter()
                .enumerate()
                .skip(app.preview_scroll_offset)
                .take(visible_height)
                .map(|(idx, l)| {
                    let line_num = idx + 1;
                    let hl = highlight_line(l, extension);

                    let query = if app.screen == TuiScreen::SymbolSearch {
                        &app.symbol_search_query
                    } else {
                        &app.search_query
                    };
                    let matched = highlight_line_matches(hl, query);

                    let is_in_symbol_range = if let Some(ref range) = symbol_range {
                        line_num >= range.start_line && line_num <= range.end_line
                    } else {
                        false
                    };

                    let gutter_style = if is_in_symbol_range {
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD)
                    } else if let Some(ref hits_map) = line_coverage {
                        if let Some(&hits) = hits_map.get(&line_num) {
                            if hits > 0 {
                                Style::default().fg(Color::Rgb(158, 206, 106)) // green
                            } else {
                                Style::default().fg(Color::Rgb(247, 118, 142)) // red
                            }
                        } else {
                            Style::default().fg(Color::Rgb(86, 95, 137)) // gray
                        }
                    } else {
                        Style::default().fg(Color::Rgb(86, 95, 137)) // gray
                    };

                    let indicator = if is_in_symbol_range {
                        "➤ "
                    } else if let Some(ref hits_map) = line_coverage {
                        if let Some(&hits) = hits_map.get(&line_num) {
                            if hits > 0 { "█ " } else { "█ " }
                        } else {
                            "  "
                        }
                    } else {
                        "  "
                    };

                    let prefix_span =
                        Span::styled(format!("{:3} │{}", line_num, indicator), gutter_style);

                    let mut spans = vec![prefix_span];
                    spans.extend(matched.spans);
                    Line::from(spans)
                })
                .collect();
            (lines, total_lines)
        }
        ctx_models::FileContentResult::Skipped(reason) => {
            let msg = match reason {
                ctx_models::FileSkipReason::TooLarge => "[File skipped: Too large]".to_string(),
                ctx_models::FileSkipReason::NonUtf8 => {
                    "[File skipped: Binary or non-UTF8]".to_string()
                }
            };
            (
                vec![Line::from(Span::styled(
                    msg,
                    Style::default().fg(Color::Rgb(247, 118, 142)),
                ))],
                1,
            )
        }
        ctx_models::FileContentResult::ReadError(e) => (
            vec![Line::from(Span::styled(
                format!("Read error: {}", e),
                Style::default().fg(Color::Rgb(247, 118, 142)),
            ))],
            1,
        ),
    }
}

pub(crate) fn ui(f: &mut ratatui::Frame, app: &mut TuiApp) {
    let size = f.size();

    match app.screen {
        TuiScreen::TreePicker => {
            let show_search_bar = app.search_active || !app.search_query.is_empty();
            let constraints = if show_search_bar {
                vec![
                    Constraint::Min(0),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ]
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

            let items: Vec<ListItem> = app
                .visible_items
                .iter()
                .enumerate()
                .map(|(idx, item)| {
                    let is_selected = app.list_state.selected() == Some(idx);
                    let checkbox = if item.checked { "☑ " } else { "☐ " };
                    let highlight = if is_selected { "❯ " } else { "  " };

                    let checkbox_color = if item.checked {
                        Color::Rgb(158, 206, 106)
                    } else {
                        Color::Rgb(86, 95, 137)
                    };

                    let icon =
                        ctx_render::get_node_icon(&item.name, item.kind == NodeKind::Directory);

                    let text_style = if is_selected {
                        Style::default()
                            .fg(Color::Rgb(192, 202, 245))
                            .add_modifier(Modifier::BOLD)
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

                    let mut stats_parts = vec![
                        format!("{} lines", item.lines),
                        format!("{} tokens", item.tokens),
                    ];
                    if item.tests > 0 {
                        stats_parts.push(format!("{} tests", item.tests));
                    }
                    if item.coverable_lines > 0 {
                        let cov = (item.covered_lines as f64 / item.coverable_lines as f64) * 100.0;
                        stats_parts.push(format!("{:.1}% cov", cov));
                    }
                    let stats_str = format!(" ({})", stats_parts.join(", "));
                    let stats_style = Style::default().fg(Color::Rgb(86, 95, 137));

                    let mut spans = vec![
                        Span::styled(highlight, Style::default().fg(Color::Rgb(224, 175, 104))),
                        Span::styled(checkbox, Style::default().fg(checkbox_color)),
                        Span::styled(
                            item.tree_line_prefix.clone(),
                            Style::default().fg(Color::Rgb(86, 95, 137)),
                        ),
                        Span::styled(
                            expand_indicator,
                            Style::default().fg(expand_indicator_color),
                        ),
                        Span::raw(icon),
                    ];

                    let highlight_style = Style::default()
                        .fg(Color::Rgb(36, 40, 59))
                        .bg(Color::Rgb(224, 175, 104))
                        .add_modifier(Modifier::BOLD);

                    spans.extend(highlight_search_matches(
                        &item.name,
                        &app.search_query,
                        text_style,
                        highlight_style,
                    ));
                    spans.push(Span::styled(stats_str, stats_style));

                    ListItem::new(Line::from(spans))
                })
                .collect();

            let files_block_border_color =
                if app.focused_panel == FocusedPanel::Left && !app.search_active {
                    Color::Rgb(224, 175, 104)
                } else {
                    Color::Rgb(86, 95, 137)
                };

            let files_block = Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    " Workspace Tree Picker ",
                    Style::default()
                        .fg(Color::Rgb(122, 162, 247))
                        .add_modifier(Modifier::BOLD),
                ))
                .border_style(Style::default().fg(files_block_border_color));
            let files_list = List::new(items)
                .block(files_block)
                .highlight_style(Style::default().bg(Color::Rgb(36, 40, 59)));
            f.render_stateful_widget(files_list, body_chunks[0], &mut app.list_state);

            let mut checked_files_nodes = Vec::new();
            collect_checked_files(
                &app.scan_result.root,
                &app.checked_paths,
                &mut checked_files_nodes,
            );

            let total_files = count_all_files(&app.scan_result.root);
            let checked_files = checked_files_nodes.len();
            let total_tokens: usize = sum_all_tokens(&app.scan_result.root);
            let checked_tokens: usize = checked_files_nodes.iter().map(|f| f.stats.tokens).sum();
            let checked_lines: usize = checked_files_nodes.iter().map(|f| f.stats.lines).sum();
            let total_bytes: u64 = sum_all_bytes(&app.scan_result.root);
            let checked_bytes: u64 = checked_files_nodes.iter().map(|f| f.stats.bytes).sum();

            let total_tests = app.scan_result.summary.tests;
            let checked_tests: usize = checked_files_nodes.iter().map(|f| f.stats.tests).sum();
            let total_coverable = app.scan_result.summary.coverable_lines;
            let total_covered = app.scan_result.summary.covered_lines;
            let checked_coverable: usize = checked_files_nodes
                .iter()
                .map(|f| f.stats.coverable_lines)
                .sum();
            let checked_covered: usize = checked_files_nodes
                .iter()
                .map(|f| f.stats.covered_lines)
                .sum();

            let stats_text = vec![
                Line::from(vec![
                    Span::styled(
                        "Scan Path: ",
                        Style::default().fg(Color::Rgb(125, 207, 255)),
                    ),
                    Span::styled(
                        app.path.to_string_lossy().to_string(),
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Files: ", Style::default().fg(Color::Rgb(125, 207, 255))),
                    Span::styled(
                        format!("{} / {}", checked_files, total_files),
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled(
                        "  │  Tokens: ",
                        Style::default().fg(Color::Rgb(125, 207, 255)),
                    ),
                    Span::styled(
                        format!("{} / {}", checked_tokens, total_tokens),
                        Style::default()
                            .fg(Color::Rgb(158, 206, 106))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "  │  Lines: ",
                        Style::default().fg(Color::Rgb(125, 207, 255)),
                    ),
                    Span::styled(
                        format!("{}", checked_lines),
                        Style::default().fg(Color::Rgb(187, 154, 247)),
                    ),
                    Span::styled(
                        "  │  Size: ",
                        Style::default().fg(Color::Rgb(125, 207, 255)),
                    ),
                    Span::styled(
                        format!(
                            "{} / {}",
                            format_bytes(checked_bytes),
                            format_bytes(total_bytes)
                        ),
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Tests: ", Style::default().fg(Color::Rgb(125, 207, 255))),
                    Span::styled(
                        format!("{} / {}", checked_tests, total_tests),
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled(
                        "  │  Coverage: ",
                        Style::default().fg(Color::Rgb(125, 207, 255)),
                    ),
                    Span::styled(
                        if total_coverable > 0 {
                            let cov = (total_covered as f64 / total_coverable as f64) * 100.0;
                            format!("{:.1}%", cov)
                        } else {
                            "N/A".to_string()
                        },
                        Style::default()
                            .fg(Color::Rgb(158, 206, 106))
                            .add_modifier(Modifier::BOLD),
                    ),
                    if checked_coverable > 0 {
                        Span::styled(
                            format!(
                                " (selected: {:.1}%)",
                                (checked_covered as f64 / checked_coverable as f64) * 100.0
                            ),
                            Style::default().fg(Color::Rgb(86, 95, 137)),
                        )
                    } else {
                        Span::raw("")
                    },
                ]),
                Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(Color::Rgb(125, 207, 255))),
                    match &app.message {
                        Some((msg, ts)) => {
                            if ts.elapsed().as_secs() < 4 {
                                Span::styled(
                                    msg,
                                    Style::default()
                                        .fg(Color::Rgb(224, 175, 104))
                                        .add_modifier(Modifier::BOLD),
                                )
                            } else {
                                Span::styled("Idle", Style::default().fg(Color::Rgb(86, 95, 137)))
                            }
                        }
                        None => Span::styled("Idle", Style::default().fg(Color::Rgb(86, 95, 137))),
                    },
                ]),
            ];

            let stats_block = Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    " Context Summary ",
                    Style::default()
                        .fg(Color::Rgb(122, 162, 247))
                        .add_modifier(Modifier::BOLD),
                ))
                .border_style(Style::default().fg(Color::Rgb(86, 95, 137)));
            let stats_paragraph = Paragraph::new(stats_text).block(stats_block);
            f.render_widget(stats_paragraph, right_chunks[0]);

            let (preview_text, total_lines) = if let Some(selected_idx) = app.list_state.selected()
            {
                if let Some(item) = app.visible_items.get(selected_idx) {
                    if item.kind == NodeKind::Directory {
                        let size_str = format_bytes(item.bytes);
                        let dir_lines = vec![
                            Line::from(Span::styled(
                                format!("📁 Directory: {}", item.name),
                                Style::default()
                                    .fg(Color::Rgb(122, 162, 247))
                                    .add_modifier(Modifier::BOLD),
                            )),
                            Line::from(""),
                            Line::from(vec![
                                Span::raw("  Path        : "),
                                Span::styled(
                                    item.path.to_string_lossy().to_string(),
                                    Style::default().fg(Color::Rgb(224, 175, 104)),
                                ),
                            ]),
                            Line::from(vec![
                                Span::raw("  Total Lines : "),
                                Span::styled(
                                    format!("{}", item.lines),
                                    Style::default().fg(Color::Rgb(158, 206, 106)),
                                ),
                            ]),
                            Line::from(vec![
                                Span::raw("  Total Tokens: "),
                                Span::styled(
                                    format!("{}", item.tokens),
                                    Style::default().fg(Color::Rgb(187, 154, 247)),
                                ),
                            ]),
                            Line::from(vec![
                                Span::raw("  Total Size  : "),
                                Span::styled(
                                    size_str,
                                    Style::default().fg(Color::Rgb(125, 207, 255)),
                                ),
                            ]),
                            Line::from(vec![
                                Span::raw("  Total Tests : "),
                                Span::styled(
                                    format!("{}", item.tests),
                                    Style::default().fg(Color::Rgb(187, 154, 247)),
                                ),
                            ]),
                            Line::from(vec![
                                Span::raw("  Coverage    : "),
                                Span::styled(
                                    if item.coverable_lines > 0 {
                                        let cov = (item.covered_lines as f64
                                            / item.coverable_lines as f64)
                                            * 100.0;
                                        format!("{:.1}%", cov)
                                    } else {
                                        "N/A".to_string()
                                    },
                                    Style::default().fg(Color::Rgb(158, 206, 106)),
                                ),
                            ]),
                        ];
                        (dir_lines, 8)
                    } else if !item.is_text {
                        (
                            vec![Line::from(Span::styled(
                                "[Binary or Non-UTF8 File - No Preview]",
                                Style::default().fg(Color::Rgb(247, 118, 142)),
                            ))],
                            1,
                        )
                    } else {
                        let visible_height = right_chunks[1].height.saturating_sub(2) as usize;
                        build_file_preview(app, &item.path, None, visible_height)
                    }
                } else {
                    (vec![], 0)
                }
            } else {
                (
                    vec![Line::from(Span::styled(
                        "No file selected",
                        Style::default().fg(Color::Rgb(86, 95, 137)),
                    ))],
                    1,
                )
            };

            app.last_preview_total_lines = total_lines;
            app.last_preview_height = right_chunks[1].height.saturating_sub(2) as usize;

            let preview_title = if let Some(selected_idx) = app.list_state.selected() {
                if let Some(item) = app.visible_items.get(selected_idx) {
                    format!(" Preview: {} ", item.name)
                } else {
                    " Preview ".to_string()
                }
            } else {
                " Preview ".to_string()
            };

            let preview_block_border_color = if app.focused_panel == FocusedPanel::Right {
                Color::Rgb(224, 175, 104)
            } else {
                Color::Rgb(86, 95, 137)
            };

            let preview_block = Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    preview_title,
                    Style::default()
                        .fg(Color::Rgb(122, 162, 247))
                        .add_modifier(Modifier::BOLD),
                ))
                .border_style(Style::default().fg(preview_block_border_color));
            let preview_paragraph = Paragraph::new(preview_text)
                .block(preview_block)
                .wrap(Wrap { trim: false });
            f.render_widget(preview_paragraph, right_chunks[1]);

            let help_spans = if app.search_active {
                vec![
                    Span::styled(
                        " Esc",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Cancel/Clear ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " Enter",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Accept & Navigate ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " ▲/▼",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Move selection ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                ]
            } else {
                let mut spans = vec![
                    Span::styled(
                        " Tab",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Focus Preview ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " s",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Symbol Search ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " j/k / ▲▼",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" Move ", Style::default().fg(Color::Rgb(192, 202, 245))),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " Space/x",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" Toggle ", Style::default().fg(Color::Rgb(192, 202, 245))),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " Enter/h/l/◄►",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Expand/Collapse ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " o",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" Open ", Style::default().fg(Color::Rgb(192, 202, 245))),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " c",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Copy Context ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " C",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Copy Path ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " f",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" Search ", Style::default().fg(Color::Rgb(192, 202, 245))),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " r",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" Rescan ", Style::default().fg(Color::Rgb(192, 202, 245))),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " q / Esc",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" Quit", Style::default().fg(Color::Rgb(192, 202, 245))),
                ];
                if !app.search_query.is_empty() {
                    spans.insert(
                        0,
                        Span::styled(
                            " Esc",
                            Style::default()
                                .fg(Color::Rgb(224, 175, 104))
                                .add_modifier(Modifier::BOLD),
                        ),
                    );
                    spans.insert(
                        1,
                        Span::styled(
                            " Clear Search ",
                            Style::default().fg(Color::Rgb(192, 202, 245)),
                        ),
                    );
                    spans.insert(
                        2,
                        Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    );
                }
                spans
            };

            let help_paragraph = Paragraph::new(Line::from(help_spans));

            if show_search_bar {
                let prefix = if app.search_active {
                    "🔍 Search (type to filter): "
                } else {
                    "🔍 Search (filtered): "
                };
                let search_text = Line::from(vec![
                    Span::styled(
                        prefix,
                        Style::default()
                            .fg(Color::Rgb(125, 207, 255))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        app.search_query.clone(),
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    if app.search_active {
                        Span::styled("█", Style::default().fg(Color::Rgb(192, 202, 245)))
                    } else {
                        Span::raw("")
                    },
                ]);
                let search_paragraph = Paragraph::new(search_text);
                f.render_widget(search_paragraph, main_chunks[1]);
                f.render_widget(help_paragraph, main_chunks[2]);
            } else {
                f.render_widget(help_paragraph, main_chunks[1]);
            }
        }
        TuiScreen::SymbolSearch => {
            let show_search_bar = app.symbol_search_active || !app.symbol_search_query.is_empty();
            let constraints = if show_search_bar {
                vec![
                    Constraint::Min(0),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ]
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

            // Draw left column: List of Symbol results
            let items: Vec<ListItem> = app
                .symbol_search_results
                .iter()
                .enumerate()
                .map(|(idx, item)| {
                    let is_selected = app.symbol_list_state.selected() == Some(idx);
                    let is_saved = app.selected_symbol.as_ref().map(|s| s.id) == Some(item.id);

                    let highlight = if is_selected { "❯ " } else { "  " };
                    let saved_indicator = if is_saved { "☑ " } else { "☐ " };

                    let text_style = if is_selected {
                        Style::default()
                            .fg(Color::Rgb(192, 202, 245))
                            .add_modifier(Modifier::BOLD)
                    } else if is_saved {
                        Style::default().fg(Color::Rgb(192, 202, 245))
                    } else {
                        Style::default().fg(Color::Rgb(86, 95, 137))
                    };

                    let kind_str = format!("{:?}", item.kind);
                    let path_rel = item
                        .file_path
                        .strip_prefix(&app.path)
                        .unwrap_or(&item.file_path);
                    let range_str = format!("L{}-{}", item.range.start_line, item.range.end_line);

                    let spans = vec![
                        Span::styled(highlight, Style::default().fg(Color::Rgb(224, 175, 104))),
                        Span::styled(
                            saved_indicator,
                            Style::default().fg(if is_saved {
                                Color::Rgb(158, 206, 106)
                            } else {
                                Color::Rgb(86, 95, 137)
                            }),
                        ),
                        Span::styled(
                            format!("[{}] ", kind_str),
                            Style::default().fg(Color::Rgb(187, 154, 247)),
                        ),
                        Span::styled(item.qualified_name.clone(), text_style),
                        Span::styled(
                            format!(" ({}:{})", path_rel.display(), range_str),
                            Style::default().fg(Color::Rgb(86, 95, 137)),
                        ),
                    ];

                    ListItem::new(Line::from(spans))
                })
                .collect();

            let symbol_block_border_color =
                if app.focused_panel == FocusedPanel::Left && !app.symbol_search_active {
                    Color::Rgb(224, 175, 104)
                } else {
                    Color::Rgb(86, 95, 137)
                };

            let symbol_block = Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    " Language Symbols Search ",
                    Style::default()
                        .fg(Color::Rgb(122, 162, 247))
                        .add_modifier(Modifier::BOLD),
                ))
                .border_style(Style::default().fg(symbol_block_border_color));

            let symbol_list = List::new(items)
                .block(symbol_block)
                .highlight_style(Style::default().bg(Color::Rgb(36, 40, 59)));
            f.render_stateful_widget(symbol_list, body_chunks[0], &mut app.symbol_list_state);

            // Draw right column: Context Summary & Preview
            let stats_text = vec![
                Line::from(vec![
                    Span::styled(
                        "Symbol Search Results: ",
                        Style::default().fg(Color::Rgb(125, 207, 255)),
                    ),
                    Span::styled(
                        format!("{} found", app.symbol_search_results.len()),
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(
                        "Selected Symbol: ",
                        Style::default().fg(Color::Rgb(125, 207, 255)),
                    ),
                    Span::styled(
                        app.selected_symbol
                            .as_ref()
                            .map(|s| s.qualified_name.clone())
                            .unwrap_or_else(|| "None".to_string()),
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(Color::Rgb(125, 207, 255))),
                    match &app.message {
                        Some((msg, ts)) => {
                            if ts.elapsed().as_secs() < 4 {
                                Span::styled(
                                    msg,
                                    Style::default()
                                        .fg(Color::Rgb(224, 175, 104))
                                        .add_modifier(Modifier::BOLD),
                                )
                            } else {
                                Span::styled("Idle", Style::default().fg(Color::Rgb(86, 95, 137)))
                            }
                        }
                        None => Span::styled("Idle", Style::default().fg(Color::Rgb(86, 95, 137))),
                    },
                ]),
            ];

            let stats_block = Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    " Selection Status ",
                    Style::default()
                        .fg(Color::Rgb(122, 162, 247))
                        .add_modifier(Modifier::BOLD),
                ))
                .border_style(Style::default().fg(Color::Rgb(86, 95, 137)));
            let stats_paragraph = Paragraph::new(stats_text).block(stats_block);
            f.render_widget(stats_paragraph, right_chunks[0]);

            // Preview containing selected search result
            let (preview_text, total_lines) =
                if let Some(selected_idx) = app.symbol_list_state.selected() {
                    if let Some(item) = app.symbol_search_results.get(selected_idx) {
                        let visible_height = right_chunks[1].height.saturating_sub(2) as usize;
                        build_file_preview(app, &item.file_path, Some(item.range), visible_height)
                    } else {
                        (vec![], 0)
                    }
                } else {
                    (
                        vec![Line::from(Span::styled(
                            "No symbol selected",
                            Style::default().fg(Color::Rgb(86, 95, 137)),
                        ))],
                        1,
                    )
                };

            app.last_preview_total_lines = total_lines;
            app.last_preview_height = right_chunks[1].height.saturating_sub(2) as usize;

            let preview_title = if let Some(selected_idx) = app.symbol_list_state.selected() {
                if let Some(item) = app.symbol_search_results.get(selected_idx) {
                    let path_rel = item
                        .file_path
                        .strip_prefix(&app.path)
                        .unwrap_or(&item.file_path);
                    format!(" Preview: {} ({}) ", path_rel.display(), item.name)
                } else {
                    " Preview ".to_string()
                }
            } else {
                " Preview ".to_string()
            };

            let preview_block_border_color = if app.focused_panel == FocusedPanel::Right {
                Color::Rgb(224, 175, 104)
            } else {
                Color::Rgb(86, 95, 137)
            };

            let preview_block = Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    preview_title,
                    Style::default()
                        .fg(Color::Rgb(122, 162, 247))
                        .add_modifier(Modifier::BOLD),
                ))
                .border_style(Style::default().fg(preview_block_border_color));
            let preview_paragraph = Paragraph::new(preview_text)
                .block(preview_block)
                .wrap(Wrap { trim: false });
            f.render_widget(preview_paragraph, right_chunks[1]);

            let help_spans = if app.symbol_search_active {
                vec![
                    Span::styled(
                        " Esc",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Stop Typing ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " ▲/▼",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Move selection ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                ]
            } else {
                let spans = vec![
                    Span::styled(
                        " Tab",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Swap Panel Focus ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " j/k / ▲▼",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Move/Scroll ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " i/a",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Edit Search ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " Enter",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Select Symbol ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " Esc/s",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Tree view ",
                        Style::default().fg(Color::Rgb(192, 202, 245)),
                    ),
                    Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                    Span::styled(
                        " q",
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" Quit", Style::default().fg(Color::Rgb(192, 202, 245))),
                ];
                spans
            };

            let help_paragraph = Paragraph::new(Line::from(help_spans));

            if show_search_bar {
                let prefix = if app.symbol_search_active {
                    "🔍 Symbol Search (type to find): "
                } else {
                    "🔍 Symbol Search (query): "
                };
                let search_text = Line::from(vec![
                    Span::styled(
                        prefix,
                        Style::default()
                            .fg(Color::Rgb(125, 207, 255))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        app.symbol_search_query.clone(),
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD),
                    ),
                    if app.symbol_search_active {
                        Span::styled("█", Style::default().fg(Color::Rgb(192, 202, 245)))
                    } else {
                        Span::raw("")
                    },
                ]);
                let search_paragraph = Paragraph::new(search_text);
                f.render_widget(search_paragraph, main_chunks[1]);
                f.render_widget(help_paragraph, main_chunks[2]);
            } else {
                f.render_widget(help_paragraph, main_chunks[1]);
            }
        }
        TuiScreen::GraphContext => {
            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(size);

            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(main_chunks[0]);

            // Configuration Options Panel
            let options_block = Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    " Graph Context Options ",
                    Style::default()
                        .fg(Color::Rgb(122, 162, 247))
                        .add_modifier(Modifier::BOLD),
                ))
                .border_style(Style::default().fg(Color::Rgb(122, 162, 247)));

            let mode_str = match app.graph_mode {
                ctx_codegraph::GraphContextMode::Callers => "Callers",
                ctx_codegraph::GraphContextMode::Callees => "Callees",
                ctx_codegraph::GraphContextMode::Dependencies => "Dependencies",
                ctx_codegraph::GraphContextMode::Dependents => "Dependents",
                ctx_codegraph::GraphContextMode::ForwardSlice => "Forward slice",
                ctx_codegraph::GraphContextMode::ReverseSlice => "Reverse slice",
                ctx_codegraph::GraphContextMode::Forward => "Forward",
                ctx_codegraph::GraphContextMode::Reverse => "Reverse",
                ctx_codegraph::GraphContextMode::Neighborhood => "Neighborhood",
                ctx_codegraph::GraphContextMode::Impact => "Impact",
            };

            let option_items = vec![
                ("Mode", mode_str.to_string()),
                ("Depth", app.graph_depth.to_string()),
                ("Max nodes", app.graph_max_nodes.to_string()),
                (
                    "Include root",
                    if app.graph_include_root {
                        "Yes".to_string()
                    } else {
                        "No".to_string()
                    },
                ),
            ];

            let items: Vec<ListItem> = option_items
                .into_iter()
                .enumerate()
                .map(|(idx, (name, val))| {
                    let is_selected = app.graph_selected_option == idx;
                    let prefix = if is_selected { "❯ " } else { "  " };
                    let name_style = if is_selected {
                        Style::default()
                            .fg(Color::Rgb(224, 175, 104))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(192, 202, 245))
                    };
                    let val_style = Style::default()
                        .fg(Color::Rgb(158, 206, 106))
                        .add_modifier(Modifier::BOLD);

                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(Color::Rgb(224, 175, 104))),
                        Span::styled(format!("{}: ", name), name_style),
                        Span::styled(val, val_style),
                    ]))
                })
                .collect();

            let options_list = List::new(items).block(options_block);
            f.render_widget(options_list, body_chunks[0]);

            // Preview Panel
            let preview_block = Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    " Context Preview ",
                    Style::default()
                        .fg(Color::Rgb(122, 162, 247))
                        .add_modifier(Modifier::BOLD),
                ))
                .border_style(Style::default().fg(Color::Rgb(86, 95, 137)));

            let preview_widget = match app.render_tui_graph_preview() {
                Ok(text) => Paragraph::new(text)
                    .block(preview_block)
                    .wrap(Wrap { trim: false }),
                Err(err_msg) => {
                    let err_spans = vec![
                        Line::from(Span::styled(
                            "ERROR STATE",
                            Style::default()
                                .fg(Color::Rgb(247, 118, 142))
                                .add_modifier(Modifier::BOLD),
                        )),
                        Line::from(""),
                        Line::from(Span::styled(
                            err_msg,
                            Style::default().fg(Color::Rgb(247, 118, 142)),
                        )),
                    ];
                    Paragraph::new(err_spans)
                        .block(preview_block)
                        .wrap(Wrap { trim: false })
                }
            };
            f.render_widget(preview_widget, body_chunks[1]);

            // Help bar
            let help_spans = vec![
                Span::styled(
                    " Esc/s",
                    Style::default()
                        .fg(Color::Rgb(224, 175, 104))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Back ", Style::default().fg(Color::Rgb(192, 202, 245))),
                Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                Span::styled(
                    " ▲/▼/j/k",
                    Style::default()
                        .fg(Color::Rgb(224, 175, 104))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " Select Option ",
                    Style::default().fg(Color::Rgb(192, 202, 245)),
                ),
                Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                Span::styled(
                    " ◄/►/Enter",
                    Style::default()
                        .fg(Color::Rgb(224, 175, 104))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " Cycle Value ",
                    Style::default().fg(Color::Rgb(192, 202, 245)),
                ),
                Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                Span::styled(
                    " c",
                    Style::default()
                        .fg(Color::Rgb(224, 175, 104))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " Copy Context ",
                    Style::default().fg(Color::Rgb(192, 202, 245)),
                ),
                Span::styled("│", Style::default().fg(Color::Rgb(86, 95, 137))),
                Span::styled(
                    " q",
                    Style::default()
                        .fg(Color::Rgb(224, 175, 104))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Quit", Style::default().fg(Color::Rgb(192, 202, 245))),
            ];
            let help_paragraph = Paragraph::new(Line::from(help_spans));
            f.render_widget(help_paragraph, main_chunks[1]);
        }
    }

    if let Some((msg, ts)) = &app.message {
        if ts.elapsed().as_secs() < 3 {
            draw_popup(f, "Copy Status", msg, 50, 15);
        }
    }
}

fn draw_popup(f: &mut ratatui::Frame, title: &str, message: &str, percent_x: u16, percent_y: u16) {
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
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(Color::Rgb(158, 206, 106))
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(158, 206, 106)));

    let paragraph = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", message),
            Style::default().fg(Color::Rgb(192, 202, 245)),
        )),
        Line::from(""),
    ])
    .block(block)
    .wrap(Wrap { trim: false });

    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);
}
