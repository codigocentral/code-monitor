use shared::types::ServiceStatus;
use tui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, BorderType, Borders, Cell, Gauge, Paragraph, Row, Sparkline, Table},
    Frame,
};

use crate::dashboard::DashboardApp;

use super::{format_bytes, format_uptime, Theme};

/// Name the failed units on a single line for the overview header.
///
/// Capped so a host with many failures cannot push the rest of the header off
/// screen; the full list stays available on the Systemd tab.
fn summarize_failed_units(failed: &[shared::types::SystemdFailedUnit]) -> String {
    const MAX_NAMES: usize = 3;

    let mut summary = failed
        .iter()
        .take(MAX_NAMES)
        .map(|u| u.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    if failed.len() > MAX_NAMES {
        summary.push_str(&format!(" +{} more", failed.len() - MAX_NAMES));
    }

    summary
}

pub(super) fn draw_overview_tab<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    app: &DashboardApp,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::BORDER))
        .title(Span::styled(
            " 󰍹 System Overview ",
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));

    if let Some(server) = app.get_selected_server() {
        if let Some(info) = app.system_info_cache.get(&server.id) {
            let inner_area = block.inner(area);
            f.render_widget(block, area);

            let failed_units = app
                .systemd_failed_cache
                .get(&server.id)
                .map(|f| f.as_slice())
                .unwrap_or(&[]);
            // The failed-unit line only exists when something is failing, so it
            // costs no vertical space on a healthy host.
            let header_height = if failed_units.is_empty() { 3 } else { 4 };

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(header_height), // System info header
                    Constraint::Length(4),             // CPU gauge + sparkline
                    Constraint::Length(4),             // Memory gauge + sparkline
                    Constraint::Min(5),                // Disks
                ])
                .split(inner_area);

            // System info header
            let mut header_lines = vec![
                Spans::from(vec![
                    Span::styled("󰟀 ", Style::default().fg(Theme::ACCENT)),
                    Span::styled(
                        &info.hostname,
                        Style::default()
                            .fg(Theme::TEXT)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  │  ", Style::default().fg(Theme::BORDER)),
                    Span::styled("󰌽 ", Style::default().fg(Theme::SUCCESS)),
                    Span::styled(&info.os, Style::default().fg(Theme::TEXT)),
                ]),
                Spans::from(vec![
                    Span::styled("󰥔 ", Style::default().fg(Theme::WARNING)),
                    Span::styled(
                        format!("Uptime: {}", format_uptime(info.uptime_seconds)),
                        Style::default().fg(Theme::TEXT),
                    ),
                    Span::styled("  │  ", Style::default().fg(Theme::BORDER)),
                    Span::styled("󰻠 ", Style::default().fg(Theme::CPU_COLOR)),
                    Span::styled(
                        format!("CPUs: {}", info.cpu_count),
                        Style::default().fg(Theme::TEXT),
                    ),
                    Span::styled("  │  ", Style::default().fg(Theme::BORDER)),
                    Span::styled("󰘚 ", Style::default().fg(Theme::MEM_COLOR)),
                    Span::styled(&info.kernel_version, Style::default().fg(Theme::MUTED)),
                ]),
            ];

            if !failed_units.is_empty() {
                header_lines.push(Spans::from(vec![
                    Span::styled("󰅙 ", Style::default().fg(Theme::ERROR)),
                    Span::styled(
                        format!(
                            "{} systemd unit{} failed",
                            failed_units.len(),
                            if failed_units.len() == 1 { "" } else { "s" }
                        ),
                        Style::default()
                            .fg(Theme::ERROR)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  │  ", Style::default().fg(Theme::BORDER)),
                    Span::styled(
                        summarize_failed_units(failed_units),
                        Style::default().fg(Theme::MUTED),
                    ),
                ]));
            }

            let sys_info = Paragraph::new(header_lines);
            f.render_widget(sys_info, chunks[0]);

            // CPU section with gauge and sparkline
            let cpu_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(chunks[1]);

            let cpu_percent = info.cpu_usage_percent.clamp(0.0, 100.0);
            let cpu_color = if cpu_percent > 80.0 {
                Theme::ERROR
            } else if cpu_percent > 50.0 {
                Theme::WARNING
            } else {
                Theme::SUCCESS
            };
            let cpu_gauge = Gauge::default()
                .block(
                    Block::default()
                        .title(Span::styled("󰻠 CPU", Style::default().fg(Theme::CPU_COLOR)))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Theme::BORDER)),
                )
                .gauge_style(Style::default().fg(cpu_color).bg(Color::Black))
                .percent(cpu_percent as u16)
                .label(Span::styled(
                    format!("{:.1}%", cpu_percent),
                    Style::default()
                        .fg(Theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                ));
            f.render_widget(cpu_gauge, cpu_chunks[0]);

            // CPU Sparkline
            let cpu_history = app
                .cpu_history
                .get(&server.id)
                .map(|h| h.as_slice())
                .unwrap_or(&[]);
            let cpu_sparkline = Sparkline::default()
                .block(
                    Block::default()
                        .title("History")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Theme::BORDER)),
                )
                .data(cpu_history)
                .max(100)
                .style(Style::default().fg(Theme::CPU_COLOR));
            f.render_widget(cpu_sparkline, cpu_chunks[1]);

            // Memory section with gauge and sparkline
            let mem_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(chunks[2]);

            let mem_percent = (info.memory_used_bytes as f64 / info.memory_total_bytes as f64
                * 100.0)
                .clamp(0.0, 100.0);
            let mem_color = if mem_percent > 80.0 {
                Theme::ERROR
            } else if mem_percent > 50.0 {
                Theme::WARNING
            } else {
                Theme::SUCCESS
            };
            let mem_gauge = Gauge::default()
                .block(
                    Block::default()
                        .title(Span::styled(
                            format!(
                                "󰍛 Memory ({} / {})",
                                format_bytes(info.memory_used_bytes),
                                format_bytes(info.memory_total_bytes)
                            ),
                            Style::default().fg(Theme::MEM_COLOR),
                        ))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Theme::BORDER)),
                )
                .gauge_style(Style::default().fg(mem_color).bg(Color::Black))
                .percent(mem_percent as u16)
                .label(Span::styled(
                    format!("{:.1}%", mem_percent),
                    Style::default()
                        .fg(Theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                ));
            f.render_widget(mem_gauge, mem_chunks[0]);

            // Memory Sparkline
            let mem_history = app
                .mem_history
                .get(&server.id)
                .map(|h| h.as_slice())
                .unwrap_or(&[]);
            let mem_sparkline = Sparkline::default()
                .block(
                    Block::default()
                        .title("History")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Theme::BORDER)),
                )
                .data(mem_history)
                .max(100)
                .style(Style::default().fg(Theme::MEM_COLOR));
            f.render_widget(mem_sparkline, mem_chunks[1]);

            // Disks table with visual bars
            let disk_rows: Vec<Row> = info
                .disk_info
                .iter()
                .map(|disk| {
                    let usage_style = if disk.usage_percent > 90.0 {
                        Style::default()
                            .fg(Theme::ERROR)
                            .add_modifier(Modifier::BOLD)
                    } else if disk.usage_percent > 70.0 {
                        Style::default().fg(Theme::WARNING)
                    } else {
                        Style::default().fg(Theme::SUCCESS)
                    };

                    // Create visual bar
                    let bar_width = 15;
                    let filled = ((disk.usage_percent / 100.0) * bar_width as f64) as usize;
                    let bar = format!(
                        "{}{}",
                        "█".repeat(filled.min(bar_width)),
                        "░".repeat(bar_width.saturating_sub(filled))
                    );

                    Row::new(vec![
                        Cell::from(Span::styled(
                            format!("󰋊 {}", disk.mount_point),
                            Style::default().fg(Theme::TEXT),
                        )),
                        Cell::from(Span::styled(
                            format_bytes(disk.used_bytes),
                            Style::default().fg(Theme::MUTED),
                        )),
                        Cell::from(Span::styled(
                            format_bytes(disk.total_bytes),
                            Style::default().fg(Theme::MUTED),
                        )),
                        Cell::from(Span::styled(bar, usage_style)),
                        Cell::from(Span::styled(
                            format!("{:.1}%", disk.usage_percent),
                            usage_style,
                        )),
                    ])
                })
                .collect();

            let disk_table = Table::new(disk_rows)
                .header(
                    Row::new(vec!["Mount", "Used", "Total", "Usage", "%"]).style(
                        Style::default()
                            .fg(Theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                )
                .block(
                    Block::default()
                        .title(Span::styled(
                            "󰋊 Disks",
                            Style::default().fg(Theme::DISK_COLOR),
                        ))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Theme::BORDER)),
                )
                .widths(&[
                    Constraint::Percentage(25),
                    Constraint::Percentage(15),
                    Constraint::Percentage(15),
                    Constraint::Percentage(30),
                    Constraint::Percentage(15),
                ]);
            f.render_widget(disk_table, chunks[3]);
        } else {
            let not_connected = Paragraph::new(vec![
                Spans::from(Span::styled(
                    "󰅛 Not connected",
                    Style::default().fg(Theme::MUTED),
                )),
                Spans::from(Span::raw("")),
                Spans::from(Span::styled("Press ", Style::default().fg(Theme::MUTED))),
                Spans::from(Span::styled(
                    "Enter",
                    Style::default()
                        .fg(Theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                )),
                Spans::from(Span::styled(
                    " to connect to this server",
                    Style::default().fg(Theme::MUTED),
                )),
            ])
            .alignment(Alignment::Center)
            .block(block);
            f.render_widget(not_connected, area);
        }
    } else {
        let no_server = Paragraph::new(vec![
            Spans::from(Span::styled(
                "󰋗 No servers configured",
                Style::default().fg(Theme::WARNING),
            )),
            Spans::from(Span::raw("")),
            Spans::from(Span::styled("Press ", Style::default().fg(Theme::MUTED))),
            Spans::from(Span::styled(
                "'a'",
                Style::default()
                    .fg(Theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )),
            Spans::from(Span::styled(
                " to add a new server",
                Style::default().fg(Theme::MUTED),
            )),
        ])
        .alignment(Alignment::Center)
        .block(block);
        f.render_widget(no_server, area);
    }
}

pub(super) fn draw_services_tab<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    app: &DashboardApp,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::BORDER))
        .title(Span::styled(
            " 󰒍 Services / Long-running Processes ",
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));

    if let Some(server) = app.get_selected_server() {
        if let Some(services) = app.services_cache.get(&server.id) {
            let rows: Vec<Row> = services
                .iter()
                .map(|service| {
                    let (status_icon, status_style) = match service.status {
                        ServiceStatus::Running => ("󰐊", Style::default().fg(Theme::SUCCESS)),
                        ServiceStatus::Stopped => ("󰓛", Style::default().fg(Theme::ERROR)),
                        ServiceStatus::Failed => (
                            "󰅜",
                            Style::default()
                                .fg(Theme::ERROR)
                                .add_modifier(Modifier::BOLD),
                        ),
                        ServiceStatus::Unknown => ("󰋗", Style::default().fg(Theme::WARNING)),
                    };
                    Row::new(vec![
                        Cell::from(Span::styled(
                            &service.name,
                            Style::default().fg(Theme::TEXT),
                        )),
                        Cell::from(Span::styled(
                            format!("{} {:?}", status_icon, service.status),
                            status_style,
                        )),
                        Cell::from(Span::styled(
                            service
                                .pid
                                .map(|p| p.to_string())
                                .unwrap_or("-".to_string()),
                            Style::default().fg(Theme::MUTED),
                        )),
                        Cell::from(Span::styled(
                            format!("{:.1}%", service.cpu_usage_percent),
                            Style::default().fg(Theme::CPU_COLOR),
                        )),
                        Cell::from(Span::styled(
                            format_bytes(service.memory_usage_bytes),
                            Style::default().fg(Theme::MEM_COLOR),
                        )),
                        Cell::from(Span::styled(
                            service
                                .uptime_seconds
                                .map(format_uptime)
                                .unwrap_or("-".to_string()),
                            Style::default().fg(Theme::MUTED),
                        )),
                    ])
                })
                .collect();

            let table = Table::new(rows)
                .header(
                    Row::new(vec!["Name", "Status", "PID", "CPU", "Memory", "Uptime"]).style(
                        Style::default()
                            .fg(Theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                )
                .block(block)
                .widths(&[
                    Constraint::Percentage(30),
                    Constraint::Percentage(15),
                    Constraint::Percentage(10),
                    Constraint::Percentage(10),
                    Constraint::Percentage(15),
                    Constraint::Percentage(20),
                ])
                .highlight_style(
                    Style::default()
                        .bg(Theme::HIGHLIGHT_BG)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");

            let mut state = app.table_state.clone();
            f.render_stateful_widget(table, area, &mut state);
        } else {
            let not_connected = Paragraph::new("󰅛 Not connected. Press Enter to connect.")
                .style(Style::default().fg(Theme::MUTED))
                .alignment(Alignment::Center)
                .block(block);
            f.render_widget(not_connected, area);
        }
    } else {
        let no_server = Paragraph::new("No server selected.")
            .style(Style::default().fg(Theme::MUTED))
            .alignment(Alignment::Center)
            .block(block);
        f.render_widget(no_server, area);
    }
}

pub(super) fn draw_network_tab<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    app: &DashboardApp,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::BORDER))
        .title(Span::styled(
            " 󰛳 Network Interfaces ",
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));

    if let Some(server) = app.get_selected_server() {
        if let Some(networks) = app.network_cache.get(&server.id) {
            let rows: Vec<Row> = networks
                .iter()
                .map(|net| {
                    let (status_icon, status_style) = if net.is_up {
                        ("󰈀", Style::default().fg(Theme::SUCCESS))
                    } else {
                        ("󰈂", Style::default().fg(Theme::ERROR))
                    };
                    let status_text = if net.is_up { "UP" } else { "DOWN" };
                    Row::new(vec![
                        Cell::from(Span::styled(
                            format!("󰛳 {}", net.interface),
                            Style::default().fg(Theme::TEXT),
                        )),
                        Cell::from(Span::styled(
                            format!("{} {}", status_icon, status_text),
                            status_style,
                        )),
                        Cell::from(Span::styled(
                            &net.ip_address,
                            Style::default().fg(Theme::ACCENT),
                        )),
                        Cell::from(Span::styled(
                            &net.mac_address,
                            Style::default().fg(Theme::MUTED),
                        )),
                        Cell::from(Span::styled(
                            format!("↑ {}", format_bytes(net.bytes_sent)),
                            Style::default().fg(Theme::SUCCESS),
                        )),
                        Cell::from(Span::styled(
                            format!("↓ {}", format_bytes(net.bytes_received)),
                            Style::default().fg(Theme::CPU_COLOR),
                        )),
                    ])
                })
                .collect();

            let table = Table::new(rows)
                .header(
                    Row::new(vec![
                        "Interface",
                        "Status",
                        "IP Address",
                        "MAC Address",
                        "Sent",
                        "Received",
                    ])
                    .style(
                        Style::default()
                            .fg(Theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                )
                .block(block)
                .widths(&[
                    Constraint::Percentage(15),
                    Constraint::Percentage(10),
                    Constraint::Percentage(20),
                    Constraint::Percentage(25),
                    Constraint::Percentage(15),
                    Constraint::Percentage(15),
                ])
                .highlight_style(
                    Style::default()
                        .bg(Theme::HIGHLIGHT_BG)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");

            let mut state = app.table_state.clone();
            f.render_stateful_widget(table, area, &mut state);
        } else {
            let not_connected = Paragraph::new("󰅛 Not connected. Press Enter to connect.")
                .style(Style::default().fg(Theme::MUTED))
                .alignment(Alignment::Center)
                .block(block);
            f.render_widget(not_connected, area);
        }
    } else {
        let no_server = Paragraph::new("No server selected.")
            .style(Style::default().fg(Theme::MUTED))
            .alignment(Alignment::Center)
            .block(block);
        f.render_widget(no_server, area);
    }
}

pub(super) fn draw_containers_tab<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    app: &DashboardApp,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::BORDER))
        .title(Span::styled(
            " 󰡨 Docker Containers ",
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));

    if let Some(server) = app.get_selected_server() {
        if let Some(containers) = app.containers_cache.get(&server.id) {
            if containers.is_empty() {
                let no_containers =
                    Paragraph::new("󰅛 No containers found. Docker may not be available.")
                        .style(Style::default().fg(Theme::MUTED))
                        .alignment(Alignment::Center)
                        .block(block);
                f.render_widget(no_containers, area);
                return;
            }

            let rows: Vec<Row> = containers
                .iter()
                .map(|c| {
                    let (status_icon, status_style) = match c.state.as_str() {
                        "running" => ("󰐝", Style::default().fg(Theme::SUCCESS)),
                        "paused" => ("󰏤", Style::default().fg(Theme::WARNING)),
                        _ => ("󰏠", Style::default().fg(Theme::ERROR)),
                    };

                    let health_icon = match c.health.as_str() {
                        "healthy" => Some((" 󰄬", Style::default().fg(Theme::SUCCESS))),
                        "unhealthy" => Some((" 󰅙", Style::default().fg(Theme::ERROR))),
                        _ => None,
                    };

                    let status_text = if c.status.is_empty() {
                        c.state.clone()
                    } else {
                        c.status.clone()
                    };

                    let mut status_spans = vec![
                        Span::styled(format!("{} ", status_icon), status_style),
                        Span::styled(status_text, status_style),
                    ];

                    if let Some((icon, style)) = health_icon {
                        status_spans.push(Span::styled(icon, style));
                    }

                    Row::new(vec![
                        Cell::from(Span::styled(&c.name, Style::default().fg(Theme::TEXT))),
                        Cell::from(Span::styled(
                            c.image.split(':').next().unwrap_or(&c.image).to_string(),
                            Style::default().fg(Theme::MUTED),
                        )),
                        Cell::from(Spans::from(status_spans)),
                        Cell::from(Span::styled(
                            format!("{:>5.1}%", c.cpu_percent),
                            if c.cpu_percent > 80.0 {
                                Style::default().fg(Theme::ERROR)
                            } else if c.cpu_percent > 50.0 {
                                Style::default().fg(Theme::WARNING)
                            } else {
                                Style::default().fg(Theme::TEXT)
                            },
                        )),
                        Cell::from(Span::styled(
                            format!(
                                "{} / {}",
                                format_bytes(c.memory_usage_bytes),
                                format_bytes(c.memory_limit_bytes)
                            ),
                            Style::default().fg(Theme::TEXT),
                        )),
                        Cell::from(Span::styled(
                            format!("{:>5.1}%", c.memory_percent),
                            if c.memory_percent > 80.0 {
                                Style::default().fg(Theme::ERROR)
                            } else if c.memory_percent > 50.0 {
                                Style::default().fg(Theme::WARNING)
                            } else {
                                Style::default().fg(Theme::TEXT)
                            },
                        )),
                    ])
                })
                .collect();

            let table = Table::new(rows)
                .header(
                    Row::new(vec!["Name", "Image", "Status", "CPU", "Memory", "MEM %"]).style(
                        Style::default()
                            .fg(Theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                )
                .block(block)
                .widths(&[
                    Constraint::Percentage(22),
                    Constraint::Percentage(23),
                    Constraint::Percentage(18),
                    Constraint::Percentage(10),
                    Constraint::Percentage(17),
                    Constraint::Percentage(10),
                ])
                .highlight_style(
                    Style::default()
                        .bg(Theme::HIGHLIGHT_BG)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");

            let mut state = app.table_state.clone();
            f.render_stateful_widget(table, area, &mut state);
        } else {
            let not_connected = Paragraph::new("󰅛 Not connected. Press Enter to connect.")
                .style(Style::default().fg(Theme::MUTED))
                .alignment(Alignment::Center)
                .block(block);
            f.render_widget(not_connected, area);
        }
    } else {
        let no_server = Paragraph::new("No server selected.")
            .style(Style::default().fg(Theme::MUTED))
            .alignment(Alignment::Center)
            .block(block);
        f.render_widget(no_server, area);
    }
}

pub(super) fn draw_systemd_tab<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    app: &DashboardApp,
    area: Rect,
) {
    let failed = app
        .get_selected_server()
        .and_then(|server| app.systemd_failed_cache.get(&server.id))
        .filter(|failed| !failed.is_empty());

    let Some(failed) = failed else {
        draw_systemd_units(f, app, area);
        return;
    };

    // Failed units come first: they are the reason to open this tab. The block
    // grows with the list but never takes more than half the panel, so the
    // configured units stay visible.
    let wanted = failed.len() as u16 + 3; // header + border
    let failed_height = wanted.min(area.height / 2).max(4);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(failed_height), Constraint::Min(3)])
        .split(area);

    draw_systemd_failed_units(f, failed, chunks[0]);
    draw_systemd_units(f, app, chunks[1]);
}

/// Render the host-wide list of units in the `failed` state.
fn draw_systemd_failed_units<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    failed: &[shared::types::SystemdFailedUnit],
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::ERROR))
        .title(Span::styled(
            format!(" 󰅙 Failed Units ({}) ", failed.len()),
            Style::default()
                .fg(Theme::ERROR)
                .add_modifier(Modifier::BOLD),
        ));

    let rows: Vec<Row> = failed
        .iter()
        .map(|u| {
            let since = u
                .since
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "N/A".to_string());

            Row::new(vec![
                Cell::from(Span::styled(
                    &u.name,
                    Style::default()
                        .fg(Theme::ERROR)
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    &u.description,
                    Style::default().fg(Theme::TEXT),
                )),
                Cell::from(Span::styled(since, Style::default().fg(Theme::MUTED))),
            ])
        })
        .collect();

    let table = Table::new(rows)
        .header(
            Row::new(vec!["Unit", "Description", "Since"]).style(
                Style::default()
                    .fg(Theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(block)
        .widths(&[
            Constraint::Percentage(35),
            Constraint::Percentage(45),
            Constraint::Percentage(20),
        ]);

    f.render_widget(table, area);
}

/// Render the units explicitly configured for monitoring.
fn draw_systemd_units<B: tui::backend::Backend>(f: &mut Frame<B>, app: &DashboardApp, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::BORDER))
        .title(Span::styled(
            " ⚙️ systemd Units ",
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));

    if let Some(server) = app.get_selected_server() {
        if let Some(units) = app.systemd_cache.get(&server.id) {
            if units.is_empty() {
                let no_units =
                    Paragraph::new("No systemd units configured or Linux host unavailable.")
                        .style(Style::default().fg(Theme::MUTED))
                        .alignment(Alignment::Center)
                        .block(block);
                f.render_widget(no_units, area);
                return;
            }

            let rows: Vec<Row> = units
                .iter()
                .map(|u| {
                    let (status_icon, status_style) = if u.is_active {
                        ("●", Style::default().fg(Theme::SUCCESS))
                    } else if u.status.contains("failed") {
                        ("●", Style::default().fg(Theme::ERROR))
                    } else {
                        ("○", Style::default().fg(Theme::MUTED))
                    };

                    let started = u
                        .started_at
                        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "N/A".to_string());

                    Row::new(vec![
                        Cell::from(Span::styled(&u.name, Style::default().fg(Theme::TEXT))),
                        Cell::from(Span::styled(
                            format!("{} {}", status_icon, u.status),
                            status_style,
                        )),
                        Cell::from(Span::styled(
                            u.pid
                                .map(|p| format!("{}", p))
                                .unwrap_or_else(|| "-".to_string()),
                            Style::default().fg(Theme::MUTED),
                        )),
                        Cell::from(Span::styled(
                            format_bytes(u.memory_current_bytes),
                            Style::default().fg(Theme::TEXT),
                        )),
                        Cell::from(Span::styled(started, Style::default().fg(Theme::MUTED))),
                    ])
                })
                .collect();

            let table = Table::new(rows)
                .header(
                    Row::new(vec!["Unit", "Status", "PID", "Memory", "Started"]).style(
                        Style::default()
                            .fg(Theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                )
                .block(block)
                .widths(&[
                    Constraint::Percentage(35),
                    Constraint::Percentage(25),
                    Constraint::Percentage(10),
                    Constraint::Percentage(15),
                    Constraint::Percentage(15),
                ])
                .highlight_style(
                    Style::default()
                        .bg(Theme::HIGHLIGHT_BG)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");

            let mut state = app.table_state.clone();
            f.render_stateful_widget(table, area, &mut state);
        } else {
            let not_connected = Paragraph::new("󰅛 Not connected. Press Enter to connect.")
                .style(Style::default().fg(Theme::MUTED))
                .alignment(Alignment::Center)
                .block(block);
            f.render_widget(not_connected, area);
        }
    } else {
        let no_server = Paragraph::new("No server selected.")
            .style(Style::default().fg(Theme::MUTED))
            .alignment(Alignment::Center)
            .block(block);
        f.render_widget(no_server, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use shared::notifications::NotificationConfig;
    use shared::types::*;
    use tui::backend::TestBackend;
    use tui::Terminal;
    use uuid::Uuid;

    fn create_test_app() -> DashboardApp {
        let server = ServerEndpoint {
            id: Uuid::new_v4(),
            name: "Test Server".to_string(),
            address: "127.0.0.1".to_string(),
            port: 50051,
            description: None,
            access_token: None,
        };
        let mut app = DashboardApp::new(vec![server.clone()], NotificationConfig::default(), None);
        app.servers = vec![server];
        app
    }

    fn create_test_system_info() -> SystemInfo {
        SystemInfo {
            hostname: "test-host".to_string(),
            os: "Linux".to_string(),
            kernel_version: "5.15.0".to_string(),
            uptime_seconds: 3661,
            cpu_count: 4,
            cpu_usage_percent: 42.5,
            memory_total_bytes: 16_000_000_000,
            memory_used_bytes: 8_000_000_000,
            memory_available_bytes: 8_000_000_000,
            disk_info: vec![
                DiskInfo {
                    device: "/dev/sda1".to_string(),
                    mount_point: "/".to_string(),
                    filesystem_type: "ext4".to_string(),
                    total_bytes: 500_000_000_000,
                    used_bytes: 250_000_000_000,
                    available_bytes: 250_000_000_000,
                    usage_percent: 50.0,
                },
                DiskInfo {
                    device: "/dev/sdb1".to_string(),
                    mount_point: "/data".to_string(),
                    filesystem_type: "xfs".to_string(),
                    total_bytes: 1_000_000_000_000,
                    used_bytes: 950_000_000_000,
                    available_bytes: 50_000_000_000,
                    usage_percent: 95.0,
                },
            ],
            timestamp: Utc::now(),
        }
    }

    fn render_app_to_buffer<F>(app: &DashboardApp, draw_fn: F) -> tui::buffer::Buffer
    where
        F: FnOnce(&mut Frame<TestBackend>, &DashboardApp, Rect),
    {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, 120, 40);
                draw_fn(f, app, area);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn buffer_contains(buffer: &tui::buffer::Buffer, text: &str) -> bool {
        let content: String = buffer.content.iter().map(|c| c.symbol.clone()).collect();
        content.contains(text)
    }

    // ─────────────────────────────────────────
    // Overview tab tests
    // ─────────────────────────────────────────

    #[test]
    fn test_draw_overview_no_servers() {
        let mut app = DashboardApp::new(vec![], NotificationConfig::default(), None);
        app.servers = vec![];
        let buffer = render_app_to_buffer(&app, draw_overview_tab);
        assert!(buffer_contains(&buffer, "No servers configured"));
    }

    #[test]
    fn test_draw_overview_not_connected() {
        let app = create_test_app();
        let buffer = render_app_to_buffer(&app, draw_overview_tab);
        assert!(buffer_contains(&buffer, "System Overview"));
        assert!(buffer_contains(&buffer, "Not connected"));
    }

    #[test]
    fn test_draw_overview_with_data() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        let info = create_test_system_info();
        app.system_info_cache.insert(server_id, info);
        app.cpu_history.insert(server_id, vec![10, 20, 30, 40, 50]);
        app.mem_history.insert(server_id, vec![40, 45, 50, 55, 60]);

        let buffer = render_app_to_buffer(&app, draw_overview_tab);
        assert!(buffer_contains(&buffer, "System Overview"));
        assert!(buffer_contains(&buffer, "test-host"));
        assert!(buffer_contains(&buffer, "Linux"));
        assert!(buffer_contains(&buffer, "Uptime:"));
        assert!(buffer_contains(&buffer, "CPUs: 4"));
        assert!(buffer_contains(&buffer, "CPU"));
        assert!(buffer_contains(&buffer, "Memory"));
        assert!(buffer_contains(&buffer, "Disks"));
        assert!(buffer_contains(&buffer, "/"));
        assert!(buffer_contains(&buffer, "/data"));
    }

    #[test]
    fn test_draw_overview_cpu_color_warning() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        let mut info = create_test_system_info();
        info.cpu_usage_percent = 65.0;
        app.system_info_cache.insert(server_id, info);
        // Should render without panic
        let _buffer = render_app_to_buffer(&app, draw_overview_tab);
    }

    #[test]
    fn test_draw_overview_cpu_color_error() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        let mut info = create_test_system_info();
        info.cpu_usage_percent = 90.0;
        app.system_info_cache.insert(server_id, info);
        let _buffer = render_app_to_buffer(&app, draw_overview_tab);
    }

    #[test]
    fn test_draw_overview_disk_high_usage() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        let mut info = create_test_system_info();
        info.disk_info[0].usage_percent = 95.0;
        app.system_info_cache.insert(server_id, info);
        let _buffer = render_app_to_buffer(&app, draw_overview_tab);
    }

    // ─────────────────────────────────────────
    // Services tab tests
    // ─────────────────────────────────────────

    #[test]
    fn test_draw_services_no_server() {
        let mut app = DashboardApp::new(vec![], NotificationConfig::default(), None);
        app.servers = vec![];
        let buffer = render_app_to_buffer(&app, draw_services_tab);
        assert!(buffer_contains(&buffer, "No server selected"));
    }

    #[test]
    fn test_draw_services_not_connected() {
        let app = create_test_app();
        let buffer = render_app_to_buffer(&app, draw_services_tab);
        assert!(buffer_contains(&buffer, "Services"));
        assert!(buffer_contains(&buffer, "Not connected"));
    }

    #[test]
    fn test_draw_services_with_data() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.services_cache.insert(
            server_id,
            vec![
                ServiceInfo {
                    name: "nginx".to_string(),
                    status: ServiceStatus::Running,
                    pid: Some(1234),
                    cpu_usage_percent: 5.5,
                    memory_usage_bytes: 50_000_000,
                    user: "www-data".to_string(),
                    uptime_seconds: Some(3600),
                },
                ServiceInfo {
                    name: "postgres".to_string(),
                    status: ServiceStatus::Stopped,
                    pid: None,
                    cpu_usage_percent: 0.0,
                    memory_usage_bytes: 0,
                    user: "postgres".to_string(),
                    uptime_seconds: None,
                },
                ServiceInfo {
                    name: "redis".to_string(),
                    status: ServiceStatus::Failed,
                    pid: None,
                    cpu_usage_percent: 0.0,
                    memory_usage_bytes: 0,
                    user: "redis".to_string(),
                    uptime_seconds: None,
                },
                ServiceInfo {
                    name: "unknown-svc".to_string(),
                    status: ServiceStatus::Unknown,
                    pid: None,
                    cpu_usage_percent: 0.0,
                    memory_usage_bytes: 0,
                    user: "root".to_string(),
                    uptime_seconds: None,
                },
            ],
        );
        let buffer = render_app_to_buffer(&app, draw_services_tab);
        assert!(buffer_contains(&buffer, "nginx"));
        assert!(buffer_contains(&buffer, "postgres"));
        assert!(buffer_contains(&buffer, "redis"));
        assert!(buffer_contains(&buffer, "unknown-svc"));
        assert!(buffer_contains(&buffer, "Running"));
        assert!(buffer_contains(&buffer, "Stopped"));
    }

    // ─────────────────────────────────────────
    // Network tab tests
    // ─────────────────────────────────────────

    #[test]
    fn test_draw_network_no_server() {
        let mut app = DashboardApp::new(vec![], NotificationConfig::default(), None);
        app.servers = vec![];
        let buffer = render_app_to_buffer(&app, draw_network_tab);
        assert!(buffer_contains(&buffer, "No server selected"));
    }

    #[test]
    fn test_draw_network_not_connected() {
        let app = create_test_app();
        let buffer = render_app_to_buffer(&app, draw_network_tab);
        assert!(buffer_contains(&buffer, "Network Interfaces"));
        assert!(buffer_contains(&buffer, "Not connected"));
    }

    #[test]
    fn test_draw_network_with_data() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.network_cache.insert(
            server_id,
            vec![
                NetworkInfo {
                    interface: "eth0".to_string(),
                    ip_address: "192.168.1.10".to_string(),
                    mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
                    is_up: true,
                    bytes_sent: 1_000_000,
                    bytes_received: 2_000_000,
                    packets_sent: 5000,
                    packets_received: 8000,
                },
                NetworkInfo {
                    interface: "eth1".to_string(),
                    ip_address: "10.0.0.5".to_string(),
                    mac_address: "11:22:33:44:55:66".to_string(),
                    is_up: false,
                    bytes_sent: 0,
                    bytes_received: 0,
                    packets_sent: 0,
                    packets_received: 0,
                },
            ],
        );
        let buffer = render_app_to_buffer(&app, draw_network_tab);
        assert!(buffer_contains(&buffer, "eth0"));
        assert!(buffer_contains(&buffer, "eth1"));
        assert!(buffer_contains(&buffer, "192.168.1.10"));
        assert!(buffer_contains(&buffer, "10.0.0.5"));
        assert!(buffer_contains(&buffer, "UP"));
        assert!(buffer_contains(&buffer, "DOWN"));
    }

    // ─────────────────────────────────────────
    // Containers tab tests
    // ─────────────────────────────────────────

    #[test]
    fn test_draw_containers_no_server() {
        let mut app = DashboardApp::new(vec![], NotificationConfig::default(), None);
        app.servers = vec![];
        let buffer = render_app_to_buffer(&app, draw_containers_tab);
        assert!(buffer_contains(&buffer, "No server selected"));
    }

    #[test]
    fn test_draw_containers_not_connected() {
        let app = create_test_app();
        let buffer = render_app_to_buffer(&app, draw_containers_tab);
        assert!(buffer_contains(&buffer, "Docker Containers"));
        assert!(buffer_contains(&buffer, "Not connected"));
    }

    #[test]
    fn test_draw_containers_empty() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.containers_cache.insert(server_id, vec![]);
        let buffer = render_app_to_buffer(&app, draw_containers_tab);
        assert!(buffer_contains(&buffer, "No containers found"));
    }

    #[test]
    fn test_draw_containers_with_data() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.containers_cache.insert(
            server_id,
            vec![
                ContainerInfo {
                    id: "abc123".to_string(),
                    name: "web-app".to_string(),
                    image: "nginx:latest".to_string(),
                    status: "Up 2 hours".to_string(),
                    state: "running".to_string(),
                    health: "healthy".to_string(),
                    cpu_percent: 10.5,
                    memory_usage_bytes: 100_000_000,
                    memory_limit_bytes: 500_000_000,
                    memory_percent: 20.0,
                    restart_count: 0,
                    network_rx_bytes: 1_000_000,
                    network_tx_bytes: 500_000,
                    networks: vec!["bridge".to_string()],
                },
                ContainerInfo {
                    id: "def456".to_string(),
                    name: "db".to_string(),
                    image: "postgres:15".to_string(),
                    status: "Paused".to_string(),
                    state: "paused".to_string(),
                    health: "".to_string(),
                    cpu_percent: 75.0,
                    memory_usage_bytes: 200_000_000,
                    memory_limit_bytes: 1_000_000_000,
                    memory_percent: 85.0,
                    restart_count: 1,
                    network_rx_bytes: 0,
                    network_tx_bytes: 0,
                    networks: vec![],
                },
                ContainerInfo {
                    id: "ghi789".to_string(),
                    name: "cache".to_string(),
                    image: "redis".to_string(),
                    status: "Exited (1)".to_string(),
                    state: "exited".to_string(),
                    health: "unhealthy".to_string(),
                    cpu_percent: 0.0,
                    memory_usage_bytes: 0,
                    memory_limit_bytes: 100_000_000,
                    memory_percent: 0.0,
                    restart_count: 3,
                    network_rx_bytes: 0,
                    network_tx_bytes: 0,
                    networks: vec![],
                },
            ],
        );
        let buffer = render_app_to_buffer(&app, draw_containers_tab);
        assert!(buffer_contains(&buffer, "web-app"));
        assert!(buffer_contains(&buffer, "db"));
        assert!(buffer_contains(&buffer, "cache"));
        assert!(buffer_contains(&buffer, "nginx"));
        assert!(buffer_contains(&buffer, "postgres"));
    }

    #[test]
    fn test_draw_containers_cpu_warning() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.containers_cache.insert(
            server_id,
            vec![ContainerInfo {
                id: "x".to_string(),
                name: "high-cpu".to_string(),
                image: "busybox".to_string(),
                status: "Up".to_string(),
                state: "running".to_string(),
                health: "".to_string(),
                cpu_percent: 85.0,
                memory_usage_bytes: 10_000_000,
                memory_limit_bytes: 100_000_000,
                memory_percent: 60.0,
                restart_count: 0,
                network_rx_bytes: 0,
                network_tx_bytes: 0,
                networks: vec![],
            }],
        );
        let _buffer = render_app_to_buffer(&app, draw_containers_tab);
    }

    // ─────────────────────────────────────────
    // Systemd tab tests
    // ─────────────────────────────────────────

    #[test]
    fn test_draw_systemd_no_server() {
        let mut app = DashboardApp::new(vec![], NotificationConfig::default(), None);
        app.servers = vec![];
        let buffer = render_app_to_buffer(&app, draw_systemd_tab);
        assert!(buffer_contains(&buffer, "No server selected"));
    }

    #[test]
    fn test_draw_systemd_not_connected() {
        let app = create_test_app();
        let buffer = render_app_to_buffer(&app, draw_systemd_tab);
        assert!(buffer_contains(&buffer, "systemd Units"));
        assert!(buffer_contains(&buffer, "Not connected"));
    }

    #[test]
    fn test_draw_systemd_empty() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.systemd_cache.insert(server_id, vec![]);
        let buffer = render_app_to_buffer(&app, draw_systemd_tab);
        assert!(buffer_contains(&buffer, "No systemd units configured"));
    }

    #[test]
    fn test_draw_systemd_with_data() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.systemd_cache.insert(
            server_id,
            vec![
                SystemdUnitInfo {
                    name: "nginx.service".to_string(),
                    status: "active (running)".to_string(),
                    is_active: true,
                    pid: Some(1234),
                    memory_current_bytes: 50_000_000,
                    started_at: Some(Utc::now()),
                },
                SystemdUnitInfo {
                    name: "postgres.service".to_string(),
                    status: "failed".to_string(),
                    is_active: false,
                    pid: None,
                    memory_current_bytes: 0,
                    started_at: None,
                },
                SystemdUnitInfo {
                    name: "cron.service".to_string(),
                    status: "inactive (dead)".to_string(),
                    is_active: false,
                    pid: None,
                    memory_current_bytes: 0,
                    started_at: None,
                },
            ],
        );
        let buffer = render_app_to_buffer(&app, draw_systemd_tab);
        assert!(buffer_contains(&buffer, "nginx.service"));
        assert!(buffer_contains(&buffer, "postgres.service"));
        assert!(buffer_contains(&buffer, "cron.service"));
        assert!(buffer_contains(&buffer, "active (running)"));
        assert!(buffer_contains(&buffer, "failed"));
    }

    fn failed_unit(name: &str, description: &str) -> SystemdFailedUnit {
        SystemdFailedUnit {
            name: name.to_string(),
            description: description.to_string(),
            since: None,
        }
    }

    #[test]
    fn test_draw_systemd_lists_failed_units() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.systemd_cache.insert(server_id, vec![]);
        app.systemd_failed_cache.insert(
            server_id,
            vec![
                failed_unit("certbot.service", "Certbot"),
                failed_unit("cloud-init.service", "Initial cloud-init job"),
            ],
        );

        let buffer = render_app_to_buffer(&app, draw_systemd_tab);
        assert!(buffer_contains(&buffer, "Failed Units (2)"));
        assert!(buffer_contains(&buffer, "certbot.service"));
        assert!(buffer_contains(&buffer, "cloud-init.service"));
        // The configured-units panel must stay visible below
        assert!(buffer_contains(&buffer, "No systemd units configured"));
    }

    #[test]
    fn test_draw_systemd_failed_block_absent_when_healthy() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.systemd_cache.insert(server_id, vec![]);
        app.systemd_failed_cache.insert(server_id, vec![]);

        let buffer = render_app_to_buffer(&app, draw_systemd_tab);
        assert!(
            !buffer_contains(&buffer, "Failed Units"),
            "a healthy host must not show the failed block at all"
        );
    }

    #[test]
    fn test_draw_systemd_failed_shows_since_when_known() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        let since = Utc::now();
        app.systemd_cache.insert(server_id, vec![]);
        app.systemd_failed_cache.insert(
            server_id,
            vec![SystemdFailedUnit {
                name: "certbot.service".to_string(),
                description: "Certbot".to_string(),
                since: Some(since),
            }],
        );

        let buffer = render_app_to_buffer(&app, draw_systemd_tab);
        assert!(buffer_contains(
            &buffer,
            &since.format("%Y-%m-%d").to_string()
        ));
    }

    #[test]
    fn test_overview_reports_failed_unit_count() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.system_info_cache
            .insert(server_id, create_test_system_info());
        app.systemd_failed_cache.insert(
            server_id,
            vec![
                failed_unit("certbot.service", "Certbot"),
                failed_unit("networking.service", "Raise network interfaces"),
            ],
        );

        let buffer = render_app_to_buffer(&app, draw_overview_tab);
        assert!(buffer_contains(&buffer, "2 systemd units failed"));
        assert!(buffer_contains(&buffer, "certbot.service"));
    }

    #[test]
    fn test_overview_singular_wording_for_one_failed_unit() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.system_info_cache
            .insert(server_id, create_test_system_info());
        app.systemd_failed_cache
            .insert(server_id, vec![failed_unit("certbot.service", "Certbot")]);

        let buffer = render_app_to_buffer(&app, draw_overview_tab);
        assert!(buffer_contains(&buffer, "1 systemd unit failed"));
    }

    #[test]
    fn test_overview_omits_failed_line_when_healthy() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.system_info_cache
            .insert(server_id, create_test_system_info());
        app.systemd_failed_cache.insert(server_id, vec![]);

        let buffer = render_app_to_buffer(&app, draw_overview_tab);
        assert!(!buffer_contains(&buffer, "systemd unit"));
    }

    #[test]
    fn test_summarize_failed_units_lists_up_to_three() {
        let units = vec![
            failed_unit("a.service", ""),
            failed_unit("b.service", ""),
            failed_unit("c.service", ""),
        ];
        assert_eq!(
            summarize_failed_units(&units),
            "a.service, b.service, c.service"
        );
    }

    #[test]
    fn test_summarize_failed_units_truncates_long_lists() {
        let units: Vec<SystemdFailedUnit> = (0..7)
            .map(|i| failed_unit(&format!("unit{}.service", i), ""))
            .collect();
        assert_eq!(
            summarize_failed_units(&units),
            "unit0.service, unit1.service, unit2.service +4 more"
        );
    }

    #[test]
    fn test_summarize_failed_units_empty() {
        assert_eq!(summarize_failed_units(&[]), "");
    }

    #[test]
    fn test_draw_systemd_started_at_format() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        let started = Utc::now();
        app.systemd_cache.insert(
            server_id,
            vec![SystemdUnitInfo {
                name: "app.service".to_string(),
                status: "active".to_string(),
                is_active: true,
                pid: Some(1),
                memory_current_bytes: 10_000,
                started_at: Some(started),
            }],
        );
        let buffer = render_app_to_buffer(&app, draw_systemd_tab);
        // Verify the date is rendered by checking year and month separately
        // to avoid issues with line wrapping or spacing in the buffer
        let year = started.format("%Y").to_string();
        let month = started.format("%m").to_string();
        assert!(buffer_contains(&buffer, &year));
        assert!(buffer_contains(&buffer, &month));
    }
}
