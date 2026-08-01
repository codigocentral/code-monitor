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

/// Abbreviate a settings source file to what an operator needs to tell them
/// apart: the filename.
fn setting_source_label(setting: &shared::types::PostgresSetting) -> String {
    match setting.source_file.as_deref() {
        Some(path) => path.rsplit('/').next().unwrap_or(path).to_string(),
        None => setting.source.clone(),
    }
}

/// Render the cluster's memory and connection settings with their origin.
///
/// The origin is highlighted, not the value: a `shared_buffers` of 4GB is only
/// surprising once you know the config file under version control says 128MB
/// and `ALTER SYSTEM` quietly won.
fn draw_postgres_settings<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    cluster: &shared::types::PostgresClusterInfo,
    area: Rect,
) {
    let mixed = cluster.has_mixed_setting_sources();
    let title = if mixed {
        format!(" Settings — {} (mixed sources) ", cluster.name)
    } else {
        format!(" Settings — {} ", cluster.name)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if mixed { Theme::WARNING } else { Theme::BORDER }))
        .title(Span::styled(
            title,
            Style::default()
                .fg(if mixed { Theme::WARNING } else { Theme::ACCENT })
                .add_modifier(Modifier::BOLD),
        ));

    let rows: Vec<Row> = cluster
        .settings
        .iter()
        .map(|setting| {
            let value = match &setting.unit {
                Some(unit) => format!("{} {}", setting.value, unit),
                None => setting.value.clone(),
            };

            let source_style = if setting.is_from_auto_conf() {
                // The case that misleads whoever debugs from postgresql.conf
                Style::default()
                    .fg(Theme::WARNING)
                    .add_modifier(Modifier::BOLD)
            } else if setting.is_default() {
                Style::default().fg(Theme::MUTED)
            } else {
                Style::default().fg(Theme::TEXT)
            };

            Row::new(vec![
                Cell::from(Span::styled(
                    &setting.name,
                    Style::default().fg(Theme::TEXT),
                )),
                Cell::from(Span::styled(
                    value,
                    Style::default()
                        .fg(Theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(setting_source_label(setting), source_style)),
            ])
        })
        .collect();

    let table = Table::new(rows)
        .header(
            Row::new(vec!["Setting", "Value", "From"]).style(
                Style::default()
                    .fg(Theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .widths(&[
            Constraint::Percentage(42),
            Constraint::Percentage(26),
            Constraint::Percentage(30),
        ])
        .block(block);

    f.render_widget(table, area);
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
                            .map(|d| format!("{} ({})", d.name, format_db_size_mb(d.size_bytes)))
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

            // --- Bottom panel: top queries and settings for selected cluster ---
            let selected_idx = app.selected_item_idx.min(clusters.len().saturating_sub(1));
            let selected_cluster = &clusters[selected_idx];

            // Settings share the bottom half. A cluster's memory settings
            // explain more incidents than its slow queries do.
            let (queries_area, settings_area) = if selected_cluster.settings.is_empty() {
                (chunks[1], None)
            } else {
                let bottom = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                    .split(chunks[1]);
                (bottom[0], Some(bottom[1]))
            };

            if let Some(settings_area) = settings_area {
                draw_postgres_settings(f, selected_cluster, settings_area);
            }

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
                f.render_widget(no_queries, queries_area);
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

                f.render_widget(query_table, queries_area);
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
#[cfg(test)]
mod tests {
    use super::*;

    use shared::types::PostgresSetting;

    fn pg_setting(name: &str, value: &str, file: Option<&str>) -> PostgresSetting {
        PostgresSetting {
            name: name.to_string(),
            value: value.to_string(),
            unit: None,
            source: if file.is_some() {
                "configuration file".to_string()
            } else {
                "default".to_string()
            },
            source_file: file.map(str::to_string),
            source_line: file.map(|_| 1),
        }
    }

    #[test]
    fn test_setting_source_label_uses_the_filename() {
        let s = pg_setting(
            "shared_buffers",
            "524288",
            Some("/var/lib/postgresql/data/postgresql.auto.conf"),
        );
        assert_eq!(setting_source_label(&s), "postgresql.auto.conf");
    }

    #[test]
    fn test_setting_source_label_falls_back_to_source() {
        let s = pg_setting("work_mem", "4096", None);
        assert_eq!(setting_source_label(&s), "default");
    }

    #[test]
    fn test_setting_source_label_handles_a_bare_filename() {
        let s = pg_setting("work_mem", "4096", Some("postgresql.conf"));
        assert_eq!(setting_source_label(&s), "postgresql.conf");
    }

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
