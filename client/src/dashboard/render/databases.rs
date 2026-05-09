use tui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

use crate::dashboard::DashboardApp;

use super::Theme;

/// Format database size in bytes to a human-readable MB string
pub(super) fn format_db_size_mb(size_bytes: u64) -> String {
    format!("{:.1} MB", size_bytes as f64 / 1_048_576.0)
}

/// Truncate a query string to max length with ellipsis
pub(super) fn truncate_query(query: &str, max_len: usize) -> String {
    if query.len() > max_len {
        format!("{}...", &query[..max_len])
    } else {
        query.to_string()
    }
}

pub(super) fn draw_postgres_tab<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    app: &DashboardApp,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::BORDER))
        .title(Span::styled(
            " 🐘 Postgres Clusters ",
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));

    if let Some(server) = app.get_selected_server() {
        if let Some(clusters) = app.postgres_cache.get(&server.id) {
            if clusters.is_empty() {
                let no_pg = Paragraph::new("No Postgres clusters configured or reachable.")
                    .style(Style::default().fg(Theme::MUTED))
                    .alignment(Alignment::Center)
                    .block(block);
                f.render_widget(no_pg, area);
                return;
            }

            // Split area: top for clusters list, bottom for top queries of selected cluster
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .margin(1)
                .split(area);

            // Draw block border manually since we use inner area
            f.render_widget(block.clone(), area);

            // --- Top panel: cluster list with databases ---
            let cluster_rows: Vec<Row> = clusters
                .iter()
                .map(|c| {
                    let db_summary = if c.databases.is_empty() {
                        "No user databases".to_string()
                    } else {
                        c.databases
                            .iter()
                            .map(|d| {
                                format!("{} ({})", d.name, format_db_size_mb(d.size_bytes))
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    };

                    let conn_style = if c.connections_total > 80 {
                        Style::default().fg(Theme::ERROR)
                    } else if c.connections_total > 40 {
                        Style::default().fg(Theme::WARNING)
                    } else {
                        Style::default().fg(Theme::TEXT)
                    };

                    let cache_style = if c.cache_hit_ratio < 90.0 {
                        Style::default().fg(Theme::WARNING)
                    } else {
                        Style::default().fg(Theme::SUCCESS)
                    };

                    Row::new(vec![
                        Cell::from(Span::styled(&c.name, Style::default().fg(Theme::TEXT))),
                        Cell::from(Span::styled(
                            format!("{}:{}", c.host, c.port),
                            Style::default().fg(Theme::MUTED),
                        )),
                        Cell::from(Span::styled(db_summary, Style::default().fg(Theme::TEXT))),
                        Cell::from(Span::styled(format!("{}", c.connections_total), conn_style)),
                        Cell::from(Span::styled(
                            format!("{:.1}%", c.cache_hit_ratio),
                            cache_style,
                        )),
                    ])
                })
                .collect();

            let cluster_table = Table::new(cluster_rows)
                .header(
                    Row::new(vec![
                        "Cluster",
                        "Endpoint",
                        "Databases",
                        "Conns",
                        "Cache Hit",
                    ])
                    .style(
                        Style::default()
                            .fg(Theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                )
                .widths(&[
                    Constraint::Percentage(18),
                    Constraint::Percentage(20),
                    Constraint::Percentage(42),
                    Constraint::Percentage(10),
                    Constraint::Percentage(10),
                ])
                .highlight_style(
                    Style::default()
                        .bg(Theme::HIGHLIGHT_BG)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");

            let mut state = app.table_state.clone();
            f.render_stateful_widget(cluster_table, chunks[0], &mut state);

            // --- Bottom panel: top queries for selected cluster ---
            let selected_idx = app.selected_item_idx.min(clusters.len().saturating_sub(1));
            let selected_cluster = &clusters[selected_idx];

            let query_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Theme::BORDER))
                .title(Span::styled(
                    format!(" Top Queries — {} ", selected_cluster.name),
                    Style::default()
                        .fg(Theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ));

            if selected_cluster.top_queries.is_empty() {
                let no_queries =
                    Paragraph::new("No pg_stat_statements available or no queries recorded.")
                        .style(Style::default().fg(Theme::MUTED))
                        .alignment(Alignment::Center)
                        .block(query_block);
                f.render_widget(no_queries, chunks[1]);
            } else {
                let query_rows: Vec<Row> = selected_cluster
                    .top_queries
                    .iter()
                    .map(|q| {
                        let truncated_query = truncate_query(&q.query, 60);
                        Row::new(vec![
                            Cell::from(Span::styled(
                                truncated_query,
                                Style::default().fg(Theme::TEXT),
                            )),
                            Cell::from(Span::styled(
                                format!("{}", q.calls),
                                Style::default().fg(Theme::MUTED),
                            )),
                            Cell::from(Span::styled(
                                format!("{:.1} ms", q.total_exec_time_ms),
                                Style::default().fg(Theme::WARNING),
                            )),
                            Cell::from(Span::styled(
                                format!("{:.2} ms", q.mean_exec_time_ms),
                                Style::default().fg(Theme::TEXT),
                            )),
                        ])
                    })
                    .collect();

                let query_table = Table::new(query_rows)
                    .header(
                        Row::new(vec!["Query", "Calls", "Total (ms)", "Mean (ms)"]).style(
                            Style::default()
                                .fg(Theme::ACCENT)
                                .add_modifier(Modifier::BOLD),
                        ),
                    )
                    .widths(&[
                        Constraint::Percentage(60),
                        Constraint::Percentage(12),
                        Constraint::Percentage(14),
                        Constraint::Percentage(14),
                    ])
                    .block(query_block);

                f.render_widget(query_table, chunks[1]);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_db_size_mb_zero() {
        assert_eq!(format_db_size_mb(0), "0.0 MB");
    }

    #[test]
    fn test_format_db_size_mb_one_mb() {
        assert_eq!(format_db_size_mb(1_048_576), "1.0 MB");
    }

    #[test]
    fn test_format_db_size_mb_fractional() {
        assert_eq!(format_db_size_mb(1_572_864), "1.5 MB");
    }

    #[test]
    fn test_format_db_size_mb_large() {
        assert_eq!(format_db_size_mb(1_073_741_824), "1024.0 MB");
    }

    #[test]
    fn test_truncate_query_short() {
        assert_eq!(truncate_query("SELECT 1", 60), "SELECT 1");
    }

    #[test]
    fn test_truncate_query_exact() {
        let query = "a".repeat(60);
        assert_eq!(truncate_query(&query, 60), query);
    }

    #[test]
    fn test_truncate_query_long() {
        let query = "a".repeat(100);
        assert_eq!(truncate_query(&query, 60), format!("{}...", "a".repeat(60)));
    }

    #[test]
    fn test_truncate_query_empty() {
        assert_eq!(truncate_query("", 60), "");
    }
}

pub(super) fn draw_mariadb_tab<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    app: &DashboardApp,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Theme::BORDER))
        .title(Span::styled(
            " 🗄️ MariaDB Clusters ",
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));

    if let Some(server) = app.get_selected_server() {
        if let Some(clusters) = app.mariadb_cache.get(&server.id) {
            if clusters.is_empty() {
                let no_db = Paragraph::new("No MariaDB clusters configured or reachable.")
                    .style(Style::default().fg(Theme::MUTED))
                    .alignment(Alignment::Center)
                    .block(block);
                f.render_widget(no_db, area);
                return;
            }

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .margin(1)
                .split(area);

            f.render_widget(block.clone(), area);

            let cluster_rows: Vec<Row> = clusters
                .iter()
                .map(|c| {
                    let schema_summary = if c.schemas.is_empty() {
                        "No schemas".to_string()
                    } else {
                        c.schemas
                            .iter()
                            .map(|s| {
                                format!(
                                    "{} ({:.1} MB, {} tables)",
                                    s.name,
                                    s.size_bytes as f64 / 1_048_576.0,
                                    s.table_count
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    };

                    let conn_style = if c.connections_active > 80 {
                        Style::default().fg(Theme::ERROR)
                    } else if c.connections_active > 40 {
                        Style::default().fg(Theme::WARNING)
                    } else {
                        Style::default().fg(Theme::TEXT)
                    };

                    Row::new(vec![
                        Cell::from(Span::styled(&c.name, Style::default().fg(Theme::TEXT))),
                        Cell::from(Span::styled(
                            format!("{}:{}", c.host, c.port),
                            Style::default().fg(Theme::MUTED),
                        )),
                        Cell::from(Span::styled(
                            schema_summary,
                            Style::default().fg(Theme::TEXT),
                        )),
                        Cell::from(Span::styled(
                            format!("{}/{}", c.connections_active, c.connections_total),
                            conn_style,
                        )),
                    ])
                })
                .collect();

            let cluster_table = Table::new(cluster_rows)
                .header(
                    Row::new(vec!["Cluster", "Endpoint", "Schemas", "Conns"]).style(
                        Style::default()
                            .fg(Theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                )
                .widths(&[
                    Constraint::Percentage(20),
                    Constraint::Percentage(22),
                    Constraint::Percentage(42),
                    Constraint::Percentage(16),
                ])
                .highlight_style(
                    Style::default()
                        .bg(Theme::HIGHLIGHT_BG)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");

            let mut state = app.table_state.clone();
            f.render_stateful_widget(cluster_table, chunks[0], &mut state);

            // Bottom panel: active processes for selected cluster
            let selected_idx = app.selected_item_idx.min(clusters.len().saturating_sub(1));
            let selected_cluster = &clusters[selected_idx];

            let proc_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Theme::BORDER))
                .title(Span::styled(
                    format!(" Active Processes — {} ", selected_cluster.name),
                    Style::default()
                        .fg(Theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ));

            if selected_cluster.processes.is_empty() {
                let no_proc = Paragraph::new("No active processes (all sleeping).")
                    .style(Style::default().fg(Theme::MUTED))
                    .alignment(Alignment::Center)
                    .block(proc_block);
                f.render_widget(no_proc, chunks[1]);
            } else {
                let proc_rows: Vec<Row> = selected_cluster
                    .processes
                    .iter()
                    .map(|p| {
                        let cmd = if p.command.len() > 30 {
                            format!("{}...", &p.command[..30])
                        } else {
                            p.command.clone()
                        };
                        let info = p
                            .info
                            .as_ref()
                            .map(|i| {
                                if i.len() > 40 {
                                    format!("{}...", &i[..40])
                                } else {
                                    i.clone()
                                }
                            })
                            .unwrap_or_default();
                        Row::new(vec![
                            Cell::from(Span::styled(
                                format!("{}", p.id),
                                Style::default().fg(Theme::MUTED),
                            )),
                            Cell::from(Span::styled(&p.user, Style::default().fg(Theme::TEXT))),
                            Cell::from(Span::styled(cmd, Style::default().fg(Theme::TEXT))),
                            Cell::from(Span::styled(
                                format!("{}s", p.time_seconds),
                                Style::default().fg(Theme::WARNING),
                            )),
                            Cell::from(Span::styled(&p.state, Style::default().fg(Theme::MUTED))),
                            Cell::from(Span::styled(info, Style::default().fg(Theme::TEXT))),
                        ])
                    })
                    .collect();

                let proc_table = Table::new(proc_rows)
                    .header(
                        Row::new(vec!["ID", "User", "Command", "Time", "State", "Query"]).style(
                            Style::default()
                                .fg(Theme::ACCENT)
                                .add_modifier(Modifier::BOLD),
                        ),
                    )
                    .widths(&[
                        Constraint::Percentage(8),
                        Constraint::Percentage(10),
                        Constraint::Percentage(15),
                        Constraint::Percentage(8),
                        Constraint::Percentage(12),
                        Constraint::Percentage(47),
                    ])
                    .block(proc_block);

                f.render_widget(proc_table, chunks[1]);
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
