use tui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Tabs},
    Frame,
};

use super::{ConnectionStatus, DashboardApp, InputMode};

mod databases;
mod popups;
mod processes;
mod tabs;

use databases::{draw_mariadb_tab, draw_postgres_tab};
use popups::{draw_alerts_popup, draw_command_input, draw_help_popup, draw_settings_popup};
use processes::draw_processes_tab;
use tabs::{
    draw_containers_tab, draw_network_tab, draw_overview_tab, draw_services_tab, draw_systemd_tab,
};

/// Theme colors for a modern look
pub(super) struct Theme;

impl Theme {
    pub(super) const ACCENT: Color = Color::Rgb(137, 180, 250);
    pub(super) const SUCCESS: Color = Color::Rgb(166, 227, 161);
    pub(super) const WARNING: Color = Color::Rgb(249, 226, 175);
    pub(super) const ERROR: Color = Color::Rgb(243, 139, 168);
    pub(super) const MUTED: Color = Color::Rgb(147, 153, 178);
    pub(super) const TEXT: Color = Color::Rgb(205, 214, 244);
    pub(super) const BORDER: Color = Color::Rgb(88, 91, 112);
    pub(super) const HIGHLIGHT_BG: Color = Color::Rgb(69, 71, 90);
    pub(super) const CPU_COLOR: Color = Color::Rgb(137, 180, 250);
    pub(super) const MEM_COLOR: Color = Color::Rgb(203, 166, 247);
    pub(super) const DISK_COLOR: Color = Color::Rgb(166, 227, 161);
}

pub(super) fn draw_ui<B: tui::backend::Backend>(f: &mut Frame<B>, app: &DashboardApp) {
    let size = f.size();

    // Create main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(0)
        .constraints([
            Constraint::Length(3), // Title and tabs
            Constraint::Length(2), // Help bar
            Constraint::Min(10),   // Main content
            Constraint::Length(3), // Status bar
        ])
        .split(size);

    // Draw title and tabs
    draw_header(f, app, chunks[0]);

    // Draw help bar
    draw_help_bar(f, app, chunks[1]);

    // Draw main content based on current tab
    draw_main_content(f, app, chunks[2]);

    // Draw status bar
    draw_status_bar(f, app, chunks[3]);

    // Draw command input if in input mode
    if matches!(
        app.input_mode,
        InputMode::Command
            | InputMode::AddServer
            | InputMode::EditToken
            | InputMode::ConfirmDisconnect
    ) {
        draw_command_input(f, app, size);
    }

    // Draw settings popup
    if matches!(app.input_mode, InputMode::Settings) {
        draw_settings_popup(f, app, size);
    }

    // Draw help popup if shown
    if app.show_help {
        draw_help_popup(f, size);
    }

    // Draw alerts popup if shown
    if app.show_alerts {
        draw_alerts_popup(f, app, size);
    }
}

fn draw_help_bar<B: tui::backend::Backend>(f: &mut Frame<B>, app: &DashboardApp, area: Rect) {
    let help_text = match app.input_mode {
        InputMode::AddServer => "Enter value, then press Enter. Press Esc to cancel.",
        InputMode::EditToken => "Enter token, then press Enter. Press Esc to cancel.",
        InputMode::Command => "Enter command, then press Enter. Press Esc to cancel.",
        InputMode::ConfirmDisconnect => "Press Y to confirm disconnect, N or Esc to cancel.",
        InputMode::Settings => "Up/Down:Navigate  Enter:Toggle  Esc:Close",
        InputMode::Normal => match app.current_tab {
            0 => "Enter:Connect  a:Add  t:Token  Del:Remove  A:Alerts  s:Settings  o:Sort  ?:Help  q:Quit",
            1 => "Up/Down:Navigate  o:Sort  O:Reverse  s:Settings  ?:Help  q:Quit",
            2 => "Up/Down:Navigate  /:Filter  x:Clear  o:Sort  s:Settings  ?:Help  q:Quit",
            3 => "Up/Down:Navigate  s:Settings  ?:Help  q:Quit",
            4 => "Up/Down:Navigate  o:Sort  s:Settings  ?:Help  q:Quit",
            _ => "",
        },
    };

    let help = Paragraph::new(Spans::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(help_text, Style::default().fg(Theme::MUTED)),
    ]));

    f.render_widget(help, area);
}

fn draw_header<B: tui::backend::Backend>(f: &mut Frame<B>, app: &DashboardApp, area: Rect) {
    let tab_titles = [
        "󰍹 Overview",
        "󰒍 Services",
        "󰓁 Processes",
        "󰛳 Network",
        "󰡨 Containers",
        "🐘 Postgres",
        "🗄️ MariaDB",
        "⚙️ Systemd",
    ];
    let titles: Vec<Spans> = tab_titles
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style = if i == app.current_tab {
                Style::default()
                    .fg(Theme::ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Theme::MUTED)
            };
            Spans::from(Span::styled(format!(" {} [{}] ", t, i + 1), style))
        })
        .collect();

    // Count connected servers
    let connected_count = app.connected_clients.len();
    let total_count = app.servers.len();
    let conn_status = format!(" 󰒋 {}/{} ", connected_count, total_count);

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Theme::BORDER))
                .title(Span::styled(
                    " 󰄪 System Monitor ",
                    Style::default()
                        .fg(Theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ))
                .title_alignment(Alignment::Left),
        )
        .select(app.current_tab)
        .style(Style::default().fg(Theme::MUTED))
        .divider(Span::styled(" │ ", Style::default().fg(Theme::BORDER)));

    f.render_widget(tabs, area);

    // Draw connection status on the right
    let status_area = Rect::new(area.x + area.width.saturating_sub(15), area.y, 14, 1);
    let status_style = if connected_count > 0 {
        Style::default().fg(Theme::SUCCESS)
    } else {
        Style::default().fg(Theme::ERROR)
    };
    let status = Paragraph::new(Span::styled(conn_status, status_style));
    f.render_widget(status, status_area);
}

fn draw_main_content<B: tui::backend::Backend>(f: &mut Frame<B>, app: &DashboardApp, area: Rect) {
    // Split into sidebar (servers) and main panel
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25), // Server list
            Constraint::Percentage(75), // Content
        ])
        .split(area);

    // Draw server list
    draw_server_list(f, app, chunks[0]);

    // Draw content panel based on tab
    match app.current_tab {
        0 => draw_overview_tab(f, app, chunks[1]),
        1 => draw_services_tab(f, app, chunks[1]),
        2 => draw_processes_tab(f, app, chunks[1]),
        3 => draw_network_tab(f, app, chunks[1]),
        4 => draw_containers_tab(f, app, chunks[1]),
        5 => draw_postgres_tab(f, app, chunks[1]),
        6 => draw_mariadb_tab(f, app, chunks[1]),
        7 => draw_systemd_tab(f, app, chunks[1]),
        _ => {}
    }
}

fn draw_server_list<B: tui::backend::Backend>(f: &mut Frame<B>, app: &DashboardApp, area: Rect) {
    let items: Vec<ListItem> = app
        .servers
        .iter()
        .map(|server| {
            let (status_icon, status_color) = match app.connection_status.get(&server.id) {
                Some(ConnectionStatus::Connected) => ("󰅟", Theme::SUCCESS),
                Some(ConnectionStatus::Connecting) => ("󰦖", Theme::WARNING),
                Some(ConnectionStatus::Error(_)) => ("󰅜", Theme::ERROR),
                _ => ("󰅛", Theme::MUTED),
            };

            let content = Spans::from(vec![
                Span::styled(
                    format!("{} ", status_icon),
                    Style::default().fg(status_color),
                ),
                Span::styled(&server.name, Style::default().fg(Theme::TEXT)),
            ]);
            ListItem::new(content)
        })
        .collect();

    let server_list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Theme::BORDER))
                .title(Span::styled(
                    " 󰒋 Servers ",
                    Style::default()
                        .fg(Theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                )),
        )
        .highlight_style(
            Style::default()
                .bg(Theme::HIGHLIGHT_BG)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    let mut state = app.server_list_state.clone();
    f.render_stateful_widget(server_list, area, &mut state);
}

fn draw_status_bar<B: tui::backend::Backend>(f: &mut Frame<B>, app: &DashboardApp, area: Rect) {
    let now = chrono::Local::now().format("%H:%M:%S").to_string();

    let status_spans = if app.status_message.is_empty() {
        vec![
            Span::styled(" 󰥔 ", Style::default().fg(Theme::MUTED)),
            Span::styled(&now, Style::default().fg(Theme::TEXT)),
            Span::styled(" │ ", Style::default().fg(Theme::BORDER)),
            Span::styled("Ready", Style::default().fg(Theme::SUCCESS)),
        ]
    } else {
        vec![
            Span::styled(" 󰥔 ", Style::default().fg(Theme::MUTED)),
            Span::styled(&now, Style::default().fg(Theme::TEXT)),
            Span::styled(" │ ", Style::default().fg(Theme::BORDER)),
            Span::styled(&app.status_message, Style::default().fg(Theme::ACCENT)),
        ]
    };

    let status = Paragraph::new(Spans::from(status_spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Theme::BORDER)),
    );

    f.render_widget(status, area);
}

pub(super) fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.1} {}", size, UNITS[unit_index])
}

pub(super) fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;

    if days > 0 {
        format!("{}d {}h {}m", days, hours, minutes)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes_zero() {
        assert_eq!(format_bytes(0), "0.0 B");
    }

    #[test]
    fn test_format_bytes_bytes() {
        assert_eq!(format_bytes(512), "512.0 B");
    }

    #[test]
    fn test_format_bytes_kb() {
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
    }

    #[test]
    fn test_format_bytes_mb() {
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.0 MB");
    }

    #[test]
    fn test_format_bytes_gb() {
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_bytes(1500 * 1024 * 1024), "1.5 GB");
    }

    #[test]
    fn test_format_bytes_tb() {
        assert_eq!(format_bytes(1024u64.pow(4)), "1.0 TB");
    }

    #[test]
    fn test_format_bytes_large() {
        // 5 TB should be formatted correctly
        assert_eq!(format_bytes(5 * 1024u64.pow(4)), "5.0 TB");
    }

    #[test]
    fn test_format_uptime_zero() {
        assert_eq!(format_uptime(0), "0m");
    }

    #[test]
    fn test_format_uptime_minutes() {
        assert_eq!(format_uptime(300), "5m");
        assert_eq!(format_uptime(3599), "59m");
    }

    #[test]
    fn test_format_uptime_hours() {
        assert_eq!(format_uptime(3600), "1h 0m");
        assert_eq!(format_uptime(3660), "1h 1m");
        assert_eq!(format_uptime(7200), "2h 0m");
    }

    #[test]
    fn test_format_uptime_days() {
        assert_eq!(format_uptime(86400), "1d 0h 0m");
        assert_eq!(format_uptime(90061), "1d 1h 1m");
        assert_eq!(format_uptime(172800), "2d 0h 0m");
    }

    #[test]
    fn test_format_uptime_complex() {
        assert_eq!(format_uptime(90061), "1d 1h 1m"); // 86400 + 3600 + 60 + 1
    }

    #[test]
    fn test_theme_colors_exist() {
        // Just verify the theme constants are accessible
        assert_eq!(Theme::ACCENT, Color::Rgb(137, 180, 250));
        assert_eq!(Theme::SUCCESS, Color::Rgb(166, 227, 161));
        assert_eq!(Theme::ERROR, Color::Rgb(243, 139, 168));
    }
}
