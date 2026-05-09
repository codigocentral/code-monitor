//! Interactive terminal dashboard for monitoring servers
//!
//! This module provides a full-screen terminal UI for managing and monitoring
//! multiple servers with real-time updates.

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use shared::notifications::NotificationConfig;
use shared::types::*;
use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};
use tui::{
    backend::CrosstermBackend,
    widgets::{ListState, TableState},
    Terminal,
};

use crate::client::MonitorClient;
use crate::storage::MetricsStorage;
use shared::alerts::{AlertManager, AlertRule, AlertSeverity, AlertType};

mod data;
mod render;

use data::{connect_to_server, fetch_server_data, handle_command};
use render::draw_ui;

/// Application state for the dashboard
pub struct DashboardApp {
    /// List of configured servers
    servers: Vec<ServerEndpoint>,
    /// Currently selected server index
    selected_server_idx: usize,
    /// Server list selection state
    server_list_state: ListState,
    /// Connected clients by server ID
    connected_clients: HashMap<uuid::Uuid, MonitorClient>,
    /// Cached system info by server ID
    system_info_cache: HashMap<uuid::Uuid, SystemInfo>,
    /// Cached services by server ID
    services_cache: HashMap<uuid::Uuid, Vec<ServiceInfo>>,
    /// Cached processes by server ID
    processes_cache: HashMap<uuid::Uuid, Vec<ProcessInfo>>,
    /// Cached network info by server ID
    network_cache: HashMap<uuid::Uuid, Vec<NetworkInfo>>,
    /// Cached containers by server ID
    containers_cache: HashMap<uuid::Uuid, Vec<ContainerInfo>>,
    /// Cached postgres clusters by server ID
    postgres_cache: HashMap<uuid::Uuid, Vec<shared::types::PostgresClusterInfo>>,
    /// Cached MariaDB clusters by server ID
    mariadb_cache: HashMap<uuid::Uuid, Vec<shared::types::MariaDBClusterInfo>>,
    /// Cached systemd units by server ID
    systemd_cache: HashMap<uuid::Uuid, Vec<shared::types::SystemdUnitInfo>>,
    /// CPU history for sparkline (last 60 values)
    cpu_history: HashMap<uuid::Uuid, Vec<u64>>,
    /// Memory history for sparkline (last 60 values)
    mem_history: HashMap<uuid::Uuid, Vec<u64>>,
    /// Current tab (0: Overview, 1: Services, 2: Processes, 3: Network, 4: Containers, 5: Postgres, 6: MariaDB, 7: Systemd)
    current_tab: usize,
    /// Selected service/process index
    selected_item_idx: usize,
    /// Table state for services/processes
    table_state: TableState,
    /// Whether the app is running
    running: bool,
    /// Status message to display
    status_message: String,
    /// Input mode for commands
    input_mode: InputMode,
    /// Current input string
    input_buffer: String,
    /// Last update time
    last_update: Instant,
    /// Update interval in seconds
    update_interval: u64,
    /// Connection status by server ID
    connection_status: HashMap<uuid::Uuid, ConnectionStatus>,
    /// Show help popup
    show_help: bool,
    /// Process filter string
    process_filter: String,
    /// Show process details panel
    show_details: bool,
    /// Auto-connect on startup
    auto_connect: bool,
    /// Last key press time for debounce
    last_key_time: Instant,
    /// Config manager reference path
    config_path: std::path::PathBuf,
    /// Add server wizard state
    add_server_state: AddServerState,
    /// Current sort column for services/processes
    sort_column: SortColumn,
    /// Current sort order
    sort_order: SortOrder,
    /// Use ASCII fallback for icons (for terminals without Nerd Fonts)
    use_ascii_icons: bool,
    /// Settings menu selection
    settings_selection: usize,
    /// Storage for metrics history
    storage: Option<MetricsStorage>,
    /// Show alerts panel
    show_alerts: bool,
    /// Alert manager for evaluating rules
    alert_manager: AlertManager,
    /// Selected alert index
    selected_alert_idx: usize,
    /// Notification dispatcher for sending alerts
    notification_dispatcher: shared::notifications::NotificationDispatcher,
    /// TLS configuration for connections
    tls_config: Option<shared::types::ClientTlsConfig>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Command,
    /// Adding a new server - step by step
    AddServer,
    /// Editing token for selected server
    EditToken,
    /// Confirming disconnect
    ConfirmDisconnect,
    /// Settings menu
    Settings,
}

/// Sorting options for lists
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SortColumn {
    #[default]
    Name,
    Status,
    Pid,
    Cpu,
    Memory,
    Uptime,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SortOrder {
    #[default]
    Ascending,
    Descending,
}

/// State for add server wizard
#[derive(Debug, Clone, Default)]
pub struct AddServerState {
    pub step: AddServerStep,
    pub name: String,
    pub address: String,
    pub port: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum AddServerStep {
    #[default]
    Name,
    Address,
    Port,
    Token,
    Confirm,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

impl DashboardApp {
    pub fn new(
        servers: Vec<ServerEndpoint>,
        notifications: NotificationConfig,
        tls_config: Option<shared::types::ClientTlsConfig>,
    ) -> Self {
        let mut server_list_state = ListState::default();
        if !servers.is_empty() {
            server_list_state.select(Some(0));
        }

        let connection_status: HashMap<uuid::Uuid, ConnectionStatus> = servers
            .iter()
            .map(|s| (s.id, ConnectionStatus::Disconnected))
            .collect();

        // Initialize storage
        let storage = MetricsStorage::new("code-monitor.db")
            .map_err(|e| {
                tracing::warn!("Failed to initialize storage: {}", e);
            })
            .ok();

        let mut app = Self {
            servers,
            selected_server_idx: 0,
            server_list_state,
            connected_clients: HashMap::new(),
            system_info_cache: HashMap::new(),
            services_cache: HashMap::new(),
            processes_cache: HashMap::new(),
            network_cache: HashMap::new(),
            cpu_history: HashMap::new(),
            mem_history: HashMap::new(),
            current_tab: 0,
            containers_cache: HashMap::new(),
            postgres_cache: HashMap::new(),
            mariadb_cache: HashMap::new(),
            systemd_cache: HashMap::new(),
            selected_item_idx: 0,
            table_state: TableState::default(),
            running: true,
            status_message: String::new(),
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            last_update: Instant::now(),
            update_interval: 5,
            connection_status,
            show_help: false,
            process_filter: String::new(),
            show_details: false,
            auto_connect: true,
            last_key_time: Instant::now(),
            config_path: std::path::PathBuf::from("client-config.toml"),
            add_server_state: AddServerState::default(),
            sort_column: SortColumn::default(),
            sort_order: SortOrder::default(),
            use_ascii_icons: !Self::detect_nerd_font_support(),
            settings_selection: 0,
            storage,
            show_alerts: false,
            alert_manager: AlertManager::new(),
            selected_alert_idx: 0,
            notification_dispatcher: notifications.build_dispatcher(),
            tls_config,
        };

        // Add default alert rules
        app.setup_default_alerts();

        app
    }

    fn setup_default_alerts(&mut self) {
        // CPU high alert - 80% for 3 samples (15 seconds at 5s interval)
        self.alert_manager.add_rule(AlertRule {
            id: "cpu-high".to_string(),
            name: "High CPU Usage".to_string(),
            alert_type: AlertType::CpuHigh,
            severity: AlertSeverity::Warning,
            enabled: true,
            threshold: 80.0,
            duration_seconds: 15,
            servers: vec![],
            channels: vec![],
        });

        // Memory high alert - 90% for 3 samples
        self.alert_manager.add_rule(AlertRule {
            id: "mem-high".to_string(),
            name: "High Memory Usage".to_string(),
            alert_type: AlertType::MemoryHigh,
            severity: AlertSeverity::Warning,
            enabled: true,
            threshold: 90.0,
            duration_seconds: 15,
            servers: vec![],
            channels: vec![],
        });

        // Disk high alert - 85% usage
        self.alert_manager.add_rule(AlertRule {
            id: "disk-high".to_string(),
            name: "High Disk Usage".to_string(),
            alert_type: AlertType::DiskHigh,
            severity: AlertSeverity::Warning,
            enabled: true,
            threshold: 85.0,
            duration_seconds: 0,
            servers: vec![],
            channels: vec![],
        });

        // Server down alert
        self.alert_manager.add_rule(AlertRule {
            id: "server-down".to_string(),
            name: "Server Down".to_string(),
            alert_type: AlertType::ServerDown,
            severity: AlertSeverity::Critical,
            enabled: true,
            threshold: 30.0, // timeout in seconds
            duration_seconds: 0,
            servers: vec![],
            channels: vec![],
        });
    }

    /// Detect if terminal likely supports Nerd Fonts
    fn detect_nerd_font_support() -> bool {
        // Check for common environment indicators
        // VSCode terminal usually supports icons, Windows Terminal depends on font
        if std::env::var("TERM_PROGRAM")
            .map(|v| v.contains("vscode"))
            .unwrap_or(false)
        {
            return true;
        }
        // Windows Terminal with proper font config
        if std::env::var("WT_SESSION").is_ok() {
            return true; // Assume configured correctly
        }
        // Default to ASCII for safety on Windows cmd/PowerShell
        #[cfg(windows)]
        {
            false
        }
        #[cfg(not(windows))]
        {
            true // Most Linux terminals support it
        }
    }

    #[allow(dead_code)]
    pub fn with_config_path(mut self, path: std::path::PathBuf) -> Self {
        self.config_path = path;
        self
    }

    /// Get icon with ASCII fallback
    #[allow(dead_code)]
    pub fn icon<'a>(&self, nerd: &'a str, ascii: &'a str) -> &'a str {
        if self.use_ascii_icons {
            ascii
        } else {
            nerd
        }
    }

    /// Toggle sort order or change column
    #[allow(dead_code)]
    pub fn toggle_sort(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_order = match self.sort_order {
                SortOrder::Ascending => SortOrder::Descending,
                SortOrder::Descending => SortOrder::Ascending,
            };
        } else {
            self.sort_column = column;
            self.sort_order = SortOrder::Ascending;
        }
    }

    pub fn get_selected_server(&self) -> Option<&ServerEndpoint> {
        self.servers.get(self.selected_server_idx)
    }

    pub fn get_selected_server_id(&self) -> Option<uuid::Uuid> {
        self.get_selected_server().map(|s| s.id)
    }

    pub fn is_connected(&self, server_id: &uuid::Uuid) -> bool {
        matches!(
            self.connection_status.get(server_id),
            Some(ConnectionStatus::Connected)
        )
    }

    fn next_server(&mut self) {
        if !self.servers.is_empty() {
            self.selected_server_idx = (self.selected_server_idx + 1) % self.servers.len();
            self.server_list_state
                .select(Some(self.selected_server_idx));
        }
    }

    fn previous_server(&mut self) {
        if !self.servers.is_empty() {
            if self.selected_server_idx == 0 {
                self.selected_server_idx = self.servers.len() - 1;
            } else {
                self.selected_server_idx -= 1;
            }
            self.server_list_state
                .select(Some(self.selected_server_idx));
        }
    }

    fn next_tab(&mut self) {
        self.current_tab = (self.current_tab + 1) % 8;
        self.selected_item_idx = 0;
        self.table_state.select(Some(0));
    }

    fn previous_tab(&mut self) {
        if self.current_tab == 0 {
            self.current_tab = 7;
        } else {
            self.current_tab -= 1;
        }
        self.selected_item_idx = 0;
        self.table_state.select(Some(0));
    }

    fn next_item(&mut self) {
        let max_items = self.get_current_list_len();
        if max_items > 0 {
            self.selected_item_idx = (self.selected_item_idx + 1) % max_items;
            self.table_state.select(Some(self.selected_item_idx));
        }
    }

    fn previous_item(&mut self) {
        let max_items = self.get_current_list_len();
        if max_items > 0 {
            if self.selected_item_idx == 0 {
                self.selected_item_idx = max_items - 1;
            } else {
                self.selected_item_idx -= 1;
            }
            self.table_state.select(Some(self.selected_item_idx));
        }
    }

    fn get_current_list_len(&self) -> usize {
        if let Some(server_id) = self.get_selected_server_id() {
            match self.current_tab {
                1 => self
                    .services_cache
                    .get(&server_id)
                    .map(|s| s.len())
                    .unwrap_or(0),
                2 => self.get_filtered_processes().len(),
                3 => self
                    .network_cache
                    .get(&server_id)
                    .map(|n| n.len())
                    .unwrap_or(0),
                4 => self
                    .containers_cache
                    .get(&server_id)
                    .map(|c| c.len())
                    .unwrap_or(0),
                5 => self
                    .postgres_cache
                    .get(&server_id)
                    .map(|c| c.len())
                    .unwrap_or(0),
                6 => self
                    .mariadb_cache
                    .get(&server_id)
                    .map(|c| c.len())
                    .unwrap_or(0),
                7 => self
                    .systemd_cache
                    .get(&server_id)
                    .map(|c| c.len())
                    .unwrap_or(0),
                _ => 0,
            }
        } else {
            0
        }
    }

    fn get_filtered_processes(&self) -> Vec<&ProcessInfo> {
        if let Some(server_id) = self.get_selected_server_id() {
            if let Some(processes) = self.processes_cache.get(&server_id) {
                if self.process_filter.is_empty() {
                    return processes.iter().collect();
                }
                let filter_lower = self.process_filter.to_lowercase();
                return processes
                    .iter()
                    .filter(|p| {
                        p.name.to_lowercase().contains(&filter_lower)
                            || p.command_line.to_lowercase().contains(&filter_lower)
                    })
                    .collect();
            }
        }
        Vec::new()
    }

    fn get_selected_process(&self) -> Option<&ProcessInfo> {
        let processes = self.get_filtered_processes();
        processes.get(self.selected_item_idx).copied()
    }

    fn update_history(&mut self, server_id: uuid::Uuid, cpu: f64, mem_percent: f64) {
        // Update CPU history
        let cpu_entry = self.cpu_history.entry(server_id).or_default();
        cpu_entry.push(cpu.clamp(0.0, 100.0) as u64);
        if cpu_entry.len() > 60 {
            cpu_entry.remove(0);
        }

        // Update Memory history
        let mem_entry = self.mem_history.entry(server_id).or_default();
        mem_entry.push(mem_percent.clamp(0.0, 100.0) as u64);
        if mem_entry.len() > 60 {
            mem_entry.remove(0);
        }
    }
}

/// Run the interactive dashboard
pub async fn run_dashboard(
    servers: Vec<ServerEndpoint>,
    update_interval: u64,
    notifications: NotificationConfig,
    tls_config: Option<shared::types::ClientTlsConfig>,
) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = DashboardApp::new(servers.clone(), notifications, tls_config.clone());
    app.update_interval = update_interval;

    // Auto-connect to all servers on startup
    if app.auto_connect {
        for server in &servers {
            app.status_message = format!("🔄 Connecting to {}...", server.name);
            app.connection_status
                .insert(server.id, ConnectionStatus::Connecting);
            terminal.draw(|f| draw_ui(f, &app))?;

            match connect_to_server(server, update_interval, app.tls_config.as_ref()).await {
                Ok(client) => {
                    app.connected_clients.insert(server.id, client);
                    app.connection_status
                        .insert(server.id, ConnectionStatus::Connected);
                    let _ = fetch_server_data(&mut app, server.id).await;
                }
                Err(e) => {
                    app.connection_status
                        .insert(server.id, ConnectionStatus::Error(e.to_string()));
                }
            }
        }
        app.status_message = format!("✅ Connected to {} server(s)", app.connected_clients.len());
    }

    // Run the main loop
    let result = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_app<B: tui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut DashboardApp,
) -> Result<()> {
    let tick_rate = Duration::from_millis(100);
    let debounce_duration = Duration::from_millis(50);

    loop {
        terminal.draw(|f| draw_ui(f, app))?;

        // Handle events with timeout
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                // Only process key press events, ignore key release and repeat
                // On Windows, key events can fire multiple times (Press, Repeat, Release)
                match key.kind {
                    crossterm::event::KeyEventKind::Press => {}
                    _ => continue, // Skip Release and Repeat events
                }

                // Debounce for Tab key to prevent double-firing
                let now = Instant::now();
                if matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
                    && now.duration_since(app.last_key_time) < debounce_duration
                {
                    continue; // Skip if too soon after last key
                }
                app.last_key_time = now;

                // Handle help popup first
                if app.show_help {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('h') | KeyCode::Char('?') | KeyCode::Enter => {
                            app.show_help = false;
                        }
                        _ => {}
                    }
                    continue;
                }

                // Handle alerts popup
                if app.show_alerts {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('A') | KeyCode::Enter => {
                            app.show_alerts = false;
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            let active_count = app.alert_manager.get_active_alerts().len();
                            if app.selected_alert_idx > 0 {
                                app.selected_alert_idx -= 1;
                            }
                            // Also update app.active_alerts selected index for rendering
                            if app.selected_alert_idx >= active_count && active_count > 0 {
                                app.selected_alert_idx = active_count - 1;
                            }
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            let active_count = app.alert_manager.get_active_alerts().len();
                            if app.selected_alert_idx + 1 < active_count {
                                app.selected_alert_idx += 1;
                            }
                        }
                        KeyCode::Char('a') => {
                            // Acknowledge selected alert
                            let active = app.alert_manager.get_active_alerts();
                            if app.selected_alert_idx < active.len() {
                                let alert_id = active[app.selected_alert_idx].id;
                                app.alert_manager
                                    .acknowledge_alert(alert_id, "user".to_string());
                                app.status_message = "✅ Alert acknowledged".to_string();
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                match app.input_mode {
                    InputMode::Normal => {
                        match key.code {
                            // Quit
                            KeyCode::Char('q') | KeyCode::Esc => {
                                app.running = false;
                            }
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.running = false;
                            }
                            // Help
                            KeyCode::Char('h') | KeyCode::Char('?') => {
                                app.show_help = true;
                            }
                            // Connect/Disconnect
                            KeyCode::Enter => {
                                if let Some(server) = app.get_selected_server().cloned() {
                                    if app.is_connected(&server.id) {
                                        // Ask for confirmation before disconnecting
                                        app.input_mode = InputMode::ConfirmDisconnect;
                                        app.status_message =
                                            format!("Disconnect from '{}'? (y/n)", server.name);
                                    } else {
                                        // Connect
                                        app.status_message =
                                            format!("Connecting to {}...", server.name);
                                        app.connection_status
                                            .insert(server.id, ConnectionStatus::Connecting);

                                        match connect_to_server(
                                            &server,
                                            app.update_interval,
                                            app.tls_config.as_ref(),
                                        )
                                        .await
                                        {
                                            Ok(client) => {
                                                app.connected_clients.insert(server.id, client);
                                                app.connection_status
                                                    .insert(server.id, ConnectionStatus::Connected);
                                                app.status_message =
                                                    format!("Connected to {}", server.name);
                                                let _ = fetch_server_data(app, server.id).await;
                                            }
                                            Err(e) => {
                                                app.connection_status.insert(
                                                    server.id,
                                                    ConnectionStatus::Error(e.to_string()),
                                                );
                                                app.status_message = format!("Failed: {}", e);
                                            }
                                        }
                                    }
                                }
                            }
                            // Settings menu
                            KeyCode::Char('s') => {
                                app.input_mode = InputMode::Settings;
                                app.settings_selection = 0;
                                app.status_message = "Settings".to_string();
                            }
                            // Sort by column (in Services/Processes tabs)
                            KeyCode::Char('o') => {
                                if app.current_tab == 1 || app.current_tab == 2 {
                                    // Cycle through sort columns
                                    app.sort_column = match app.sort_column {
                                        SortColumn::Name => SortColumn::Cpu,
                                        SortColumn::Cpu => SortColumn::Memory,
                                        SortColumn::Memory => SortColumn::Name,
                                        _ => SortColumn::Name,
                                    };
                                    app.status_message = format!("Sorted by {:?}", app.sort_column);
                                }
                            }
                            // Reverse sort order
                            KeyCode::Char('O') => {
                                if app.current_tab == 1 || app.current_tab == 2 {
                                    app.sort_order = match app.sort_order {
                                        SortOrder::Ascending => SortOrder::Descending,
                                        SortOrder::Descending => SortOrder::Ascending,
                                    };
                                    app.status_message =
                                        format!("Sort order: {:?}", app.sort_order);
                                }
                            }
                            // Connect all
                            KeyCode::Char('C') => {
                                let servers_clone: Vec<_> = app.servers.clone();
                                for server in &servers_clone {
                                    if !app.is_connected(&server.id) {
                                        app.status_message =
                                            format!("🔄 Connecting to {}...", server.name);
                                        app.connection_status
                                            .insert(server.id, ConnectionStatus::Connecting);

                                        match connect_to_server(
                                            server,
                                            app.update_interval,
                                            app.tls_config.as_ref(),
                                        )
                                        .await
                                        {
                                            Ok(client) => {
                                                app.connected_clients.insert(server.id, client);
                                                app.connection_status
                                                    .insert(server.id, ConnectionStatus::Connected);
                                                let _ = fetch_server_data(app, server.id).await;
                                            }
                                            Err(e) => {
                                                app.connection_status.insert(
                                                    server.id,
                                                    ConnectionStatus::Error(e.to_string()),
                                                );
                                            }
                                        }
                                    }
                                }
                                app.status_message = format!(
                                    "✅ Connected to {} server(s)",
                                    app.connected_clients.len()
                                );
                            }
                            // Disconnect all
                            KeyCode::Char('D') => {
                                let server_ids: Vec<_> =
                                    app.connected_clients.keys().cloned().collect();
                                for server_id in server_ids {
                                    app.connected_clients.remove(&server_id);
                                    app.system_info_cache.remove(&server_id);
                                    app.services_cache.remove(&server_id);
                                    app.processes_cache.remove(&server_id);
                                    app.network_cache.remove(&server_id);
                                    app.cpu_history.remove(&server_id);
                                    app.mem_history.remove(&server_id);
                                    app.connection_status
                                        .insert(server_id, ConnectionStatus::Disconnected);
                                }
                                app.status_message = "🔌 Disconnected from all servers".to_string();
                            }
                            // Refresh
                            KeyCode::Char('r') => {
                                if let Some(server_id) = app.get_selected_server_id() {
                                    if app.is_connected(&server_id) {
                                        app.status_message = "🔄 Refreshing...".to_string();
                                        if let Err(e) = fetch_server_data(app, server_id).await {
                                            app.status_message =
                                                format!("❌ Refresh failed: {}", e);
                                        } else {
                                            app.status_message = "✅ Data refreshed".to_string();
                                        }
                                    }
                                }
                            }
                            // Refresh all
                            KeyCode::Char('R') => {
                                let server_ids: Vec<_> =
                                    app.connected_clients.keys().cloned().collect();
                                for server_id in server_ids {
                                    let _ = fetch_server_data(app, server_id).await;
                                }
                                app.status_message = format!(
                                    "✅ Refreshed {} server(s)",
                                    app.connected_clients.len()
                                );
                            }
                            // Tab navigation
                            KeyCode::Tab => {
                                app.next_tab();
                            }
                            KeyCode::BackTab => {
                                app.previous_tab();
                            }
                            // Number keys for tabs
                            KeyCode::Char('1') => {
                                app.current_tab = 0;
                                app.selected_item_idx = 0;
                            }
                            KeyCode::Char('2') => {
                                app.current_tab = 1;
                                app.selected_item_idx = 0;
                                app.table_state.select(Some(0));
                            }
                            KeyCode::Char('3') => {
                                app.current_tab = 2;
                                app.selected_item_idx = 0;
                                app.table_state.select(Some(0));
                            }
                            KeyCode::Char('4') => {
                                app.current_tab = 3;
                                app.selected_item_idx = 0;
                                app.table_state.select(Some(0));
                            }
                            KeyCode::Char('5') => {
                                app.current_tab = 4;
                                app.selected_item_idx = 0;
                                app.table_state.select(Some(0));
                            }
                            KeyCode::Char('6') => {
                                app.current_tab = 5;
                                app.selected_item_idx = 0;
                                app.table_state.select(Some(0));
                            }
                            KeyCode::Char('7') => {
                                app.current_tab = 6;
                                app.selected_item_idx = 0;
                                app.table_state.select(Some(0));
                            }
                            KeyCode::Char('8') => {
                                app.current_tab = 7;
                                app.selected_item_idx = 0;
                                app.table_state.select(Some(0));
                            }
                            // Server navigation (j/k or arrows in overview)
                            KeyCode::Char('j') | KeyCode::Down => {
                                if app.current_tab == 0 {
                                    app.next_server();
                                    // Auto-connect to the new selected server
                                    if let Some(server) = app.get_selected_server().cloned() {
                                        if !app.connected_clients.contains_key(&server.id) {
                                            app.status_message =
                                                format!("🔄 Connecting to {}...", server.name);
                                            app.connection_status
                                                .insert(server.id, ConnectionStatus::Connecting);
                                            match connect_to_server(
                                                &server,
                                                app.update_interval,
                                                app.tls_config.as_ref(),
                                            )
                                            .await
                                            {
                                                Ok(client) => {
                                                    app.connected_clients.insert(server.id, client);
                                                    app.connection_status.insert(
                                                        server.id,
                                                        ConnectionStatus::Connected,
                                                    );
                                                    app.status_message =
                                                        format!("✅ Connected to {}", server.name);
                                                    let _ = fetch_server_data(app, server.id).await;
                                                }
                                                Err(e) => {
                                                    app.connection_status.insert(
                                                        server.id,
                                                        ConnectionStatus::Error(e.to_string()),
                                                    );
                                                    app.status_message =
                                                        format!("❌ {}: {}", server.name, e);
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    app.next_item();
                                }
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                if app.current_tab == 0 {
                                    app.previous_server();
                                    // Auto-connect to the new selected server
                                    if let Some(server) = app.get_selected_server().cloned() {
                                        if !app.connected_clients.contains_key(&server.id) {
                                            app.status_message =
                                                format!("🔄 Connecting to {}...", server.name);
                                            app.connection_status
                                                .insert(server.id, ConnectionStatus::Connecting);
                                            match connect_to_server(
                                                &server,
                                                app.update_interval,
                                                app.tls_config.as_ref(),
                                            )
                                            .await
                                            {
                                                Ok(client) => {
                                                    app.connected_clients.insert(server.id, client);
                                                    app.connection_status.insert(
                                                        server.id,
                                                        ConnectionStatus::Connected,
                                                    );
                                                    app.status_message =
                                                        format!("✅ Connected to {}", server.name);
                                                    let _ = fetch_server_data(app, server.id).await;
                                                }
                                                Err(e) => {
                                                    app.connection_status.insert(
                                                        server.id,
                                                        ConnectionStatus::Error(e.to_string()),
                                                    );
                                                    app.status_message =
                                                        format!("❌ {}: {}", server.name, e);
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    app.previous_item();
                                }
                            }
                            // Left/Right for servers regardless of tab
                            KeyCode::Left => {
                                app.previous_server();
                                // Auto-connect to the new selected server
                                if let Some(server) = app.get_selected_server().cloned() {
                                    if !app.connected_clients.contains_key(&server.id) {
                                        app.status_message =
                                            format!("🔄 Connecting to {}...", server.name);
                                        app.connection_status
                                            .insert(server.id, ConnectionStatus::Connecting);
                                        match connect_to_server(
                                            &server,
                                            app.update_interval,
                                            app.tls_config.as_ref(),
                                        )
                                        .await
                                        {
                                            Ok(client) => {
                                                app.connected_clients.insert(server.id, client);
                                                app.connection_status
                                                    .insert(server.id, ConnectionStatus::Connected);
                                                app.status_message =
                                                    format!("✅ Connected to {}", server.name);
                                                let _ = fetch_server_data(app, server.id).await;
                                            }
                                            Err(e) => {
                                                app.connection_status.insert(
                                                    server.id,
                                                    ConnectionStatus::Error(e.to_string()),
                                                );
                                                app.status_message =
                                                    format!("❌ {}: {}", server.name, e);
                                            }
                                        }
                                    }
                                }
                            }
                            KeyCode::Right => {
                                app.next_server();
                                // Auto-connect to the new selected server
                                if let Some(server) = app.get_selected_server().cloned() {
                                    if !app.connected_clients.contains_key(&server.id) {
                                        app.status_message =
                                            format!("🔄 Connecting to {}...", server.name);
                                        app.connection_status
                                            .insert(server.id, ConnectionStatus::Connecting);
                                        match connect_to_server(
                                            &server,
                                            app.update_interval,
                                            app.tls_config.as_ref(),
                                        )
                                        .await
                                        {
                                            Ok(client) => {
                                                app.connected_clients.insert(server.id, client);
                                                app.connection_status
                                                    .insert(server.id, ConnectionStatus::Connected);
                                                app.status_message =
                                                    format!("✅ Connected to {}", server.name);
                                                let _ = fetch_server_data(app, server.id).await;
                                            }
                                            Err(e) => {
                                                app.connection_status.insert(
                                                    server.id,
                                                    ConnectionStatus::Error(e.to_string()),
                                                );
                                                app.status_message =
                                                    format!("❌ {}: {}", server.name, e);
                                            }
                                        }
                                    }
                                }
                            }
                            // Toggle details panel
                            KeyCode::Char('d') => {
                                app.show_details = !app.show_details;
                            }
                            // Filter (in processes tab)
                            KeyCode::Char('/') => {
                                if app.current_tab == 2 {
                                    app.input_mode = InputMode::Command;
                                    app.input_buffer = app.process_filter.clone();
                                    app.status_message =
                                        "🔍 Type filter, press Enter to apply, Esc to cancel"
                                            .to_string();
                                }
                            }
                            // Clear filter
                            KeyCode::Char('x') => {
                                app.process_filter.clear();
                                app.selected_item_idx = 0;
                                app.status_message = "🧹 Filter cleared".to_string();
                            }
                            // Command mode
                            KeyCode::Char(':') => {
                                app.input_mode = InputMode::Command;
                                app.input_buffer.clear();
                            }
                            // Home/End for fast navigation
                            KeyCode::Home => {
                                app.selected_item_idx = 0;
                                app.table_state.select(Some(0));
                            }
                            KeyCode::End => {
                                let max = app.get_current_list_len().saturating_sub(1);
                                app.selected_item_idx = max;
                                app.table_state.select(Some(max));
                            }
                            // Page up/down
                            KeyCode::PageDown => {
                                let max = app.get_current_list_len();
                                app.selected_item_idx =
                                    (app.selected_item_idx + 10).min(max.saturating_sub(1));
                                app.table_state.select(Some(app.selected_item_idx));
                            }
                            KeyCode::PageUp => {
                                app.selected_item_idx = app.selected_item_idx.saturating_sub(10);
                                app.table_state.select(Some(app.selected_item_idx));
                            }
                            // Add new server
                            KeyCode::Char('a') => {
                                app.add_server_state = AddServerState::default();
                                app.add_server_state.port = "50051".to_string();
                                app.input_mode = InputMode::AddServer;
                                app.input_buffer.clear();
                                app.status_message = "➕ Add Server - Enter name:".to_string();
                            }
                            // Edit token for selected server
                            KeyCode::Char('t') => {
                                if app.get_selected_server().is_some() {
                                    app.input_mode = InputMode::EditToken;
                                    app.input_buffer.clear();
                                    app.status_message =
                                        "🔑 Enter access token (from server):".to_string();
                                }
                            }
                            // Remove selected server
                            KeyCode::Delete => {
                                if let Some(server) = app.get_selected_server().cloned() {
                                    // Disconnect first if connected
                                    app.connected_clients.remove(&server.id);
                                    app.system_info_cache.remove(&server.id);
                                    app.services_cache.remove(&server.id);
                                    app.processes_cache.remove(&server.id);
                                    app.network_cache.remove(&server.id);
                                    app.cpu_history.remove(&server.id);
                                    app.mem_history.remove(&server.id);
                                    app.connection_status.remove(&server.id);

                                    // Remove from list
                                    app.servers.retain(|s| s.id != server.id);

                                    // Update selection
                                    if app.selected_server_idx >= app.servers.len()
                                        && !app.servers.is_empty()
                                    {
                                        app.selected_server_idx = app.servers.len() - 1;
                                    }
                                    app.server_list_state.select(if app.servers.is_empty() {
                                        None
                                    } else {
                                        Some(app.selected_server_idx)
                                    });

                                    // Save config
                                    if let Ok(mut manager) =
                                        crate::config::ClientConfigManager::new(&app.config_path)
                                    {
                                        let _ = manager.remove_server(server.id);
                                    }

                                    app.status_message =
                                        format!("🗑️ Removed server '{}'", server.name);
                                }
                            }
                            // Show alerts panel
                            KeyCode::Char('A') => {
                                app.show_alerts = !app.show_alerts;
                                app.selected_alert_idx = 0;
                            }
                            _ => {}
                        }
                    }
                    InputMode::AddServer => {
                        match key.code {
                            KeyCode::Enter => {
                                match app.add_server_state.step {
                                    AddServerStep::Name => {
                                        app.add_server_state.name = app.input_buffer.clone();
                                        app.input_buffer.clear();
                                        app.add_server_state.step = AddServerStep::Address;
                                        app.status_message =
                                            "➕ Enter server address (IP or hostname):".to_string();
                                    }
                                    AddServerStep::Address => {
                                        app.add_server_state.address = app.input_buffer.clone();
                                        app.input_buffer = app.add_server_state.port.clone();
                                        app.add_server_state.step = AddServerStep::Port;
                                        app.status_message =
                                            "➕ Enter port (default 50051):".to_string();
                                    }
                                    AddServerStep::Port => {
                                        app.add_server_state.port = if app.input_buffer.is_empty() {
                                            "50051".to_string()
                                        } else {
                                            app.input_buffer.clone()
                                        };
                                        app.input_buffer.clear();
                                        app.add_server_state.step = AddServerStep::Token;
                                        app.status_message =
                                            "➕ Enter access token (or leave empty):".to_string();
                                    }
                                    AddServerStep::Token => {
                                        app.add_server_state.token = app.input_buffer.clone();
                                        app.add_server_state.step = AddServerStep::Confirm;
                                        let state = &app.add_server_state;
                                        app.status_message = format!(
                                            "➕ Add '{}' at {}:{}? (y/n)",
                                            state.name, state.address, state.port
                                        );
                                        app.input_buffer.clear();
                                    }
                                    AddServerStep::Confirm => {
                                        if app.input_buffer.to_lowercase() == "y" {
                                            // Create new server
                                            let port: u16 =
                                                app.add_server_state.port.parse().unwrap_or(50051);
                                            let new_server = ServerEndpoint {
                                                id: uuid::Uuid::new_v4(),
                                                name: app.add_server_state.name.clone(),
                                                address: app.add_server_state.address.clone(),
                                                port,
                                                description: None,
                                                access_token: if app
                                                    .add_server_state
                                                    .token
                                                    .is_empty()
                                                {
                                                    None
                                                } else {
                                                    Some(app.add_server_state.token.clone())
                                                },
                                            };

                                            // Save to config
                                            if let Ok(mut manager) =
                                                crate::config::ClientConfigManager::new(
                                                    &app.config_path,
                                                )
                                            {
                                                let _ = manager.add_server(new_server.clone());
                                            }

                                            // Add to app
                                            app.connection_status.insert(
                                                new_server.id,
                                                ConnectionStatus::Disconnected,
                                            );
                                            app.servers.push(new_server.clone());
                                            app.selected_server_idx = app.servers.len() - 1;
                                            app.server_list_state
                                                .select(Some(app.selected_server_idx));

                                            app.status_message =
                                                format!("✅ Added server '{}'", new_server.name);
                                        } else {
                                            app.status_message =
                                                "❌ Cancelled adding server".to_string();
                                        }
                                        app.input_mode = InputMode::Normal;
                                        app.add_server_state = AddServerState::default();
                                        app.input_buffer.clear();
                                    }
                                }
                            }
                            KeyCode::Esc => {
                                app.input_mode = InputMode::Normal;
                                app.add_server_state = AddServerState::default();
                                app.input_buffer.clear();
                                app.status_message = "❌ Cancelled".to_string();
                            }
                            KeyCode::Char(c) => {
                                app.input_buffer.push(c);
                            }
                            KeyCode::Backspace => {
                                app.input_buffer.pop();
                            }
                            _ => {}
                        }
                    }
                    InputMode::EditToken => {
                        match key.code {
                            KeyCode::Enter => {
                                if let Some(server) = app.get_selected_server().cloned() {
                                    let token = if app.input_buffer.is_empty() {
                                        None
                                    } else {
                                        Some(app.input_buffer.clone())
                                    };

                                    // Update in servers list
                                    if let Some(s) =
                                        app.servers.iter_mut().find(|s| s.id == server.id)
                                    {
                                        s.access_token = token.clone();
                                    }

                                    // Save to config
                                    if let Ok(manager) =
                                        crate::config::ClientConfigManager::new(&app.config_path)
                                    {
                                        if let Ok(mut config) = manager.load_config() {
                                            if let Some(s) = config
                                                .servers
                                                .iter_mut()
                                                .find(|s| s.id == server.id)
                                            {
                                                s.access_token = token;
                                                let _ = manager.save_config(&config);
                                            }
                                        }
                                    }

                                    app.status_message =
                                        format!("✅ Token updated for '{}'", server.name);
                                }
                                app.input_mode = InputMode::Normal;
                                app.input_buffer.clear();
                            }
                            KeyCode::Esc => {
                                app.input_mode = InputMode::Normal;
                                app.input_buffer.clear();
                                app.status_message = "❌ Cancelled".to_string();
                            }
                            KeyCode::Char(c) => {
                                app.input_buffer.push(c);
                            }
                            KeyCode::Backspace => {
                                app.input_buffer.pop();
                            }
                            _ => {}
                        }
                    }
                    InputMode::Command => {
                        match key.code {
                            KeyCode::Enter => {
                                let command = app.input_buffer.clone();
                                app.input_buffer.clear();
                                app.input_mode = InputMode::Normal;
                                // Check if it's a filter (from / key)
                                if app.current_tab == 2 && !command.starts_with(':') {
                                    app.process_filter = command;
                                    app.selected_item_idx = 0;
                                    app.status_message = format!("Filter: {}", app.process_filter);
                                } else {
                                    handle_command(app, &command).await;
                                }
                            }
                            KeyCode::Esc => {
                                app.input_buffer.clear();
                                app.input_mode = InputMode::Normal;
                                app.status_message.clear();
                            }
                            KeyCode::Char(c) => {
                                app.input_buffer.push(c);
                            }
                            KeyCode::Backspace => {
                                app.input_buffer.pop();
                            }
                            _ => {}
                        }
                    }
                    InputMode::ConfirmDisconnect => {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                if let Some(server) = app.get_selected_server().cloned() {
                                    // Disconnect confirmed
                                    app.connected_clients.remove(&server.id);
                                    app.system_info_cache.remove(&server.id);
                                    app.services_cache.remove(&server.id);
                                    app.processes_cache.remove(&server.id);
                                    app.network_cache.remove(&server.id);
                                    app.cpu_history.remove(&server.id);
                                    app.mem_history.remove(&server.id);
                                    app.connection_status
                                        .insert(server.id, ConnectionStatus::Disconnected);
                                    app.status_message =
                                        format!("Disconnected from {}", server.name);
                                }
                                app.input_mode = InputMode::Normal;
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                app.input_mode = InputMode::Normal;
                                app.status_message = "Disconnect cancelled".to_string();
                            }
                            _ => {}
                        }
                    }
                    InputMode::Settings => {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('s') => {
                                app.input_mode = InputMode::Normal;
                                app.status_message.clear();
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                if app.settings_selection > 0 {
                                    app.settings_selection -= 1;
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                if app.settings_selection < 2 {
                                    app.settings_selection += 1;
                                }
                            }
                            KeyCode::Enter => {
                                match app.settings_selection {
                                    0 => {
                                        // Toggle ASCII icons
                                        app.use_ascii_icons = !app.use_ascii_icons;
                                        app.status_message = if app.use_ascii_icons {
                                            "Using ASCII icons".to_string()
                                        } else {
                                            "Using Nerd Font icons".to_string()
                                        };
                                    }
                                    1 => {
                                        // Increase update interval on Enter
                                        if app.update_interval < 60 {
                                            app.update_interval += 1;
                                        }
                                        app.status_message =
                                            format!("Update interval: {}s", app.update_interval);
                                    }
                                    2 => {
                                        // Toggle auto-connect
                                        app.auto_connect = !app.auto_connect;
                                        app.status_message = if app.auto_connect {
                                            "Auto-connect enabled".to_string()
                                        } else {
                                            "Auto-connect disabled".to_string()
                                        };
                                    }
                                    _ => {}
                                }
                            }
                            KeyCode::Right | KeyCode::Char('l') => {
                                if app.settings_selection == 1 {
                                    // Increase update interval
                                    if app.update_interval < 60 {
                                        app.update_interval += 1;
                                    }
                                    app.status_message =
                                        format!("Update interval: {}s", app.update_interval);
                                }
                            }
                            KeyCode::Left | KeyCode::Char('h') => {
                                if app.settings_selection == 1 {
                                    // Decrease update interval
                                    if app.update_interval > 1 {
                                        app.update_interval -= 1;
                                    }
                                    app.status_message =
                                        format!("Update interval: {}s", app.update_interval);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Auto-refresh connected servers
        if app.last_update.elapsed() >= Duration::from_secs(app.update_interval) {
            for server_id in app.connected_clients.keys().cloned().collect::<Vec<_>>() {
                let _ = fetch_server_data(app, server_id).await;
            }
            app.last_update = Instant::now();
        }

        if !app.running {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_app() -> DashboardApp {
        let servers = vec![
            ServerEndpoint {
                id: uuid::Uuid::new_v4(),
                name: "server-1".to_string(),
                address: "192.168.1.1".to_string(),
                port: 50051,
                description: None,
                access_token: None,
            },
            ServerEndpoint {
                id: uuid::Uuid::new_v4(),
                name: "server-2".to_string(),
                address: "192.168.1.2".to_string(),
                port: 50051,
                description: None,
                access_token: None,
            },
        ];
        DashboardApp::new(servers, NotificationConfig::default(), None)
    }

    #[test]
    fn test_dashboard_app_new() {
        let app = create_test_app();
        assert_eq!(app.servers.len(), 2);
        assert_eq!(app.current_tab, 0);
        assert!(app.running);
        assert_eq!(app.update_interval, 5);
        assert_eq!(app.connection_status.len(), 2);
    }

    #[test]
    fn test_get_selected_server() {
        let app = create_test_app();
        let selected = app.get_selected_server();
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().name, "server-1");
    }

    #[test]
    fn test_next_server() {
        let mut app = create_test_app();
        app.next_server();
        let selected = app.get_selected_server();
        assert_eq!(selected.unwrap().name, "server-2");
    }

    #[test]
    fn test_previous_server_wrap() {
        let mut app = create_test_app();
        app.previous_server();
        let selected = app.get_selected_server();
        assert_eq!(selected.unwrap().name, "server-2");
    }

    #[test]
    fn test_next_tab() {
        let mut app = create_test_app();
        assert_eq!(app.current_tab, 0);
        app.next_tab();
        assert_eq!(app.current_tab, 1);
        app.next_tab();
        assert_eq!(app.current_tab, 2);
    }

    #[test]
    fn test_next_tab_wrap() {
        let mut app = create_test_app();
        app.current_tab = 7;
        app.next_tab();
        assert_eq!(app.current_tab, 0);
    }

    #[test]
    fn test_previous_tab() {
        let mut app = create_test_app();
        app.current_tab = 1;
        app.previous_tab();
        assert_eq!(app.current_tab, 0);
    }

    #[test]
    fn test_previous_tab_wrap() {
        let mut app = create_test_app();
        app.current_tab = 0;
        app.previous_tab();
        assert_eq!(app.current_tab, 7);
    }

    #[test]
    fn test_update_history() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;

        app.update_history(server_id, 45.5, 60.0);
        assert_eq!(app.cpu_history.get(&server_id).unwrap().len(), 1);
        assert_eq!(app.mem_history.get(&server_id).unwrap().len(), 1);

        // Add 65 more entries to test limit of 60
        for i in 0..65 {
            app.update_history(server_id, i as f64, i as f64);
        }
        assert_eq!(app.cpu_history.get(&server_id).unwrap().len(), 60);
        assert_eq!(app.mem_history.get(&server_id).unwrap().len(), 60);
    }

    #[test]
    fn test_update_history_clamping() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;

        app.update_history(server_id, 150.0, -10.0);
        let cpu = app.cpu_history.get(&server_id).unwrap()[0];
        let mem = app.mem_history.get(&server_id).unwrap()[0];
        assert_eq!(cpu, 100);
        assert_eq!(mem, 0);
    }

    #[test]
    fn test_is_connected() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        assert!(!app.is_connected(&server_id));

        app.connection_status
            .insert(server_id, ConnectionStatus::Connected);
        assert!(app.is_connected(&server_id));

        app.connection_status
            .insert(server_id, ConnectionStatus::Disconnected);
        assert!(!app.is_connected(&server_id));
    }

    #[test]
    fn test_icon_selection() {
        let mut app = create_test_app();
        app.use_ascii_icons = true;
        assert_eq!(app.icon("", ">"), ">");

        app.use_ascii_icons = false;
        assert_eq!(app.icon("", ">"), "");
    }

    #[test]
    fn test_toggle_sort() {
        let mut app = create_test_app();
        app.toggle_sort(SortColumn::Cpu);
        assert_eq!(app.sort_column, SortColumn::Cpu);
        assert_eq!(app.sort_order, SortOrder::Ascending);

        app.toggle_sort(SortColumn::Cpu);
        assert_eq!(app.sort_order, SortOrder::Descending);

        app.toggle_sort(SortColumn::Memory);
        assert_eq!(app.sort_column, SortColumn::Memory);
        assert_eq!(app.sort_order, SortOrder::Ascending);
    }

    #[test]
    fn test_get_filtered_processes_empty() {
        let app = create_test_app();
        let processes = app.get_filtered_processes();
        assert!(processes.is_empty());
    }

    #[test]
    fn test_get_filtered_processes_with_filter() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.processes_cache.insert(
            server_id,
            vec![
                ProcessInfo {
                    pid: 1,
                    name: "nginx".to_string(),
                    user: "root".to_string(),
                    cpu_usage_percent: 5.0,
                    memory_usage_bytes: 1024,
                    command_line: "/usr/sbin/nginx".to_string(),
                    start_time: Utc::now(),
                    status: "Running".to_string(),
                },
                ProcessInfo {
                    pid: 2,
                    name: "postgres".to_string(),
                    user: "postgres".to_string(),
                    cpu_usage_percent: 10.0,
                    memory_usage_bytes: 2048,
                    command_line: "/usr/bin/postgres".to_string(),
                    start_time: Utc::now(),
                    status: "Running".to_string(),
                },
            ],
        );

        // No filter
        let processes = app.get_filtered_processes();
        assert_eq!(processes.len(), 2);

        // Filter by name
        app.process_filter = "nginx".to_string();
        let processes = app.get_filtered_processes();
        assert_eq!(processes.len(), 1);
        assert_eq!(processes[0].name, "nginx");

        // Filter by command line
        app.process_filter = "postgres".to_string();
        let processes = app.get_filtered_processes();
        assert_eq!(processes.len(), 1);
        assert_eq!(processes[0].name, "postgres");

        // No match
        app.process_filter = "nonexistent".to_string();
        let processes = app.get_filtered_processes();
        assert!(processes.is_empty());
    }

    #[test]
    fn test_connection_status_display() {
        let s = ConnectionStatus::Connected;
        assert_eq!(format!("{:?}", s), "Connected");
    }

    #[test]
    fn test_add_server_state_default() {
        let state = AddServerState::default();
        assert_eq!(state.step, AddServerStep::Name);
        assert!(state.name.is_empty());
    }

    #[test]
    fn test_sort_order_default() {
        assert_eq!(SortOrder::default(), SortOrder::Ascending);
    }

    #[test]
    fn test_sort_column_default() {
        assert_eq!(SortColumn::default(), SortColumn::Name);
    }

    #[test]
    fn test_update_history_performance() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;

        let start = std::time::Instant::now();
        for i in 0..1000 {
            app.update_history(server_id, i as f64 % 100.0, i as f64 % 100.0);
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_secs() < 5,
            "update_history 1000x took too long: {:?}",
            elapsed
        );
        assert_eq!(app.cpu_history.get(&server_id).unwrap().len(), 60);
        assert_eq!(app.mem_history.get(&server_id).unwrap().len(), 60);
    }

    #[test]
    fn test_get_filtered_processes_performance() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;

        // Create a large process list
        let processes: Vec<ProcessInfo> = (0..500)
            .map(|i| ProcessInfo {
                pid: i,
                name: format!("process-{}", i),
                user: "root".to_string(),
                cpu_usage_percent: i as f64,
                memory_usage_bytes: i as u64 * 1024,
                command_line: format!("/usr/bin/process-{}", i),
                start_time: Utc::now(),
                status: "Running".to_string(),
            })
            .collect();

        app.processes_cache.insert(server_id, processes);

        let start = std::time::Instant::now();
        for i in 0..100 {
            app.process_filter = format!("process-{}", i);
            let _ = app.get_filtered_processes();
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_secs() < 5,
            "get_filtered_processes 100x took too long: {:?}",
            elapsed
        );
    }

    #[test]
    fn test_with_config_path() {
        let app = create_test_app().with_config_path(std::path::PathBuf::from("custom-config.toml"));
        assert_eq!(app.config_path, std::path::PathBuf::from("custom-config.toml"));
    }

    #[test]
    fn test_next_item_empty() {
        let mut app = create_test_app();
        app.current_tab = 1;
        app.next_item();
        assert_eq!(app.selected_item_idx, 0);
    }

    #[test]
    fn test_next_item() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.current_tab = 1;
        app.services_cache.insert(
            server_id,
            vec![
                ServiceInfo {
                    name: "svc1".to_string(),
                    status: ServiceStatus::Running,
                    pid: Some(1),
                    cpu_usage_percent: 0.0,
                    memory_usage_bytes: 0,
                    user: "root".to_string(),
                    uptime_seconds: None,
                },
                ServiceInfo {
                    name: "svc2".to_string(),
                    status: ServiceStatus::Running,
                    pid: Some(2),
                    cpu_usage_percent: 0.0,
                    memory_usage_bytes: 0,
                    user: "root".to_string(),
                    uptime_seconds: None,
                },
            ],
        );
        app.next_item();
        assert_eq!(app.selected_item_idx, 1);
        app.next_item();
        assert_eq!(app.selected_item_idx, 0);
    }

    #[test]
    fn test_previous_item() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.current_tab = 1;
        app.services_cache.insert(
            server_id,
            vec![
                ServiceInfo {
                    name: "svc1".to_string(),
                    status: ServiceStatus::Running,
                    pid: Some(1),
                    cpu_usage_percent: 0.0,
                    memory_usage_bytes: 0,
                    user: "root".to_string(),
                    uptime_seconds: None,
                },
                ServiceInfo {
                    name: "svc2".to_string(),
                    status: ServiceStatus::Running,
                    pid: Some(2),
                    cpu_usage_percent: 0.0,
                    memory_usage_bytes: 0,
                    user: "root".to_string(),
                    uptime_seconds: None,
                },
            ],
        );
        app.selected_item_idx = 1;
        app.previous_item();
        assert_eq!(app.selected_item_idx, 0);
        app.previous_item();
        assert_eq!(app.selected_item_idx, 1);
    }

    #[test]
    fn test_previous_item_empty() {
        let mut app = create_test_app();
        app.current_tab = 1;
        app.previous_item();
        assert_eq!(app.selected_item_idx, 0);
    }

    #[test]
    fn test_get_current_list_len_overview() {
        let app = create_test_app();
        assert_eq!(app.get_current_list_len(), 0);
    }

    #[test]
    fn test_get_current_list_len_services() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.current_tab = 1;
        app.services_cache.insert(
            server_id,
            vec![
                ServiceInfo {
                    name: "svc1".to_string(),
                    status: ServiceStatus::Running,
                    pid: Some(1),
                    cpu_usage_percent: 0.0,
                    memory_usage_bytes: 0,
                    user: "root".to_string(),
                    uptime_seconds: None,
                },
                ServiceInfo {
                    name: "svc2".to_string(),
                    status: ServiceStatus::Running,
                    pid: Some(2),
                    cpu_usage_percent: 0.0,
                    memory_usage_bytes: 0,
                    user: "root".to_string(),
                    uptime_seconds: None,
                },
                ServiceInfo {
                    name: "svc3".to_string(),
                    status: ServiceStatus::Running,
                    pid: Some(3),
                    cpu_usage_percent: 0.0,
                    memory_usage_bytes: 0,
                    user: "root".to_string(),
                    uptime_seconds: None,
                },
            ],
        );
        assert_eq!(app.get_current_list_len(), 3);
    }

    #[test]
    fn test_get_current_list_len_processes() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.current_tab = 2;
        app.processes_cache.insert(
            server_id,
            vec![
                ProcessInfo {
                    pid: 1,
                    name: "p1".to_string(),
                    user: "root".to_string(),
                    cpu_usage_percent: 0.0,
                    memory_usage_bytes: 0,
                    command_line: "".to_string(),
                    start_time: Utc::now(),
                    status: "".to_string(),
                },
                ProcessInfo {
                    pid: 2,
                    name: "p2".to_string(),
                    user: "root".to_string(),
                    cpu_usage_percent: 0.0,
                    memory_usage_bytes: 0,
                    command_line: "".to_string(),
                    start_time: Utc::now(),
                    status: "".to_string(),
                },
            ],
        );
        assert_eq!(app.get_current_list_len(), 2);
    }

    #[test]
    fn test_get_current_list_len_network() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.current_tab = 3;
        app.network_cache.insert(
            server_id,
            vec![
                NetworkInfo {
                    interface: "eth0".to_string(),
                    ip_address: "127.0.0.1".to_string(),
                    mac_address: "00:00:00:00:00:00".to_string(),
                    is_up: true,
                    bytes_sent: 0,
                    bytes_received: 0,
                    packets_sent: 0,
                    packets_received: 0,
                },
                NetworkInfo {
                    interface: "eth1".to_string(),
                    ip_address: "192.168.1.1".to_string(),
                    mac_address: "00:00:00:00:00:01".to_string(),
                    is_up: true,
                    bytes_sent: 0,
                    bytes_received: 0,
                    packets_sent: 0,
                    packets_received: 0,
                },
                NetworkInfo {
                    interface: "lo".to_string(),
                    ip_address: "::1".to_string(),
                    mac_address: "00:00:00:00:00:00".to_string(),
                    is_up: true,
                    bytes_sent: 0,
                    bytes_received: 0,
                    packets_sent: 0,
                    packets_received: 0,
                },
            ],
        );
        assert_eq!(app.get_current_list_len(), 3);
    }

    #[test]
    fn test_get_current_list_len_containers() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.current_tab = 4;
        app.containers_cache.insert(
            server_id,
            vec![
                ContainerInfo {
                    id: "c1".to_string(),
                    name: "container1".to_string(),
                    image: "img1".to_string(),
                    status: "running".to_string(),
                    state: "running".to_string(),
                    health: "healthy".to_string(),
                    cpu_percent: 0.0,
                    memory_usage_bytes: 0,
                    memory_limit_bytes: 0,
                    memory_percent: 0.0,
                    restart_count: 0,
                    network_rx_bytes: 0,
                    network_tx_bytes: 0,
                    networks: vec![],
                },
            ],
        );
        assert_eq!(app.get_current_list_len(), 1);
    }

    #[test]
    fn test_get_current_list_len_postgres() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.current_tab = 5;
        app.postgres_cache.insert(
            server_id,
            vec![
                PostgresClusterInfo {
                    name: "pg".to_string(),
                    host: "localhost".to_string(),
                    port: 5432,
                    databases: vec![],
                    connections_total: 0,
                    connections_by_state: vec![],
                    cache_hit_ratio: 0.0,
                    top_queries: vec![],
                    timestamp: Utc::now(),
                },
            ],
        );
        assert_eq!(app.get_current_list_len(), 1);
    }

    #[test]
    fn test_get_current_list_len_mariadb() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.current_tab = 6;
        app.mariadb_cache.insert(
            server_id,
            vec![
                MariaDBClusterInfo {
                    name: "mdb".to_string(),
                    host: "localhost".to_string(),
                    port: 3306,
                    schemas: vec![],
                    connections_active: 0,
                    connections_total: 0,
                    innodb_status: None,
                    processes: vec![],
                    timestamp: Utc::now(),
                },
            ],
        );
        assert_eq!(app.get_current_list_len(), 1);
    }

    #[test]
    fn test_get_current_list_len_systemd() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.current_tab = 7;
        app.systemd_cache.insert(
            server_id,
            vec![
                SystemdUnitInfo {
                    name: "unit1".to_string(),
                    status: "active".to_string(),
                    is_active: true,
                    pid: Some(1),
                    memory_current_bytes: 0,
                    started_at: None,
                },
                SystemdUnitInfo {
                    name: "unit2".to_string(),
                    status: "inactive".to_string(),
                    is_active: false,
                    pid: None,
                    memory_current_bytes: 0,
                    started_at: None,
                },
            ],
        );
        assert_eq!(app.get_current_list_len(), 2);
    }

    #[test]
    fn test_get_current_list_len_no_server() {
        let mut app = create_test_app();
        app.servers.clear();
        app.current_tab = 1;
        assert_eq!(app.get_current_list_len(), 0);
    }

    #[test]
    fn test_get_selected_process_none() {
        let app = create_test_app();
        assert!(app.get_selected_process().is_none());
    }

    #[test]
    fn test_get_selected_process_some() {
        let mut app = create_test_app();
        let server_id = app.servers[0].id;
        app.processes_cache.insert(
            server_id,
            vec![ProcessInfo {
                pid: 1,
                name: "test".to_string(),
                user: "root".to_string(),
                cpu_usage_percent: 0.0,
                memory_usage_bytes: 0,
                command_line: "".to_string(),
                start_time: Utc::now(),
                status: "".to_string(),
            }],
        );
        app.selected_item_idx = 0;
        assert!(app.get_selected_process().is_some());
        assert_eq!(app.get_selected_process().unwrap().name, "test");
    }

    #[test]
    fn test_toggle_sort_descending_to_ascending() {
        let mut app = create_test_app();
        app.sort_column = SortColumn::Cpu;
        app.sort_order = SortOrder::Descending;
        app.toggle_sort(SortColumn::Cpu);
        assert_eq!(app.sort_order, SortOrder::Ascending);
    }
}
