//! User interface components for the monitoring client
//!
//! This module provides terminal UI components for displaying system information
//! Note: This module contains legacy UI code, the dashboard module is used instead

#![allow(dead_code)]
#![allow(unused_imports)]

use anyhow::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
#[cfg(not(test))]
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use shared::types::*;
use std::io;
#[cfg(test)]
use tests::{disable_raw_mode, enable_raw_mode};
use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use tui::Terminal;

pub struct UiState {
    pub servers: Vec<ServerEndpoint>,
    pub selected_server: Option<usize>,
    pub system_info: Vec<SystemInfo>,
    pub show_details: bool,
    pub auto_refresh: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            selected_server: None,
            system_info: Vec::new(),
            show_details: false,
            auto_refresh: true,
        }
    }
}

pub struct UiManager<W: io::Write = io::Stdout> {
    terminal: Terminal<CrosstermBackend<W>>,
    state: UiState,
}

impl UiManager {
    pub fn new() -> Result<Self> {
        Self::new_with(enable_raw_mode, io::stdout, Terminal::new)
    }
}

impl<W: io::Write> UiManager<W> {
    fn new_with(
        enable: impl FnOnce() -> io::Result<()>,
        output: impl FnOnce() -> W,
        create_terminal: impl FnOnce(CrosstermBackend<W>) -> io::Result<Terminal<CrosstermBackend<W>>>,
    ) -> Result<Self> {
        enable()?;
        let mut stdout = output();
        enter_screen(&mut stdout)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = create_terminal(backend)?;
        Ok(Self {
            terminal,
            state: UiState::default(),
        })
    }

    pub fn set_servers(&mut self, servers: Vec<ServerEndpoint>) {
        self.state.servers = servers;
    }

    pub fn set_system_info(&mut self, system_info: Vec<SystemInfo>) {
        self.state.system_info = system_info;
    }

    pub fn shutdown(self) -> Result<()> {
        self.shutdown_with(disable_raw_mode)
    }

    fn shutdown_with(mut self, disable: impl FnOnce() -> io::Result<()>) -> Result<()> {
        disable()?;
        exit_screen(self.terminal.backend_mut())?;
        self.terminal.show_cursor()?;
        Ok(())
    }
}

fn enter_screen(output: &mut impl io::Write) -> io::Result<()> {
    execute!(output, EnterAlternateScreen, EnableMouseCapture)
}

fn exit_screen(output: &mut impl io::Write) -> io::Result<()> {
    execute!(output, LeaveAlternateScreen, DisableMouseCapture)
}

fn create_server_list(state: &UiState) -> List<'_> {
    let items: Vec<ListItem> = state
        .servers
        .iter()
        .map(|server| {
            let content = vec![
                Spans::from(Span::styled(
                    format!("📊 {}", server.name),
                    Style::default().fg(Color::Cyan),
                )),
                Spans::from(Span::styled(
                    format!("   {}:{}", server.address, server.port),
                    Style::default().fg(Color::Gray),
                )),
            ];
            ListItem::new(content)
        })
        .collect();

    List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Servers "))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ")
}

fn create_progress_bar(percentage: f64, width: usize) -> Vec<Span<'static>> {
    let filled = ((percentage / 100.0) * width as f64) as usize;
    let empty = width.saturating_sub(filled);

    let bar = "█".repeat(filled) + &"░".repeat(empty);
    let percentage_str = format!("{:5.1}%", percentage);

    vec![Span::styled(
        format!("   [{}] {}", bar, percentage_str),
        Style::default().fg(Color::White),
    )]
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.1} {}", size, UNITS[unit_index])
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::{
        io::Write,
        sync::{Arc, Mutex},
    };
    use tui::{buffer::Buffer, layout::Rect, widgets::Widget, TerminalOptions, Viewport};

    // Raw-mode SDK calls are replaced at the import boundary in unit builds.
    // No physical terminal mode is changed by either constructor or shutdown.
    pub(super) fn enable_raw_mode() -> io::Result<()> {
        Err(io::Error::other("unit raw-mode refusal"))
    }
    pub(super) fn disable_raw_mode() -> io::Result<()> {
        Ok(())
    }
    fn mode_ok() -> io::Result<()> {
        Ok(())
    }
    fn mode_error() -> io::Result<()> {
        Err(io::Error::other("unit mode failure"))
    }

    #[derive(Default)]
    struct Output {
        bytes: Vec<u8>,
        calls: usize,
        fail_at: Option<usize>,
        fail_flush: bool,
    }
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Output>>);
    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let mut output = self.0.lock().unwrap();
            output.calls += 1;
            if output.fail_at == Some(output.calls) {
                return Err(io::Error::other("unit write failure"));
            }
            output.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.0.lock().unwrap().fail_flush {
                Err(io::Error::other("unit flush failure"))
            } else {
                Ok(())
            }
        }
    }
    type BufferedTerminal = Terminal<CrosstermBackend<Capture>>;
    type Factory = fn(CrosstermBackend<Capture>) -> io::Result<BufferedTerminal>;
    type Mode = fn() -> io::Result<()>;
    fn terminal(backend: CrosstermBackend<Capture>) -> io::Result<BufferedTerminal> {
        Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::fixed(Rect::new(0, 0, 80, 24)),
            },
        )
    }
    fn terminal_error(_backend: CrosstermBackend<Capture>) -> io::Result<BufferedTerminal> {
        Err(io::Error::other("unit terminal failure"))
    }
    fn make(output: Capture, mode: Mode, factory: Factory) -> Result<UiManager<Capture>> {
        UiManager::new_with(mode, move || output, factory)
    }
    fn endpoint(name: &str) -> ServerEndpoint {
        ServerEndpoint {
            id: uuid::Uuid::nil(),
            name: name.into(),
            address: "192.0.2.1".into(),
            port: 50051,
            description: None,
            access_token: None,
        }
    }

    #[test]
    fn default_state_and_data_setters_preserve_supplied_values() {
        let output = Capture::default();
        let mut manager = make(output.clone(), mode_ok, terminal).unwrap();
        assert!(manager.state.servers.is_empty());
        assert!(manager.state.system_info.is_empty());
        assert_eq!(manager.state.selected_server, None);
        assert!(!manager.state.show_details);
        assert!(manager.state.auto_refresh);
        manager.set_servers(vec![endpoint("Unit Server")]);
        assert_eq!(manager.state.servers[0].name, "Unit Server");
        manager.set_system_info(vec![SystemInfo {
            hostname: "unit-host".into(),
            os: "unit-os".into(),
            kernel_version: "unit-kernel".into(),
            uptime_seconds: 10,
            cpu_count: 2,
            cpu_usage_percent: 25.0,
            memory_total_bytes: 1024,
            memory_used_bytes: 256,
            memory_available_bytes: 768,
            disk_info: vec![],
            timestamp: chrono::Utc.timestamp_opt(0, 0).unwrap(),
            swap: SwapInfo::default(),
            memory_pressure: None,
        }]);
        assert_eq!(manager.state.system_info[0].hostname, "unit-host");
        manager.shutdown().unwrap();
        let bytes = output.0.lock().unwrap();
        let text = String::from_utf8_lossy(&bytes.bytes);
        assert!(text.contains("\x1b[?1049h"));
        assert!(text.contains("\x1b[?1049l"));
        assert!(text.contains("\x1b[?25h"));
    }
    #[test]
    fn default_constructor_and_injected_mode_and_terminal_errors_propagate() {
        assert!(UiManager::new()
            .err()
            .unwrap()
            .to_string()
            .contains("unit raw-mode refusal"));
        let output = Capture::default();
        assert!(make(output.clone(), mode_error, terminal).is_err());
        assert_eq!(output.0.lock().unwrap().calls, 0);
        assert!(make(output.clone(), mode_ok, terminal_error).is_err());
        assert!(output.0.lock().unwrap().calls > 0);
    }
    #[test]
    fn constructor_reports_every_screen_write_and_flush_failure() {
        let output = Capture::default();
        let manager = make(output.clone(), mode_ok, terminal).unwrap();
        let calls = output.0.lock().unwrap().calls;
        drop(manager);
        for fail_at in 1..=calls {
            let output = Capture::default();
            output.0.lock().unwrap().fail_at = Some(fail_at);
            assert!(make(output, mode_ok, terminal).is_err());
        }
        let output = Capture::default();
        output.0.lock().unwrap().fail_flush = true;
        assert!(make(output, mode_ok, terminal).is_err());
    }
    #[test]
    fn shutdown_reports_disable_exit_screen_and_cursor_failures() {
        let output = Capture::default();
        let manager = make(output.clone(), mode_ok, terminal).unwrap();
        let before = output.0.lock().unwrap().calls;
        assert!(manager.shutdown_with(mode_error as Mode).is_err());
        assert_eq!(output.0.lock().unwrap().calls, before);
        let output = Capture::default();
        let manager = make(output.clone(), mode_ok, terminal).unwrap();
        output.0.lock().unwrap().fail_at = Some(before + 1);
        assert!(manager.shutdown_with(mode_ok as Mode).is_err());
        let output = Capture::default();
        let manager = make(output.clone(), mode_ok, terminal).unwrap();
        manager.shutdown_with(mode_ok as Mode).unwrap();
        let last = output.0.lock().unwrap().calls;
        let output = Capture::default();
        let manager = make(output.clone(), mode_ok, terminal).unwrap();
        output.0.lock().unwrap().fail_at = Some(last);
        assert!(manager.shutdown_with(mode_ok as Mode).is_err());
    }
    #[test]
    fn exit_sequence_handles_all_writer_and_flush_error_positions() {
        let output = Capture::default();
        exit_screen(&mut CrosstermBackend::new(output.clone())).unwrap();
        let calls = output.0.lock().unwrap().calls;
        for fail_at in 1..=calls {
            let output = Capture::default();
            output.0.lock().unwrap().fail_at = Some(fail_at);
            assert!(exit_screen(&mut CrosstermBackend::new(output)).is_err());
        }
        let output = Capture::default();
        output.0.lock().unwrap().fail_flush = true;
        assert!(exit_screen(&mut CrosstermBackend::new(output)).is_err());
    }
    #[test]
    fn server_list_renders_names_and_addresses_in_an_owned_buffer() {
        let mut state = UiState::default();
        let area = Rect::new(0, 0, 60, 8);
        let mut buffer = Buffer::empty(area);
        create_server_list(&state).render(area, &mut buffer);
        state.servers = vec![endpoint("Alpha"), endpoint("Beta")];
        create_server_list(&state).render(area, &mut buffer);
        let text = buffer
            .content
            .iter()
            .map(|cell| cell.symbol.as_str())
            .collect::<String>();
        assert!(text.contains("Alpha"));
        assert!(text.contains("Beta"));
        assert!(text.contains("192.0.2.1:50051"));
    }
    #[test]
    fn progress_and_byte_formatting_cover_limits_and_existing_overflow_display() {
        for (value, width, expected) in [
            (0.0, 4, "[░░░░]"),
            (50.0, 4, "[██░░]"),
            (100.0, 4, "[████]"),
            (150.0, 4, "[██████]"),
            (50.0, 0, "[]"),
        ] {
            assert!(create_progress_bar(value, width)[0]
                .content
                .contains(expected));
        }
        for (bytes, text) in [
            (0, "0.0 B"),
            (1023, "1023.0 B"),
            (1024, "1.0 KB"),
            (1024_u64.pow(2), "1.0 MB"),
            (1024_u64.pow(3), "1.0 GB"),
            (1024_u64.pow(4), "1.0 TB"),
            (u64::MAX, "16777216.0 TB"),
        ] {
            assert_eq!(format_bytes(bytes), text);
        }
    }
}
