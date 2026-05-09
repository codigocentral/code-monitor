use tui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::dashboard::{AddServerStep, DashboardApp, InputMode};

use super::Theme;

pub(super) fn draw_command_input<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    app: &DashboardApp,
    area: Rect,
) {
    let popup_area = centered_rect(60, 3, area);
    f.render_widget(Clear, popup_area);

    let (title, prefix, show_cursor) = match app.input_mode {
        InputMode::AddServer => {
            let step_name = match app.add_server_state.step {
                AddServerStep::Name => "Server Name",
                AddServerStep::Address => "Server Address",
                AddServerStep::Port => "Port",
                AddServerStep::Token => "Access Token",
                AddServerStep::Confirm => "Confirm (y/n)",
            };
            (format!(" [+] Add Server - {} ", step_name), "> ", true)
        }
        InputMode::EditToken => (" [Key] Enter Access Token ".to_string(), "> ", true),
        InputMode::Command => {
            if app.current_tab == 2 && !app.input_buffer.starts_with(':') {
                (" [?] Filter Processes ".to_string(), "? ", true)
            } else {
                (" [>] Command ".to_string(), ": ", true)
            }
        }
        InputMode::ConfirmDisconnect => {
            let server_name = app
                .get_selected_server()
                .map(|s| s.name.as_str())
                .unwrap_or("server");
            (
                format!(" [!] Disconnect from '{}'? ", server_name),
                "y/n: ",
                false,
            )
        }
        InputMode::Normal | InputMode::Settings => return,
    };

    let mut spans = vec![
        Span::styled(prefix, Style::default().fg(Theme::ACCENT)),
        Span::styled(&app.input_buffer, Style::default().fg(Theme::TEXT)),
    ];
    if show_cursor {
        spans.push(Span::styled(
            "|",
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::SLOW_BLINK),
        ));
    }

    let input = Paragraph::new(Spans::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Theme::ACCENT))
            .title(Span::styled(title, Style::default().fg(Theme::ACCENT))),
    );

    f.render_widget(input, popup_area);
}

pub(super) fn draw_settings_popup<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    app: &DashboardApp,
    area: Rect,
) {
    let popup_area = centered_rect(50, 12, area);
    f.render_widget(Clear, popup_area);

    let update_interval_str = format!("{} seconds  [<-/->]", app.update_interval);

    let settings_items: Vec<(&str, String)> = vec![
        (
            "Icon Style",
            if app.use_ascii_icons {
                "ASCII (compatible)".to_string()
            } else {
                "Nerd Font (fancy)".to_string()
            },
        ),
        ("Update Interval", update_interval_str),
        (
            "Auto-connect",
            if app.auto_connect {
                "Enabled".to_string()
            } else {
                "Disabled".to_string()
            },
        ),
    ];

    let items: Vec<Spans> = settings_items
        .iter()
        .enumerate()
        .map(|(i, (label, value))| {
            let is_selected = i == app.settings_selection;
            let style = if is_selected {
                Style::default()
                    .fg(Theme::ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Theme::TEXT)
            };
            let prefix = if is_selected { "> " } else { "  " };
            Spans::from(vec![
                Span::styled(prefix, style),
                Span::styled(*label, style),
                Span::styled(": ", Style::default().fg(Theme::MUTED)),
                Span::styled(value.clone(), Style::default().fg(Theme::SUCCESS)),
            ])
        })
        .collect();

    let mut all_lines = vec![
        Spans::from(Span::styled(
            "[*] Settings",
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        Spans::from(Span::raw("")),
    ];
    all_lines.extend(items);
    all_lines.push(Spans::from(Span::raw("")));
    all_lines.push(Spans::from(Span::styled(
        "Up/Down: Navigate | Enter: Toggle | Esc: Close",
        Style::default().fg(Theme::MUTED),
    )));

    let settings = Paragraph::new(all_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Theme::ACCENT))
            .title(Span::styled(
                " Settings ",
                Style::default().fg(Theme::ACCENT),
            )),
    );

    f.render_widget(settings, popup_area);
}

pub(super) fn draw_help_popup<B: tui::backend::Backend>(f: &mut Frame<B>, area: Rect) {
    let popup_area = centered_rect(70, 28, area);
    f.render_widget(Clear, popup_area);

    let help_text = vec![
        Spans::from(Span::styled(
            "󰌌 Keyboard Shortcuts",
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        Spans::from(Span::raw("")),
        Spans::from(vec![Span::styled(
            "  Navigation",
            Style::default()
                .fg(Theme::WARNING)
                .add_modifier(Modifier::BOLD),
        )]),
        Spans::from(vec![
            Span::styled("    ↑/↓ or j/k  ", Style::default().fg(Theme::TEXT)),
            Span::styled("Navigate items/servers", Style::default().fg(Theme::MUTED)),
        ]),
        Spans::from(vec![
            Span::styled("    ←/→         ", Style::default().fg(Theme::TEXT)),
            Span::styled("Switch between servers", Style::default().fg(Theme::MUTED)),
        ]),
        Spans::from(vec![
            Span::styled("    Tab         ", Style::default().fg(Theme::TEXT)),
            Span::styled("Next tab", Style::default().fg(Theme::MUTED)),
        ]),
        Spans::from(vec![
            Span::styled("    1-8         ", Style::default().fg(Theme::TEXT)),
            Span::styled("Jump to tab", Style::default().fg(Theme::MUTED)),
        ]),
        Spans::from(Span::raw("")),
        Spans::from(vec![Span::styled(
            "  Server Management",
            Style::default()
                .fg(Theme::SUCCESS)
                .add_modifier(Modifier::BOLD),
        )]),
        Spans::from(vec![
            Span::styled("    a           ", Style::default().fg(Theme::TEXT)),
            Span::styled("Add new server (wizard)", Style::default().fg(Theme::MUTED)),
        ]),
        Spans::from(vec![
            Span::styled("    t           ", Style::default().fg(Theme::TEXT)),
            Span::styled(
                "Edit token for selected server",
                Style::default().fg(Theme::MUTED),
            ),
        ]),
        Spans::from(vec![
            Span::styled("    Delete      ", Style::default().fg(Theme::TEXT)),
            Span::styled("Remove selected server", Style::default().fg(Theme::MUTED)),
        ]),
        Spans::from(vec![
            Span::styled("    Enter       ", Style::default().fg(Theme::TEXT)),
            Span::styled(
                "Connect/disconnect server",
                Style::default().fg(Theme::MUTED),
            ),
        ]),
        Spans::from(vec![
            Span::styled("    C / D       ", Style::default().fg(Theme::TEXT)),
            Span::styled("Connect/disconnect all", Style::default().fg(Theme::MUTED)),
        ]),
        Spans::from(vec![
            Span::styled("    r / R       ", Style::default().fg(Theme::TEXT)),
            Span::styled("Refresh selected/all", Style::default().fg(Theme::MUTED)),
        ]),
        Spans::from(Span::raw("")),
        Spans::from(vec![Span::styled(
            "  Other",
            Style::default()
                .fg(Theme::MEM_COLOR)
                .add_modifier(Modifier::BOLD),
        )]),
        Spans::from(vec![
            Span::styled("    /           ", Style::default().fg(Theme::TEXT)),
            Span::styled("Filter processes", Style::default().fg(Theme::MUTED)),
        ]),
        Spans::from(vec![
            Span::styled("    x           ", Style::default().fg(Theme::TEXT)),
            Span::styled("Clear filter", Style::default().fg(Theme::MUTED)),
        ]),
        Spans::from(vec![
            Span::styled("    d           ", Style::default().fg(Theme::TEXT)),
            Span::styled("Toggle details panel", Style::default().fg(Theme::MUTED)),
        ]),
        Spans::from(vec![
            Span::styled("    A           ", Style::default().fg(Theme::TEXT)),
            Span::styled("Show alerts panel", Style::default().fg(Theme::MUTED)),
        ]),
        Spans::from(vec![
            Span::styled("    ?           ", Style::default().fg(Theme::TEXT)),
            Span::styled("Show this help", Style::default().fg(Theme::MUTED)),
        ]),
        Spans::from(vec![
            Span::styled("    q / Esc     ", Style::default().fg(Theme::TEXT)),
            Span::styled("Quit", Style::default().fg(Theme::MUTED)),
        ]),
        Spans::from(Span::raw("")),
        Spans::from(Span::styled(
            "Press any key to close",
            Style::default().fg(Theme::MUTED),
        )),
    ];

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Theme::ACCENT))
                .title(Span::styled(
                    " 󰋗 Help ",
                    Style::default()
                        .fg(Theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                )),
        )
        .alignment(Alignment::Left);

    f.render_widget(help, popup_area);
}

pub(super) fn draw_alerts_popup<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    app: &DashboardApp,
    area: Rect,
) {
    let popup_area = centered_rect(80, 20, area);
    f.render_widget(Clear, popup_area);

    // Get active alerts
    let active_alerts = app.alert_manager.get_active_alerts();

    // Build alert items
    let mut alert_items: Vec<Spans> = vec![
        Spans::from(Span::styled(
            "󰀦 Active Alerts",
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        Spans::from(Span::raw("")),
    ];

    if active_alerts.is_empty() {
        alert_items.push(Spans::from(Span::styled(
            "  No active alerts",
            Style::default().fg(Theme::SUCCESS),
        )));
    } else {
        // Header
        alert_items.push(Spans::from(vec![
            Span::styled(
                "  Severity  ",
                Style::default()
                    .fg(Theme::WARNING)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Server           ",
                Style::default()
                    .fg(Theme::WARNING)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Message",
                Style::default()
                    .fg(Theme::WARNING)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        alert_items.push(Spans::from(Span::raw(
            "  ─────────────────────────────────────────────────────────────",
        )));

        for (idx, alert) in active_alerts.iter().enumerate() {
            let severity_color = match alert.severity {
                shared::alerts::AlertSeverity::Info => Theme::ACCENT,
                shared::alerts::AlertSeverity::Warning => Theme::WARNING,
                shared::alerts::AlertSeverity::Critical => Theme::ERROR,
            };

            let severity_icon = match alert.severity {
                shared::alerts::AlertSeverity::Info => "ℹ",
                shared::alerts::AlertSeverity::Warning => "⚠",
                shared::alerts::AlertSeverity::Critical => "🔴",
            };

            let selected = idx == app.selected_alert_idx;
            let prefix = if selected { "> " } else { "  " };

            alert_items.push(Spans::from(vec![
                Span::styled(
                    format!("{}{} ", prefix, severity_icon),
                    Style::default().fg(severity_color),
                ),
                Span::styled(
                    format!("{:10}  ", format!("{:?}", alert.severity)),
                    Style::default().fg(severity_color),
                ),
                Span::styled(
                    format!("{:17}  ", &alert.server_id[..alert.server_id.len().min(16)]),
                    Style::default().fg(Theme::TEXT),
                ),
                Span::styled(alert.message.clone(), Style::default().fg(Theme::TEXT)),
            ]));
        }

        alert_items.push(Spans::from(Span::raw("")));
        alert_items.push(Spans::from(Span::styled(
            "  ↑/Down: Navigate  a: Acknowledge  A: Close",
            Style::default().fg(Theme::MUTED),
        )));
    }

    let alerts_widget = Paragraph::new(alert_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Theme::WARNING))
                .title(Span::styled(
                    format!(" 󰀦 Alerts ({}) ", active_alerts.len()),
                    Style::default()
                        .fg(Theme::WARNING)
                        .add_modifier(Modifier::BOLD),
                )),
        )
        .alignment(Alignment::Left);

    f.render_widget(alerts_widget, popup_area);
}

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(height),
            Constraint::Min(1),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui::{backend::TestBackend, Terminal};

    use crate::dashboard::{AddServerStep, DashboardApp, InputMode};
    use shared::notifications::NotificationConfig;
    use shared::types::ServerEndpoint;
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
        DashboardApp::new(vec![server], NotificationConfig::default(), None)
    }

    fn buffer_to_string(buffer: &tui::buffer::Buffer) -> String {
        buffer.content.iter().map(|cell| cell.symbol.clone()).collect()
    }

    // ------------------------------------------------------------------
    // centered_rect
    // ------------------------------------------------------------------

    #[test]
    fn test_centered_rect_basic() {
        let area = Rect::new(0, 0, 100, 40);
        let popup = centered_rect(50, 10, area);

        assert_eq!(popup.width, 50); // 50% of 100
        assert_eq!(popup.height, 10);
        // Horizontal centering: (100 - 50) / 2 = 25
        assert_eq!(popup.x, 25);
        // Should be inside the parent area
        assert!(popup.y >= area.y);
        assert!(popup.y + popup.height <= area.y + area.height);
    }

    #[test]
    fn test_centered_rect_small_area() {
        let area = Rect::new(0, 0, 10, 5);
        let popup = centered_rect(80, 3, area);

        assert_eq!(popup.width, 8); // 80% of 10
        assert_eq!(popup.height, 3);
        assert_eq!(popup.x, 1); // (10 - 8) / 2
        assert_eq!(popup.y, 1); // Min constraints share remaining 2
    }

    #[test]
    fn test_centered_rect_zero_percent_uses_one() {
        let area = Rect::new(0, 0, 100, 40);
        let popup = centered_rect(0, 5, area);

        // 0% percent -> (100 - 0) / 2 = 50% on each side
        assert_eq!(popup.width, 0); // Constraint::Percentage(0)
        assert_eq!(popup.height, 5);
    }

    // ------------------------------------------------------------------
    // draw_command_input
    // ------------------------------------------------------------------

    #[test]
    fn test_draw_command_input_add_server_name() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        app.input_mode = InputMode::AddServer;
        app.add_server_state.step = AddServerStep::Name;
        app.input_buffer = "my-server".to_string();

        terminal
            .draw(|f| draw_command_input(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Add Server"));
        assert!(text.contains("Server Name"));
        assert!(text.contains("my-server"));
    }

    #[test]
    fn test_draw_command_input_add_server_address() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        app.input_mode = InputMode::AddServer;
        app.add_server_state.step = AddServerStep::Address;
        app.input_buffer = "192.168.1.1".to_string();

        terminal
            .draw(|f| draw_command_input(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Server Address"));
        assert!(text.contains("192.168.1.1"));
    }

    #[test]
    fn test_draw_command_input_add_server_port() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        app.input_mode = InputMode::AddServer;
        app.add_server_state.step = AddServerStep::Port;
        app.input_buffer = "50051".to_string();

        terminal
            .draw(|f| draw_command_input(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Port"));
        assert!(text.contains("50051"));
    }

    #[test]
    fn test_draw_command_input_add_server_token() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        app.input_mode = InputMode::AddServer;
        app.add_server_state.step = AddServerStep::Token;
        app.input_buffer = "secret-token".to_string();

        terminal
            .draw(|f| draw_command_input(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Access Token"));
        assert!(text.contains("secret-token"));
    }

    #[test]
    fn test_draw_command_input_add_server_confirm() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        app.input_mode = InputMode::AddServer;
        app.add_server_state.step = AddServerStep::Confirm;
        app.input_buffer = "y".to_string();

        terminal
            .draw(|f| draw_command_input(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Confirm (y/n)"));
        assert!(text.contains("y"));
    }

    #[test]
    fn test_draw_command_input_edit_token() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        app.input_mode = InputMode::EditToken;
        app.input_buffer = "new-token-123".to_string();

        terminal
            .draw(|f| draw_command_input(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Enter Access Token"));
        assert!(text.contains("new-token-123"));
    }

    #[test]
    fn test_draw_command_input_command_filter() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        app.input_mode = InputMode::Command;
        app.current_tab = 2;
        app.input_buffer = "nginx".to_string();

        terminal
            .draw(|f| draw_command_input(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Filter Processes"));
        assert!(text.contains("nginx"));
    }

    #[test]
    fn test_draw_command_input_command_regular() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        app.input_mode = InputMode::Command;
        app.current_tab = 0;
        app.input_buffer = ":reload".to_string();

        terminal
            .draw(|f| draw_command_input(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Command"));
        assert!(text.contains(":reload"));
    }

    #[test]
    fn test_draw_command_input_confirm_disconnect() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        app.input_mode = InputMode::ConfirmDisconnect;

        terminal
            .draw(|f| draw_command_input(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Disconnect from"));
        assert!(text.contains("Test Server"));
        assert!(text.contains("y/n:"));
    }

    #[test]
    fn test_draw_command_input_normal_returns_early() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        app.input_mode = InputMode::Normal;
        app.input_buffer = "should-not-render".to_string();

        terminal
            .draw(|f| draw_command_input(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(!text.contains("should-not-render"));
    }

    #[test]
    fn test_draw_command_input_settings_returns_early() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        app.input_mode = InputMode::Settings;
        app.input_buffer = "should-not-render".to_string();

        terminal
            .draw(|f| draw_command_input(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(!text.contains("should-not-render"));
    }

    // ------------------------------------------------------------------
    // draw_settings_popup
    // ------------------------------------------------------------------

    #[test]
    fn test_draw_settings_popup_ascii() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        app.input_mode = InputMode::Settings;
        app.use_ascii_icons = true;
        app.update_interval = 10;
        app.auto_connect = false;
        app.settings_selection = 0;

        terminal
            .draw(|f| draw_settings_popup(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Settings"));
        assert!(text.contains("Icon Style"));
        assert!(text.contains("ASCII (compatible)"));
        assert!(text.contains("10 seconds"));
        assert!(text.contains("Disabled"));
    }

    #[test]
    fn test_draw_settings_popup_nerd_font() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        app.input_mode = InputMode::Settings;
        app.use_ascii_icons = false;
        app.update_interval = 5;
        app.auto_connect = true;
        app.settings_selection = 1;

        terminal
            .draw(|f| draw_settings_popup(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Nerd Font (fancy)"));
        assert!(text.contains("Enabled"));
        assert!(text.contains("Update Interval"));
    }

    #[test]
    fn test_draw_settings_popup_navigation_hints() {
        let backend = TestBackend::new(120, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();
        app.input_mode = InputMode::Settings;

        terminal
            .draw(|f| draw_settings_popup(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Up/Down: Navigate"));
        assert!(text.contains("Enter: Toggle"));
        assert!(text.contains("Esc: Close"));
    }

    // ------------------------------------------------------------------
    // draw_help_popup
    // ------------------------------------------------------------------

    #[test]
    fn test_draw_help_popup_renders() {
        let backend = TestBackend::new(100, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| draw_help_popup(f, f.size())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Keyboard Shortcuts"));
        assert!(text.contains("Navigation"));
        assert!(text.contains("Server Management"));
        assert!(text.contains("Other"));
        assert!(text.contains("Press any key to close"));
    }

    #[test]
    fn test_draw_help_popup_contains_key_bindings() {
        let backend = TestBackend::new(100, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| draw_help_popup(f, f.size())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("↑/↓ or j/k"));
        assert!(text.contains("Tab"));
        assert!(text.contains("1-8"));
        assert!(text.contains("Add new server"));
        assert!(text.contains("Edit token"));
        assert!(text.contains("Remove selected server"));
        assert!(text.contains("Filter processes"));
        assert!(text.contains("Toggle details panel"));
        assert!(text.contains("Quit"));
    }

    // ------------------------------------------------------------------
    // draw_alerts_popup
    // ------------------------------------------------------------------

    #[test]
    fn test_draw_alerts_popup_empty() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = create_test_app();

        terminal
            .draw(|f| draw_alerts_popup(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Active Alerts"));
        assert!(text.contains("No active alerts"));
    }

    #[test]
    fn test_draw_alerts_popup_with_critical_alert() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();

        // Trigger a critical alert via alert manager
        app.alert_manager.add_rule(shared::alerts::AlertRule {
            id: "test-critical".to_string(),
            name: "Test Critical".to_string(),
            alert_type: shared::alerts::AlertType::CpuHigh,
            severity: shared::alerts::AlertSeverity::Critical,
            enabled: true,
            threshold: 50.0,
            duration_seconds: 0,
            servers: vec![],
            channels: vec![],
        });

        // With duration_seconds=0, min_samples=1
        app.alert_manager.process_metrics("srv-1", "Server 1", 99.0, 0.0, 0.0);

        terminal
            .draw(|f| draw_alerts_popup(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Active Alerts"));
        assert!(text.contains("Critical"));
        assert!(text.contains("srv-1"));
        assert!(text.contains("CPU_HIGH"));
    }

    #[test]
    fn test_draw_alerts_popup_with_multiple_severities() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();

        app.alert_manager.add_rule(shared::alerts::AlertRule {
            id: "test-warning".to_string(),
            name: "Test Warning".to_string(),
            alert_type: shared::alerts::AlertType::MemoryHigh,
            severity: shared::alerts::AlertSeverity::Warning,
            enabled: true,
            threshold: 50.0,
            duration_seconds: 0,
            servers: vec![],
            channels: vec![],
        });

        app.alert_manager.add_rule(shared::alerts::AlertRule {
            id: "test-info".to_string(),
            name: "Test Info".to_string(),
            alert_type: shared::alerts::AlertType::DiskHigh,
            severity: shared::alerts::AlertSeverity::Info,
            enabled: true,
            threshold: 10.0,
            duration_seconds: 0,
            servers: vec![],
            channels: vec![],
        });

        app.alert_manager.process_metrics("srv-1", "Server 1", 0.0, 80.0, 50.0);

        terminal
            .draw(|f| draw_alerts_popup(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Warning"));
        assert!(text.contains("Info"));
    }

    #[test]
    fn test_draw_alerts_popup_selected_index() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = create_test_app();

        app.alert_manager.add_rule(shared::alerts::AlertRule {
            id: "test-alert".to_string(),
            name: "Test Alert".to_string(),
            alert_type: shared::alerts::AlertType::CpuHigh,
            severity: shared::alerts::AlertSeverity::Warning,
            enabled: true,
            threshold: 10.0,
            duration_seconds: 0,
            servers: vec![],
            channels: vec![],
        });

        app.alert_manager.process_metrics("srv-1", "Server 1", 99.0, 0.0, 0.0);

        terminal
            .draw(|f| draw_alerts_popup(f, &app, f.size()))
            .unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains(">")); // selected prefix
        assert!(text.contains("Acknowledge"));
    }
}
