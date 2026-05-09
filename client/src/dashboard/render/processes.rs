use tui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::dashboard::DashboardApp;

use super::{format_bytes, Theme};

pub(super) fn draw_processes_tab<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    app: &DashboardApp,
    area: Rect,
) {
    // Split area if details panel is shown
    let (table_area, details_area) = if app.show_details {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };

    let filter_info = if app.process_filter.is_empty() {
        String::new()
    } else {
        format!(" [Filter: {}]", app.process_filter)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::BORDER))
        .title(Span::styled(
            format!(" 󰓁 Running Processes{} ", filter_info),
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));

    if let Some(server) = app.get_selected_server() {
        let filtered_processes = app.get_filtered_processes();
        if !filtered_processes.is_empty() {
            let rows: Vec<Row> = filtered_processes
                .iter()
                .map(|proc| {
                    let cpu_style = if proc.cpu_usage_percent > 50.0 {
                        Style::default().fg(Theme::ERROR)
                    } else if proc.cpu_usage_percent > 20.0 {
                        Style::default().fg(Theme::WARNING)
                    } else {
                        Style::default().fg(Theme::CPU_COLOR)
                    };

                    Row::new(vec![
                        Cell::from(Span::styled(
                            proc.pid.to_string(),
                            Style::default().fg(Theme::MUTED),
                        )),
                        Cell::from(Span::styled(&proc.name, Style::default().fg(Theme::TEXT))),
                        Cell::from(Span::styled(
                            format!("{:.1}%", proc.cpu_usage_percent),
                            cpu_style,
                        )),
                        Cell::from(Span::styled(
                            format_bytes(proc.memory_usage_bytes),
                            Style::default().fg(Theme::MEM_COLOR),
                        )),
                        Cell::from(Span::styled(
                            &proc.status,
                            Style::default().fg(Theme::MUTED),
                        )),
                    ])
                })
                .collect();

            let table = Table::new(rows)
                .header(
                    Row::new(vec!["PID", "Name", "CPU", "Memory", "Status"]).style(
                        Style::default()
                            .fg(Theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                )
                .block(block)
                .widths(&[
                    Constraint::Percentage(10),
                    Constraint::Percentage(40),
                    Constraint::Percentage(15),
                    Constraint::Percentage(20),
                    Constraint::Percentage(15),
                ])
                .highlight_style(
                    Style::default()
                        .bg(Theme::HIGHLIGHT_BG)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");

            let mut state = app.table_state.clone();
            f.render_stateful_widget(table, table_area, &mut state);

            // Draw details panel if shown
            if let Some(details_rect) = details_area {
                draw_process_details(f, app, details_rect);
            }
        } else if app.processes_cache.contains_key(&server.id) {
            let no_match = Paragraph::new(vec![
                Spans::from(Span::styled(
                    "󰍉 No processes match the filter",
                    Style::default().fg(Theme::WARNING),
                )),
                Spans::from(Span::raw("")),
                Spans::from(Span::styled(
                    "Press 'x' to clear filter",
                    Style::default().fg(Theme::MUTED),
                )),
            ])
            .alignment(Alignment::Center)
            .block(block);
            f.render_widget(no_match, table_area);
        } else {
            let not_connected = Paragraph::new("󰅛 Not connected. Press Enter to connect.")
                .style(Style::default().fg(Theme::MUTED))
                .alignment(Alignment::Center)
                .block(block);
            f.render_widget(not_connected, table_area);
        }
    } else {
        let no_server = Paragraph::new("No server selected.")
            .style(Style::default().fg(Theme::MUTED))
            .alignment(Alignment::Center)
            .block(block);
        f.render_widget(no_server, table_area);
    }
}

pub(super) fn draw_process_details<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    app: &DashboardApp,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::BORDER))
        .title(Span::styled(
            " 󰋼 Process Details ",
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));

    if let Some(proc) = app.get_selected_process() {
        let details = Paragraph::new(vec![
            Spans::from(vec![
                Span::styled("󰆧 PID: ", Style::default().fg(Theme::MUTED)),
                Span::styled(
                    proc.pid.to_string(),
                    Style::default()
                        .fg(Theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Spans::from(Span::raw("")),
            Spans::from(vec![
                Span::styled("󰘔 Name: ", Style::default().fg(Theme::MUTED)),
                Span::styled(&proc.name, Style::default().fg(Theme::ACCENT)),
            ]),
            Spans::from(Span::raw("")),
            Spans::from(vec![
                Span::styled("󰀄 User: ", Style::default().fg(Theme::MUTED)),
                Span::styled(&proc.user, Style::default().fg(Theme::TEXT)),
            ]),
            Spans::from(Span::raw("")),
            Spans::from(vec![
                Span::styled("󰻠 CPU: ", Style::default().fg(Theme::CPU_COLOR)),
                Span::styled(
                    format!("{:.2}%", proc.cpu_usage_percent),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Spans::from(Span::raw("")),
            Spans::from(vec![
                Span::styled("󰍛 Memory: ", Style::default().fg(Theme::MEM_COLOR)),
                Span::styled(
                    format_bytes(proc.memory_usage_bytes),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Spans::from(Span::raw("")),
            Spans::from(vec![
                Span::styled("󰐊 Status: ", Style::default().fg(Theme::MUTED)),
                Span::styled(&proc.status, Style::default().fg(Theme::SUCCESS)),
            ]),
            Spans::from(Span::raw("")),
            Spans::from(Span::styled(
                "󰆍 Command:",
                Style::default().fg(Theme::MUTED),
            )),
            Spans::from(Span::styled(
                &proc.command_line,
                Style::default().fg(Theme::TEXT),
            )),
        ])
        .block(block)
        .wrap(Wrap { trim: true });
        f.render_widget(details, area);
    } else {
        let no_selection = Paragraph::new("Select a process to view details")
            .style(Style::default().fg(Theme::MUTED))
            .alignment(Alignment::Center)
            .block(block);
        f.render_widget(no_selection, area);
    }
}
