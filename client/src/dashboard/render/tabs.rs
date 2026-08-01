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

/// Format a container's swap, marking the ones a `swapoff` would kill.
///
/// This is the pre-flight the fleet needed: SonarQube was capped at 2000M and
/// holding ~1.1G in swap, so clearing swap brought it straight past its cgroup
/// limit and the kernel killed it. Every number needed to predict that was
/// already in /proc; nothing was reading it.
fn format_container_swap(container: &shared::types::ContainerInfo) -> String {
    match container.swap_bytes {
        None => "?".to_string(),
        Some(0) => "—".to_string(),
        Some(swap) if container.would_exceed_limit_on_swapoff() => {
            format!("{} ⚠", format_bytes(swap))
        }
        Some(swap) => format_bytes(swap),
    }
}

/// Paging rate above which a host is treated as thrashing.
///
/// Sustained paging at this level means the working set does not fit; brief
/// spikes during a backup or a deploy do not.
const THRASHING_PAGES_PER_SEC: f64 = 200.0;

/// Share of the last minute with every task stalled that counts as pressure.
///
/// PSI reports a percentage, so this is 5% of wall-clock time spent with
/// nothing able to run.
const PSI_FULL_WARNING_PERCENT: f64 = 5.0;

/// Describe swap in the terms that matter: how much is moving, not how much is
/// parked.
///
/// Occupancy is reported without any alerting colour on purpose. It is a
/// high-water mark from some past spike, and colouring it is what makes
/// dashboards flag the healthy host and ignore the thrashing one.
fn format_swap_line(swap: &shared::types::SwapInfo) -> Vec<(String, Style)> {
    let mut parts = Vec::new();

    parts.push((
        format!(
            "Swap: {} / {} ({:.0}%)",
            format_bytes(swap.used_bytes),
            format_bytes(swap.total_bytes),
            swap.used_percent()
        ),
        Style::default().fg(Theme::MUTED),
    ));

    let activity = swap.activity_pages_per_sec();
    let activity_style = if activity > THRASHING_PAGES_PER_SEC {
        Style::default()
            .fg(Theme::ERROR)
            .add_modifier(Modifier::BOLD)
    } else if activity > 0.0 {
        Style::default().fg(Theme::WARNING)
    } else {
        Style::default().fg(Theme::SUCCESS)
    };

    parts.push((
        format!(
            "paging: {:.0}/s in, {:.0}/s out",
            swap.in_pages_per_sec, swap.out_pages_per_sec
        ),
        activity_style,
    ));

    parts
}

/// Describe memory pressure, when the kernel reports it.
fn format_pressure_line(pressure: &shared::types::MemoryPressure) -> (String, Style) {
    let style = if pressure.full_avg60 > PSI_FULL_WARNING_PERCENT {
        Style::default()
            .fg(Theme::ERROR)
            .add_modifier(Modifier::BOLD)
    } else if pressure.full_avg60 > 0.0 {
        Style::default().fg(Theme::WARNING)
    } else {
        Style::default().fg(Theme::MUTED)
    };

    (
        format!(
            "PSI some {:.2}% / full {:.2}% (stalled {} total)",
            pressure.some_avg60,
            pressure.full_avg60,
            format_uptime(pressure.full_total_seconds as u64)
        ),
        style,
    )
}

/// Restarts beyond which a container is shown as crash-looping rather than
/// merely restarted.
///
/// A long-lived container legitimately accumulates a few restarts across
/// reboots and deploys; hundreds mean it never stays up.
const CRASH_LOOP_RESTARTS: u32 = 100;

/// Format a container's restart count, abbreviating the large ones.
///
/// The fleet's worst offender sat at 412,813 restarts — a figure that has to
/// fit in a narrow column without pushing the table around.
fn format_restart_count(restarts: u32) -> String {
    // The bounds stop just short of the next unit so rounding cannot produce a
    // four-digit mantissa such as "1000.0k".
    match restarts {
        0 => "—".to_string(),
        n if n < 1_000 => n.to_string(),
        n if n < 999_950 => format!("{:.1}k", n as f64 / 1_000.0),
        n if n < 999_950_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
        n => format!("{:.1}B", n as f64 / 1_000_000_000.0),
    }
}

/// Icon and style for a container's health.
///
/// A container whose healthcheck never worked is shown differently from one
/// that started failing: with 21 permanently red containers on a host, a real
/// failure is invisible.
fn container_health_icon(
    container: &shared::types::ContainerInfo,
) -> Option<(&'static str, Style)> {
    use shared::types::HealthAssessment;

    if let Some(detail) = &container.health_detail {
        return match detail.assess() {
            HealthAssessment::Passing => Some((" 󰄬", Style::default().fg(Theme::SUCCESS))),
            HealthAssessment::FailingNow => Some((" 󰅙", Style::default().fg(Theme::ERROR))),
            // Muted on purpose: a broken check is maintenance, not an incident
            HealthAssessment::BrokenCheck => Some((" 󰋼", Style::default().fg(Theme::MUTED))),
        };
    }

    match container.health.as_str() {
        "healthy" => Some((" 󰄬", Style::default().fg(Theme::SUCCESS))),
        "unhealthy" => Some((" 󰅙", Style::default().fg(Theme::ERROR))),
        _ => None,
    }
}

/// Summarise what the container fleet on a host is missing.
///
/// Returns `None` when there is nothing to report, so a well-configured host
/// gets no banner at all.
fn summarize_container_risks(containers: &[shared::types::ContainerInfo]) -> Option<String> {
    use shared::types::HealthAssessment;

    let unlimited = containers.iter().filter(|c| !c.memory_limit_set).count();
    let broken_checks = containers
        .iter()
        .filter_map(|c| c.health_detail.as_ref())
        .filter(|h| h.assess() == HealthAssessment::BrokenCheck)
        .count();
    let crash_looping = containers
        .iter()
        .filter(|c| c.restart_count >= CRASH_LOOP_RESTARTS)
        .count();
    let swapoff_risk = containers
        .iter()
        .filter(|c| c.would_exceed_limit_on_swapoff())
        .count();

    let mut parts = Vec::new();
    if unlimited > 0 {
        parts.push(format!(
            "{}/{} without mem_limit",
            unlimited,
            containers.len()
        ));
    }
    if broken_checks > 0 {
        parts.push(format!("{} broken healthcheck(s)", broken_checks));
    }
    if crash_looping > 0 {
        parts.push(format!("{} crash-looping", crash_looping));
    }
    if swapoff_risk > 0 {
        parts.push(format!("{} would OOM on swapoff", swapoff_risk));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("  │  "))
    }
}

/// Format a container's memory usage against its limit.
///
/// A container with no limit has no meaningful percentage: Docker reports the
/// host's total RAM as the limit, so the arithmetic yields a reassuring number
/// for precisely the container nobody is protecting. Saying "no limit" is the
/// honest answer.
fn format_container_memory_percent(percent: Option<f64>) -> String {
    match percent {
        Some(p) => format!("{:>5.1}%", p),
        None => "no limit".to_string(),
    }
}

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
            // Optional lines cost no vertical space when they have nothing to
            // say, so a healthy host keeps a compact header.
            let has_swap = info.swap.total_bytes > 0 || info.memory_pressure.is_some();
            let header_height = 2 + u16::from(!failed_units.is_empty()) + u16::from(has_swap) + 1;

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

            if has_swap {
                let mut swap_spans =
                    vec![Span::styled("󰓡 ", Style::default().fg(Theme::MEM_COLOR))];
                for (index, (text, style)) in format_swap_line(&info.swap).into_iter().enumerate() {
                    if index > 0 {
                        swap_spans.push(Span::styled("  │  ", Style::default().fg(Theme::BORDER)));
                    }
                    swap_spans.push(Span::styled(text, style));
                }

                if let Some(pressure) = &info.memory_pressure {
                    let (text, style) = format_pressure_line(pressure);
                    swap_spans.push(Span::styled("  │  ", Style::default().fg(Theme::BORDER)));
                    swap_spans.push(Span::styled(text, style));
                }

                header_lines.push(Spans::from(swap_spans));
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

/// Label a bind scope in the terms an operator reasons about.
fn bind_scope_label(scope: shared::types::BindScope) -> (&'static str, Style) {
    use shared::types::BindScope;

    match scope {
        BindScope::AllInterfaces => (
            "all interfaces",
            Style::default()
                .fg(Theme::ERROR)
                .add_modifier(Modifier::BOLD),
        ),
        BindScope::Public => (
            "public",
            Style::default()
                .fg(Theme::ERROR)
                .add_modifier(Modifier::BOLD),
        ),
        BindScope::Private => ("private", Style::default().fg(Theme::SUCCESS)),
        BindScope::Loopback => ("loopback", Style::default().fg(Theme::SUCCESS)),
        BindScope::Unknown => ("unknown", Style::default().fg(Theme::MUTED)),
    }
}

/// Count the exposed data stores, which is the headline finding.
fn count_exposed_datastores(ports: &[shared::types::ListeningPortInfo]) -> usize {
    ports.iter().filter(|p| p.is_exposed_datastore()).count()
}

/// Render the listening sockets and how widely each is bound.
fn draw_listening_ports<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    ports: &[shared::types::ListeningPortInfo],
    area: Rect,
) {
    let exposed = count_exposed_datastores(ports);
    let (title, title_style) = if exposed > 0 {
        (
            format!(" 󰦯 Listening Ports — {} exposed data store(s) ", exposed),
            Style::default()
                .fg(Theme::ERROR)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            format!(" 󰦯 Listening Ports ({}) ", ports.len()),
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if exposed > 0 {
            Theme::ERROR
        } else {
            Theme::BORDER
        }))
        .title(Span::styled(title, title_style));

    let rows: Vec<Row> = ports
        .iter()
        .map(|p| {
            let (scope_text, scope_style) = bind_scope_label(p.bind_scope);
            let port_style = if p.is_exposed_datastore() {
                Style::default()
                    .fg(Theme::ERROR)
                    .add_modifier(Modifier::BOLD)
            } else if p.sensitive {
                Style::default().fg(Theme::WARNING)
            } else {
                Style::default().fg(Theme::TEXT)
            };

            Row::new(vec![
                Cell::from(Span::styled(p.port.to_string(), port_style)),
                Cell::from(Span::styled(&p.address, Style::default().fg(Theme::TEXT))),
                Cell::from(Span::styled(scope_text, scope_style)),
                Cell::from(Span::styled(&p.protocol, Style::default().fg(Theme::MUTED))),
                Cell::from(Span::styled(
                    if p.process_name.is_empty() {
                        "—".to_string()
                    } else {
                        p.process_name.clone()
                    },
                    Style::default().fg(Theme::TEXT),
                )),
                Cell::from(Span::styled(
                    p.pid
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "—".to_string()),
                    Style::default().fg(Theme::MUTED),
                )),
            ])
        })
        .collect();

    let table = Table::new(rows)
        .header(
            Row::new(vec!["Port", "Address", "Scope", "Proto", "Process", "PID"]).style(
                Style::default()
                    .fg(Theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(block)
        .widths(&[
            Constraint::Percentage(10),
            Constraint::Percentage(22),
            Constraint::Percentage(18),
            Constraint::Percentage(10),
            Constraint::Percentage(22),
            Constraint::Percentage(10),
        ]);

    f.render_widget(table, area);
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
            // Listening sockets share the tab: an interface is only half the
            // story, and what is bound to it is the half that matters.
            let ports = app
                .ports_cache
                .get(&server.id)
                .map(|p| p.as_slice())
                .unwrap_or(&[]);
            let (interfaces_area, ports_area) = if ports.is_empty() {
                (area, None)
            } else {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
                    .split(area);
                (chunks[0], Some(chunks[1]))
            };

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
            f.render_stateful_widget(table, interfaces_area, &mut state);

            if let Some(ports_area) = ports_area {
                draw_listening_ports(f, ports, ports_area);
            }
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

                    let health_icon = container_health_icon(c);

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
                            format_container_memory_percent(c.memory_percent),
                            match c.memory_percent {
                                // No limit is not a comfortable 8% — it is a
                                // container that can take the host down.
                                None => Style::default().fg(Theme::WARNING),
                                Some(p) if p > 80.0 => Style::default().fg(Theme::ERROR),
                                Some(p) if p > 50.0 => Style::default().fg(Theme::WARNING),
                                Some(_) => Style::default().fg(Theme::TEXT),
                            },
                        )),
                        Cell::from(Span::styled(
                            format_restart_count(c.restart_count),
                            if c.restart_count >= CRASH_LOOP_RESTARTS {
                                Style::default().fg(Theme::ERROR)
                            } else if c.restart_count > 0 {
                                Style::default().fg(Theme::WARNING)
                            } else {
                                Style::default().fg(Theme::MUTED)
                            },
                        )),
                        Cell::from(Span::styled(
                            format_container_swap(c),
                            if c.would_exceed_limit_on_swapoff() {
                                // Clearing swap would push it past its limit
                                Style::default()
                                    .fg(Theme::ERROR)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(Theme::MUTED)
                            },
                        )),
                    ])
                })
                .collect();

            let table = Table::new(rows)
                .header(
                    Row::new(vec![
                        "Name", "Image", "Status", "CPU", "Memory", "MEM %", "Restarts", "Swap",
                    ])
                    .style(
                        Style::default()
                            .fg(Theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                )
                .block(block)
                // Sums to 94%: the table inserts a space between columns, and
                // claiming the full width would push the last one off screen
                .widths(&[
                    Constraint::Percentage(17),
                    Constraint::Percentage(15),
                    Constraint::Percentage(14),
                    Constraint::Percentage(7),
                    Constraint::Percentage(14),
                    Constraint::Percentage(9),
                    Constraint::Percentage(8),
                    Constraint::Percentage(10),
                ])
                .highlight_style(
                    Style::default()
                        .bg(Theme::HIGHLIGHT_BG)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");

            let mut state = app.table_state.clone();

            // A summary line, only when there is something to say
            match summarize_container_risks(containers) {
                Some(summary) => {
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(3), Constraint::Length(1)])
                        .split(area);

                    f.render_stateful_widget(table, chunks[0], &mut state);

                    let banner = Paragraph::new(Spans::from(vec![
                        Span::styled(" 󰀦 ", Style::default().fg(Theme::WARNING)),
                        Span::styled(summary, Style::default().fg(Theme::WARNING)),
                    ]));
                    f.render_widget(banner, chunks[1]);
                }
                None => f.render_stateful_widget(table, area, &mut state),
            }
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

/// How a certificate's remaining validity should be shown.
///
/// Let's Encrypt renews at 30 days, so 21 means renewal has already had a week
/// and has not worked.
fn certificate_style(days_until_expiry: i64) -> Style {
    if days_until_expiry <= 7 {
        Style::default()
            .fg(Theme::ERROR)
            .add_modifier(Modifier::BOLD)
    } else if days_until_expiry <= 21 {
        Style::default().fg(Theme::WARNING)
    } else {
        Style::default().fg(Theme::SUCCESS)
    }
}

/// Render remaining validity, saying plainly when it is already gone.
fn format_days_remaining(days: i64) -> String {
    match days {
        d if d < 0 => format!("expired {}d ago", -d),
        0 => "today".to_string(),
        d => format!("{}d", d),
    }
}

/// Describe the renewal unit, which is the leading indicator: certificates all
/// look fine right until a whole batch expires together.
fn format_renewal_status(name: Option<&str>, status: Option<&str>) -> (String, Style) {
    match (name, status) {
        (Some(unit), Some(state)) if state.contains("failed") => (
            format!("{} is {} — renewal is broken", unit, state),
            Style::default()
                .fg(Theme::ERROR)
                .add_modifier(Modifier::BOLD),
        ),
        (Some(unit), Some(state)) => (
            format!("{}: {}", unit, state),
            Style::default().fg(Theme::SUCCESS),
        ),
        _ => (
            "no renewal unit detected".to_string(),
            Style::default().fg(Theme::MUTED),
        ),
    }
}

pub(super) fn draw_tls_tab<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    app: &DashboardApp,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::BORDER))
        .title(Span::styled(
            " 󰌾 TLS Certificates ",
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));

    let Some(server) = app.get_selected_server() else {
        let no_server = Paragraph::new("No server selected.")
            .style(Style::default().fg(Theme::MUTED))
            .alignment(Alignment::Center)
            .block(block);
        f.render_widget(no_server, area);
        return;
    };

    let Some(snapshot) = app.tls_cache.get(&server.id) else {
        let not_connected = Paragraph::new("󰅛 Not connected. Press Enter to connect.")
            .style(Style::default().fg(Theme::MUTED))
            .alignment(Alignment::Center)
            .block(block);
        f.render_widget(not_connected, area);
        return;
    };

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(inner);

    let (renewal_text, renewal_style) = format_renewal_status(
        snapshot.renewal_unit_name.as_deref(),
        snapshot.renewal_unit_status.as_deref(),
    );
    f.render_widget(
        Paragraph::new(Spans::from(vec![
            Span::styled(" 󰑐 ", renewal_style),
            Span::styled(renewal_text, renewal_style),
        ])),
        chunks[0],
    );

    if snapshot.certificates.is_empty() {
        let empty = Paragraph::new("No certificates found on this host.")
            .style(Style::default().fg(Theme::MUTED))
            .alignment(Alignment::Center);
        f.render_widget(empty, chunks[1]);
        return;
    }

    let rows: Vec<Row> = snapshot
        .certificates
        .iter()
        .map(|c| {
            let style = certificate_style(c.days_until_expiry);
            Row::new(vec![
                Cell::from(Span::styled(&c.name, Style::default().fg(Theme::TEXT))),
                Cell::from(Span::styled(
                    format_days_remaining(c.days_until_expiry),
                    style,
                )),
                Cell::from(Span::styled(
                    c.not_after
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    Style::default().fg(Theme::MUTED),
                )),
                Cell::from(Span::styled(
                    c.domains.len().to_string(),
                    Style::default().fg(Theme::TEXT),
                )),
                Cell::from(Span::styled(&c.issuer, Style::default().fg(Theme::MUTED))),
            ])
        })
        .collect();

    let table = Table::new(rows)
        .header(
            Row::new(vec![
                "Certificate",
                "Remaining",
                "Expires",
                "SANs",
                "Issuer",
            ])
            .style(
                Style::default()
                    .fg(Theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .widths(&[
            Constraint::Percentage(30),
            Constraint::Percentage(16),
            Constraint::Percentage(14),
            Constraint::Percentage(8),
            Constraint::Percentage(26),
        ])
        .highlight_style(
            Style::default()
                .bg(Theme::HIGHLIGHT_BG)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    let mut state = app.table_state.clone();
    f.render_stateful_widget(table, chunks[1], &mut state);
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
            swap: SwapInfo::default(),
            memory_pressure: None,
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
                    memory_percent: Some(20.0),
                    restart_count: 0,
                    network_rx_bytes: 1_000_000,
                    network_tx_bytes: 500_000,
                    networks: vec!["bridge".to_string()],
                    memory_limit_set: false,
                    health_detail: None,
                    swap_bytes: None,
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
                    memory_percent: Some(85.0),
                    restart_count: 1,
                    network_rx_bytes: 0,
                    network_tx_bytes: 0,
                    networks: vec![],
                    memory_limit_set: false,
                    health_detail: None,
                    swap_bytes: None,
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
                    memory_percent: Some(0.0),
                    restart_count: 3,
                    network_rx_bytes: 0,
                    network_tx_bytes: 0,
                    networks: vec![],
                    memory_limit_set: false,
                    health_detail: None,
                    swap_bytes: None,
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
                memory_percent: Some(60.0),
                restart_count: 0,
                network_rx_bytes: 0,
                network_tx_bytes: 0,
                networks: vec![],
                memory_limit_set: false,
                health_detail: None,
                swap_bytes: None,
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

    // ─────────────────────────────────────────
    // Swap and memory pressure
    // ─────────────────────────────────────────

    fn swap(used: u64, total: u64, rate_in: f64, rate_out: f64) -> SwapInfo {
        SwapInfo {
            total_bytes: total,
            used_bytes: used,
            in_pages_per_sec: rate_in,
            out_pages_per_sec: rate_out,
        }
    }

    #[test]
    fn test_swap_line_shows_occupancy_without_alerting_on_it() {
        // alemanha8: 65% occupied, paging nothing. The occupancy must be
        // visible and must not be styled as a problem.
        let parts = format_swap_line(&swap(5_200_000_000, 8_000_000_000, 0.0, 0.0));

        assert!(parts[0].0.contains("65%"));
        assert_eq!(
            parts[0].1,
            Style::default().fg(Theme::MUTED),
            "occupancy is a high-water mark, not a warning"
        );
    }

    #[test]
    fn test_swap_line_flags_active_paging() {
        let idle = format_swap_line(&swap(5_200_000_000, 8_000_000_000, 0.0, 0.0));
        let thrashing = format_swap_line(&swap(4_400_000_000, 8_000_000_000, 180.0, 220.0));

        assert_ne!(
            idle[1].1, thrashing[1].1,
            "a host that is paging must not look like one that is not"
        );
        assert!(thrashing[1].0.contains("180"));
        assert!(thrashing[1].0.contains("220"));
    }

    #[test]
    fn test_swap_line_distinguishes_light_paging_from_thrashing() {
        let light = format_swap_line(&swap(1_000, 8_000_000_000, 5.0, 5.0));
        let heavy = format_swap_line(&swap(1_000, 8_000_000_000, 300.0, 300.0));
        assert_ne!(light[1].1, heavy[1].1);
    }

    #[test]
    fn test_pressure_line_reports_stall_time() {
        let (text, _) = format_pressure_line(&MemoryPressure {
            some_avg60: 12.5,
            full_avg60: 7.25,
            full_total_seconds: 61_200.0, // 17 hours, the alemanha6 figure
        });

        assert!(text.contains("12.50%"));
        assert!(text.contains("7.25%"));
        assert!(
            text.contains("17h"),
            "stall total should read as time: {}",
            text
        );
    }

    #[test]
    fn test_pressure_line_styles_escalate() {
        let quiet = format_pressure_line(&MemoryPressure {
            some_avg60: 0.0,
            full_avg60: 0.0,
            full_total_seconds: 76.0,
        });
        let stalling = format_pressure_line(&MemoryPressure {
            some_avg60: 40.0,
            full_avg60: 20.0,
            full_total_seconds: 61_200.0,
        });
        assert_ne!(quiet.1, stalling.1);
    }

    #[test]
    fn test_overview_shows_swap_and_pressure() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        let mut info = create_test_system_info();
        info.swap = swap(4_400_000_000, 8_000_000_000, 180.0, 220.0);
        info.memory_pressure = Some(MemoryPressure {
            some_avg60: 12.5,
            full_avg60: 7.25,
            full_total_seconds: 61_200.0,
        });
        app.system_info_cache.insert(server_id, info);

        let buffer = render_app_to_buffer(&app, draw_overview_tab);
        assert!(buffer_contains(&buffer, "paging:"));
        assert!(buffer_contains(&buffer, "PSI"));
    }

    #[test]
    fn test_overview_omits_swap_line_without_swap() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        let mut info = create_test_system_info();
        info.swap = SwapInfo::default();
        info.memory_pressure = None;
        app.system_info_cache.insert(server_id, info);

        let buffer = render_app_to_buffer(&app, draw_overview_tab);
        assert!(!buffer_contains(&buffer, "paging:"));
    }

    // ─────────────────────────────────────────
    // Container swapoff pre-flight
    // ─────────────────────────────────────────

    #[test]
    fn test_container_swap_flags_swapoff_risk() {
        // The SonarQube incident: 2000M limit, 1.4G resident, 1.1G in swap
        let mut container = container_fixture("sonarqube");
        container.memory_limit_bytes = 2_000_000_000;
        container.memory_usage_bytes = 1_400_000_000;
        container.swap_bytes = Some(1_100_000_000);

        assert!(container.would_exceed_limit_on_swapoff());
        assert!(format_container_swap(&container).contains('⚠'));
    }

    #[test]
    fn test_container_swap_without_risk() {
        let mut container = container_fixture("app");
        container.swap_bytes = Some(1_000_000);
        assert!(!format_container_swap(&container).contains('⚠'));
    }

    #[test]
    fn test_container_swap_unknown_is_not_zero() {
        let mut container = container_fixture("app");
        container.swap_bytes = None;
        assert_eq!(format_container_swap(&container), "?");

        container.swap_bytes = Some(0);
        assert_eq!(format_container_swap(&container), "—");
    }

    #[test]
    fn test_risk_summary_counts_swapoff_victims() {
        let mut container = container_fixture("sonarqube");
        container.memory_limit_bytes = 2_000_000_000;
        container.memory_usage_bytes = 1_400_000_000;
        container.swap_bytes = Some(1_100_000_000);

        let summary = summarize_container_risks(&[container]).unwrap();
        assert!(summary.contains("1 would OOM on swapoff"));
    }

    // ─────────────────────────────────────────
    // Listening ports
    // ─────────────────────────────────────────

    fn listening_port(port: u16, address: &str, process: &str) -> ListeningPortInfo {
        ListeningPortInfo {
            address: address.to_string(),
            port,
            protocol: "tcp".to_string(),
            bind_scope: BindScope::classify(address),
            pid: Some(4242),
            process_name: process.to_string(),
            sensitive: ListeningPortInfo::is_sensitive_port(port),
        }
    }

    #[test]
    fn test_bind_scope_labels_are_distinct() {
        let exposed = bind_scope_label(BindScope::AllInterfaces);
        let private = bind_scope_label(BindScope::Private);
        let loopback = bind_scope_label(BindScope::Loopback);

        assert_eq!(exposed.0, "all interfaces");
        assert_ne!(exposed.1, private.1, "exposure must not look like safety");
        assert_eq!(private.1, loopback.1);
    }

    #[test]
    fn test_public_bind_is_styled_like_a_wildcard_bind() {
        // Both reach beyond the host, so both must read as exposure
        assert_eq!(
            bind_scope_label(BindScope::Public).1,
            bind_scope_label(BindScope::AllInterfaces).1
        );
    }

    #[test]
    fn test_count_exposed_datastores() {
        let ports = vec![
            listening_port(5432, "0.0.0.0", "postgres"),
            listening_port(5432, "127.0.0.1", "postgres"),
            listening_port(443, "0.0.0.0", "nginx"),
            listening_port(6379, "0.0.0.0", "redis-server"),
        ];

        assert_eq!(
            count_exposed_datastores(&ports),
            2,
            "only the exposed data stores count, not every exposed port"
        );
    }

    #[test]
    fn test_network_tab_lists_listening_ports() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.network_cache.insert(server_id, vec![]);
        app.ports_cache.insert(
            server_id,
            vec![
                listening_port(5432, "0.0.0.0", "postgres"),
                listening_port(53, "127.0.0.1", "systemd-resolve"),
            ],
        );

        let buffer = render_app_to_buffer(&app, draw_network_tab);
        assert!(buffer_contains(&buffer, "5432"));
        assert!(buffer_contains(&buffer, "all interfaces"));
        assert!(buffer_contains(&buffer, "postgres"));
        assert!(buffer_contains(&buffer, "exposed data store"));
    }

    #[test]
    fn test_network_tab_without_exposure_has_no_warning() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.network_cache.insert(server_id, vec![]);
        app.ports_cache.insert(
            server_id,
            vec![listening_port(5432, "127.0.0.1", "postgres")],
        );

        let buffer = render_app_to_buffer(&app, draw_network_tab);
        assert!(!buffer_contains(&buffer, "exposed data store"));
        assert!(buffer_contains(&buffer, "loopback"));
    }

    #[test]
    fn test_network_tab_shows_dash_for_unknown_process() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        let mut port = listening_port(9200, "0.0.0.0", "");
        port.pid = None;
        app.network_cache.insert(server_id, vec![]);
        app.ports_cache.insert(server_id, vec![port]);

        // A port owned by another user is still reported, just unattributed
        let buffer = render_app_to_buffer(&app, draw_network_tab);
        assert!(buffer_contains(&buffer, "9200"));
    }

    // ─────────────────────────────────────────
    // TLS tab
    // ─────────────────────────────────────────

    fn certificate(name: &str, days: i64) -> TlsCertificateInfo {
        TlsCertificateInfo {
            name: name.to_string(),
            domains: vec![name.to_string(), format!("www.{}", name)],
            issuer: "R3".to_string(),
            not_after: Some(Utc::now() + chrono::Duration::days(days)),
            days_until_expiry: days,
            source: format!("/etc/letsencrypt/live/{}/cert.pem", name),
            orphaned: false,
        }
    }

    #[test]
    fn test_format_days_remaining() {
        assert_eq!(format_days_remaining(45), "45d");
        assert_eq!(format_days_remaining(0), "today");
        assert_eq!(format_days_remaining(-3), "expired 3d ago");
    }

    #[test]
    fn test_certificate_style_escalates() {
        // Distinct styling at each band, so severity reads at a glance
        let healthy = certificate_style(60);
        let warning = certificate_style(21);
        let critical = certificate_style(7);

        assert_ne!(healthy, warning);
        assert_ne!(warning, critical);
    }

    #[test]
    fn test_renewal_status_flags_a_failed_unit() {
        let (text, _) = format_renewal_status(Some("certbot.service"), Some("failed"));
        assert!(text.contains("renewal is broken"));
    }

    #[test]
    fn test_renewal_status_healthy() {
        let (text, _) = format_renewal_status(Some("certbot.timer"), Some("active"));
        assert!(text.contains("certbot.timer"));
        assert!(!text.contains("broken"));
    }

    #[test]
    fn test_renewal_status_absent() {
        let (text, _) = format_renewal_status(None, None);
        assert!(text.contains("no renewal unit"));
    }

    #[test]
    fn test_tls_tab_lists_certificates_and_renewal() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.tls_cache.insert(
            server_id,
            TlsSnapshot {
                certificates: vec![certificate("example.com", 12)],
                renewal_unit_name: Some("certbot.service".to_string()),
                renewal_unit_status: Some("failed".to_string()),
            },
        );

        let buffer = render_app_to_buffer(&app, draw_tls_tab);
        assert!(buffer_contains(&buffer, "example.com"));
        assert!(buffer_contains(&buffer, "12d"));
        assert!(buffer_contains(&buffer, "renewal is broken"));
    }

    #[test]
    fn test_tls_tab_reports_expired_certificate() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.tls_cache.insert(
            server_id,
            TlsSnapshot {
                certificates: vec![certificate("dead.example", -5)],
                renewal_unit_name: None,
                renewal_unit_status: None,
            },
        );

        let buffer = render_app_to_buffer(&app, draw_tls_tab);
        assert!(buffer_contains(&buffer, "expired 5d ago"));
    }

    #[test]
    fn test_tls_tab_without_certificates() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.tls_cache.insert(server_id, TlsSnapshot::default());

        let buffer = render_app_to_buffer(&app, draw_tls_tab);
        assert!(buffer_contains(&buffer, "No certificates found"));
    }

    #[test]
    fn test_tls_tab_not_connected() {
        let app = create_test_app();
        let buffer = render_app_to_buffer(&app, draw_tls_tab);
        assert!(buffer_contains(&buffer, "Not connected"));
    }

    fn container_fixture(name: &str) -> ContainerInfo {
        ContainerInfo {
            id: format!("id-{}", name),
            name: name.to_string(),
            image: "busybox:latest".to_string(),
            status: "Up 3 days".to_string(),
            state: "running".to_string(),
            health: "none".to_string(),
            cpu_percent: 1.0,
            memory_usage_bytes: 100_000_000,
            memory_limit_bytes: 1_000_000_000,
            memory_percent: Some(10.0),
            restart_count: 0,
            network_rx_bytes: 0,
            network_tx_bytes: 0,
            networks: vec![],
            memory_limit_set: true,
            health_detail: None,
            swap_bytes: None,
        }
    }

    fn health_detail(streak: u32, output: &str) -> ContainerHealthDetail {
        ContainerHealthDetail {
            failing_streak: streak,
            last_output: output.to_string(),
            last_exit_code: 1,
            last_checked_at: None,
        }
    }

    // ─────────────────────────────────────────
    // Restart formatting
    // ─────────────────────────────────────────

    #[test]
    fn test_format_restart_count_zero_is_a_dash() {
        assert_eq!(format_restart_count(0), "—");
    }

    #[test]
    fn test_format_restart_count_small_values_are_exact() {
        assert_eq!(format_restart_count(7), "7");
        assert_eq!(format_restart_count(999), "999");
    }

    #[test]
    fn test_format_restart_count_abbreviates_large_values() {
        assert_eq!(format_restart_count(60_040), "60.0k");
        assert_eq!(format_restart_count(412_813), "412.8k");
        assert_eq!(format_restart_count(1_500_000), "1.5M");
    }

    #[test]
    fn test_format_restart_count_stays_narrow() {
        // The column is 9% of the panel; nothing may blow it up
        for value in [0, 9, 999, 1_000, 60_040, 412_813, u32::MAX] {
            assert!(
                format_restart_count(value).chars().count() <= 6,
                "too wide for the column: {}",
                format_restart_count(value)
            );
        }
    }

    // ─────────────────────────────────────────
    // Health icon
    // ─────────────────────────────────────────

    #[test]
    fn test_health_icon_distinguishes_broken_check_from_failure() {
        let mut failing = container_fixture("sick");
        failing.health_detail = Some(health_detail(3, "HTTP 503"));

        let mut broken = container_fixture("misconfigured");
        broken.health_detail = Some(health_detail(1_344, "curl: not found"));

        let failing_icon = container_health_icon(&failing).unwrap();
        let broken_icon = container_health_icon(&broken).unwrap();

        assert_ne!(
            failing_icon.0, broken_icon.0,
            "a broken probe must not look like an outage"
        );
    }

    #[test]
    fn test_health_icon_passing() {
        let mut healthy = container_fixture("fine");
        healthy.health_detail = Some(health_detail(0, ""));
        assert!(container_health_icon(&healthy).is_some());
    }

    #[test]
    fn test_health_icon_falls_back_to_status_string() {
        // A server predating the health detail fields still renders sensibly
        let mut legacy = container_fixture("legacy");
        legacy.health_detail = None;
        legacy.health = "unhealthy".to_string();
        assert!(container_health_icon(&legacy).is_some());

        legacy.health = "none".to_string();
        assert!(container_health_icon(&legacy).is_none());
    }

    // ─────────────────────────────────────────
    // Risk summary
    // ─────────────────────────────────────────

    #[test]
    fn test_risk_summary_absent_for_a_healthy_host() {
        let containers = vec![container_fixture("a"), container_fixture("b")];
        assert!(summarize_container_risks(&containers).is_none());
    }

    #[test]
    fn test_risk_summary_counts_containers_without_limits() {
        // alemanha7 ran 20 of 20 containers with no limit
        let mut containers: Vec<ContainerInfo> = (0..20)
            .map(|i| container_fixture(&format!("c{}", i)))
            .collect();
        for c in containers.iter_mut() {
            c.memory_limit_set = false;
        }

        let summary = summarize_container_risks(&containers).unwrap();
        assert!(summary.contains("20/20 without mem_limit"));
    }

    #[test]
    fn test_risk_summary_counts_broken_healthchecks() {
        let mut containers = vec![container_fixture("a"), container_fixture("b")];
        containers[0].health_detail = Some(health_detail(5_000, "curl: not found"));

        let summary = summarize_container_risks(&containers).unwrap();
        assert!(summary.contains("1 broken healthcheck"));
    }

    #[test]
    fn test_risk_summary_counts_crash_loops() {
        let mut containers = vec![container_fixture("a")];
        containers[0].restart_count = 60_040;

        let summary = summarize_container_risks(&containers).unwrap();
        assert!(summary.contains("1 crash-looping"));
    }

    #[test]
    fn test_risk_summary_ignores_a_handful_of_restarts() {
        let mut containers = vec![container_fixture("a")];
        containers[0].restart_count = 4; // reboots and deploys
        assert!(summarize_container_risks(&containers).is_none());
    }

    #[test]
    fn test_containers_tab_shows_restarts_and_risk_summary() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;

        let mut looping = container_fixture("netfilter-mailcow");
        looping.restart_count = 412_813;
        looping.memory_limit_set = false;
        looping.memory_percent = None;

        app.containers_cache
            .insert(server_id, vec![looping, container_fixture("healthy-app")]);

        let buffer = render_app_to_buffer(&app, draw_containers_tab);
        assert!(buffer_contains(&buffer, "Restarts"));
        assert!(buffer_contains(&buffer, "412.8k"));
        assert!(buffer_contains(&buffer, "no limit"));
        assert!(buffer_contains(&buffer, "crash-looping"));
    }

    #[test]
    fn test_format_container_memory_percent_with_limit() {
        assert_eq!(format_container_memory_percent(Some(8.16)), "  8.2%");
        assert_eq!(format_container_memory_percent(Some(100.0)), "100.0%");
    }

    #[test]
    fn test_format_container_memory_percent_without_limit() {
        // Never render a percentage against the host's RAM: it would read as a
        // comfortable 8% for a container that can take the whole host down.
        let rendered = format_container_memory_percent(None);
        assert_eq!(rendered, "no limit");
        assert!(!rendered.contains('%'));
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
