//! User interface components for the monitoring client
//!
//! This module provides terminal UI components for displaying system information
//! Note: This module contains legacy UI code, the dashboard module is used instead

#![allow(dead_code)]
#![allow(unused_imports)]

use anyhow::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use shared::types::*;
use std::io;
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

pub struct UiManager {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    state: UiState,
}

impl UiManager {
    pub fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

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

    pub fn shutdown(mut self) -> Result<()> {
        disable_raw_mode()?;
        execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        self.terminal.show_cursor()?;
        Ok(())
    }
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
