//! System Monitoring Client
//!
//! This is the client component that connects to monitoring servers and displays
//! system information with a rich CLI interface including progress bars.

use anyhow::Result;
use clap::Parser;
use shared::types::*;
use std::collections::HashMap;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

mod cli;
mod client;
mod config;
mod dashboard;
mod storage;
mod tls;
mod ui;

use crate::cli::CliArgs;
use crate::client::MonitorClient;
use crate::config::ClientConfigManager;

#[tokio::main]
async fn main() -> Result<()> {
    let args = CliArgs::parse();

    // Setup logging
    let log_level = match args.log_level.as_str() {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    };

    tracing_subscriber::fmt().with_max_level(log_level).init();

    info!("Starting system monitoring client...");

    // Load configuration
    let mut config_manager = ClientConfigManager::new(&args.config)?;

    // If no command provided, open the interactive dashboard
    let command = args.command.unwrap_or(cli::Commands::Dashboard);

    match command {
        cli::Commands::Add {
            name,
            address,
            port,
            token,
            description,
        } => {
            add_server(&mut config_manager, name, address, port, token, description).await?;
        }
        cli::Commands::Remove { id } => {
            remove_server(&mut config_manager, id).await?;
        }
        cli::Commands::List => {
            list_servers(&config_manager).await?;
        }
        cli::Commands::Monitor { server, interval } => {
            monitor_server(&config_manager, server, interval).await?;
        }
        cli::Commands::Serve { address, port } => {
            start_server_mode(&config_manager, address, port).await?;
        }
        cli::Commands::Dashboard => {
            run_dashboard(&config_manager).await?;
        }
        cli::Commands::Services { server, limit } => {
            list_services(&config_manager, server, limit).await?;
        }
        cli::Commands::Processes {
            server,
            limit,
            filter,
        } => {
            list_processes(&config_manager, server, limit, filter).await?;
        }
        cli::Commands::SetToken { id, token } => {
            set_server_token(&mut config_manager, id, token).await?;
        }
        cli::Commands::Connect { address, port } => {
            quick_connect(&mut config_manager, address, port).await?;
        }
        cli::Commands::Export {
            server,
            format,
            output,
            hours,
        } => {
            export_metrics(&config_manager, server, format, output, hours).await?;
        }
        cli::Commands::StorageStats => {
            show_storage_stats().await?;
        }
        cli::Commands::Purge { days } => {
            purge_storage(days).await?;
        }
    }

    Ok(())
}

async fn add_server(
    config_manager: &mut ClientConfigManager,
    name: String,
    address: String,
    port: u16,
    token: Option<String>,
    description: Option<String>,
) -> Result<()> {
    let endpoint = ServerEndpoint {
        id: uuid::Uuid::new_v4(),
        name: name.clone(),
        address: address.clone(),
        port,
        description,
        access_token: token.clone(),
    };

    config_manager.add_server(endpoint)?;

    println!();
    println!("✅ Server added successfully!");
    println!();
    println!("   Name: {}", name);
    println!("   Address: {}:{}", address, port);
    if token.is_some() {
        println!("   Token: ******* (configured)");
    } else {
        println!("   Token: Not set (use 'set-token' command if needed)");
    }
    println!();

    Ok(())
}

async fn set_server_token(
    config_manager: &mut ClientConfigManager,
    id: uuid::Uuid,
    token: String,
) -> Result<()> {
    let mut config = config_manager.load_config()?;

    let server_name = if let Some(server) = config.servers.iter_mut().find(|s| s.id == id) {
        server.access_token = Some(token);
        Some(server.name.clone())
    } else {
        None
    };

    if let Some(name) = server_name {
        config_manager.save_config(&config)?;
        println!("✅ Token updated for server '{}'", name);
    } else {
        println!("❌ Server with ID {} not found", id);
    }

    Ok(())
}

async fn quick_connect(
    config_manager: &mut ClientConfigManager,
    address: String,
    port: u16,
) -> Result<()> {
    use std::io::{self, Write};

    println!();
    println!("🔌 Quick Connect to {}:{}", address, port);
    println!();

    // Ask for server name
    print!("   Server name: ");
    io::stdout().flush()?;
    let mut name = String::new();
    io::stdin().read_line(&mut name)?;
    let name = name.trim().to_string();
    let name = if name.is_empty() {
        format!("Server-{}", &address)
    } else {
        name
    };

    // Ask for access token
    print!("   Access token (from server, or leave empty): ");
    io::stdout().flush()?;
    let mut token = String::new();
    io::stdin().read_line(&mut token)?;
    let token = token.trim().to_string();
    let token = if token.is_empty() { None } else { Some(token) };

    let endpoint = ServerEndpoint {
        id: uuid::Uuid::new_v4(),
        name: name.clone(),
        address: address.clone(),
        port,
        description: None,
        access_token: token.clone(),
    };

    // Try to connect first
    println!();
    println!("   Testing connection...");

    match MonitorClient::connect_with_token(
        &format!("http://{}:{}", address, port),
        5,
        false,
        token.as_deref(),
        None,
    )
    .await
    {
        Ok(mut client) => match client.get_system_info().await {
            Ok(info) => {
                println!("   ✅ Connected successfully!");
                println!();
                println!("   Server info:");
                println!("     Hostname: {}", info.hostname);
                println!("     OS: {}", info.os);
                println!();

                config_manager.add_server(endpoint)?;
                println!("   Server saved to configuration.");
            }
            Err(e) => {
                println!("   ⚠️  Connected but failed to get info: {}", e);
                println!("   This might be an authentication issue.");
                print!("   Save anyway? (y/n): ");
                io::stdout().flush()?;
                let mut answer = String::new();
                io::stdin().read_line(&mut answer)?;
                if answer.trim().to_lowercase() == "y" {
                    config_manager.add_server(endpoint)?;
                    println!("   Server saved.");
                }
            }
        },
        Err(e) => {
            println!("   ❌ Connection failed: {}", e);
            print!("   Save anyway? (y/n): ");
            io::stdout().flush()?;
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            if answer.trim().to_lowercase() == "y" {
                config_manager.add_server(endpoint)?;
                println!("   Server saved.");
            }
        }
    }

    println!();
    Ok(())
}

async fn remove_server(config_manager: &mut ClientConfigManager, id: uuid::Uuid) -> Result<()> {
    config_manager.remove_server(id)?;
    println!("✅ Server removed successfully");
    Ok(())
}

async fn list_servers(config_manager: &ClientConfigManager) -> Result<()> {
    let config = config_manager.load_config()?;

    if config.servers.is_empty() {
        println!();
        println!("📋 No servers configured.");
        println!();
        println!("Add a server with:");
        println!(
            "  monitor-client add --name \"My Server\" --address 192.168.1.100 --token <token>"
        );
        println!();
        println!("Or use quick connect:");
        println!("  monitor-client connect 192.168.1.100");
        println!();
        return Ok(());
    }

    println!();
    println!("📋 Configured Servers:");
    println!();
    for server in &config.servers {
        let token_status = if server.access_token.is_some() {
            "✓"
        } else {
            "✗"
        };
        println!("  ┌─────────────────────────────────────────────────────");
        println!("  │ {} ({})", server.name, server.id);
        println!("  │ Address: {}:{}", server.address, server.port);
        println!("  │ Token: {}", token_status);
        if let Some(ref desc) = server.description {
            println!("  │ Description: {}", desc);
        }
        println!("  └─────────────────────────────────────────────────────");
        println!();
    }

    Ok(())
}

async fn monitor_server(
    config_manager: &ClientConfigManager,
    server_id: Option<uuid::Uuid>,
    interval: u64,
) -> Result<()> {
    let config = config_manager.load_config()?;

    let servers_to_monitor = if let Some(id) = server_id {
        config
            .servers
            .iter()
            .filter(|s| s.id == id)
            .cloned()
            .collect()
    } else {
        config.servers.clone()
    };

    if servers_to_monitor.is_empty() {
        println!("No servers to monitor. Add servers first or specify a valid server ID.");
        return Ok(());
    }

    info!(
        "Starting monitoring for {} server(s)",
        servers_to_monitor.len()
    );

    // Create clients for each server
    let mut clients = HashMap::new();
    for server in &servers_to_monitor {
        let client = MonitorClient::connect_with_token(
            &format!("http://{}:{}", server.address, server.port),
            interval,
            config.auto_reconnect,
            server.access_token.as_deref(),
            config.tls.as_ref(),
        )
        .await?;
        clients.insert(server.id, (server.clone(), client));
    }

    // Start monitoring loop
    start_monitoring_loop(clients, interval).await?;

    Ok(())
}

async fn start_monitoring_loop(
    mut clients: HashMap<uuid::Uuid, (ServerEndpoint, MonitorClient)>,
    interval: u64,
) -> Result<()> {
    loop {
        let mut system_summaries = Vec::new();

        // Collect data from all servers
        for (endpoint, client) in clients.values_mut() {
            match client.get_system_info().await {
                Ok(system_info) => {
                    system_summaries.push((endpoint.clone(), system_info));
                }
                Err(e) => {
                    warn!("Failed to get system info from {}: {}", endpoint.name, e);
                    if client.is_auto_reconnect() {
                        // Try to reconnect
                        match MonitorClient::connect_with_token(
                            &format!("http://{}:{}", endpoint.address, endpoint.port),
                            interval,
                            true,
                            endpoint.access_token.as_deref(),
                            None,
                        )
                        .await
                        {
                            Ok(new_client) => {
                                *client = new_client;
                                info!("Reconnected to {}", endpoint.name);
                            }
                            Err(reconnect_err) => {
                                error!(
                                    "Failed to reconnect to {}: {}",
                                    endpoint.name, reconnect_err
                                );
                            }
                        }
                    }
                }
            }
        }

        // Display the collected data
        display_system_summary(&system_summaries);

        // Wait for the next update
        sleep(Duration::from_secs(interval)).await;
    }
}

fn display_system_summary(summaries: &Vec<(ServerEndpoint, SystemInfo)>) {
    // Clear screen and show header
    print!("\x1B[2J\x1B[H");
    println!("=== System Monitoring Dashboard ===");
    println!(
        "Last updated: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!();

    for (server, system_info) in summaries {
        println!("📊 Server: {} ({})", server.name, server.address);
        println!("   Hostname: {}", system_info.hostname);
        println!("   OS: {}", system_info.os);
        println!("   Uptime: {} seconds", system_info.uptime_seconds);
        println!();

        // CPU usage with progress bar
        let cpu_percent = system_info.cpu_usage_percent;
        println!("   🖥️  CPU Usage: {:.1}%", cpu_percent);
        draw_progress_bar(cpu_percent, 50);
        println!();

        // Memory usage with progress bar
        let memory_percent =
            (system_info.memory_used_bytes as f64 / system_info.memory_total_bytes as f64) * 100.0;
        println!("   💾 Memory Usage: {:.1}%", memory_percent);
        draw_progress_bar(memory_percent, 50);
        println!(
            "   Total: {} MB, Used: {} MB, Available: {} MB",
            system_info.memory_total_bytes / 1024 / 1024,
            system_info.memory_used_bytes / 1024 / 1024,
            system_info.memory_available_bytes / 1024 / 1024
        );
        println!();

        // Disk usage
        println!("   💿 Disk Usage:");
        for disk in &system_info.disk_info {
            println!(
                "     {}: {:.1}% ({}/{})",
                disk.mount_point,
                disk.usage_percent,
                format_bytes(disk.used_bytes),
                format_bytes(disk.total_bytes)
            );
        }
        println!();

        println!("{}", "-".repeat(80));
        println!();
    }
}

fn draw_progress_bar(percentage: f64, width: usize) {
    let filled = ((percentage / 100.0) * width as f64).min(width as f64) as usize;
    let empty = width.saturating_sub(filled);

    let bar = "█".repeat(filled) + &"░".repeat(empty);
    let percentage_str = format!("{:5.1}%", percentage);

    println!("   [{}] {}", bar, percentage_str);
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

async fn start_server_mode(
    _config_manager: &ClientConfigManager,
    _address: String,
    _port: u16,
) -> Result<()> {
    println!("Client server mode is not yet implemented");
    println!("This would run a local server for the client to connect to remote systems");
    Ok(())
}

async fn run_dashboard(config_manager: &ClientConfigManager) -> Result<()> {
    let config = config_manager.load_config()?;

    info!(
        "Starting interactive dashboard with {} server(s)",
        config.servers.len()
    );

    // Dashboard always opens - users can add servers from within the dashboard using 'a' key
    dashboard::run_dashboard(
        config.servers,
        config.update_interval_seconds,
        config.notifications,
        config.tls,
    )
    .await?;

    Ok(())
}

async fn list_services(
    config_manager: &ClientConfigManager,
    server_id: Option<uuid::Uuid>,
    limit: u32,
) -> Result<()> {
    let config = config_manager.load_config()?;

    let server = if let Some(id) = server_id {
        config.servers.iter().find(|s| s.id == id).cloned()
    } else {
        config.servers.first().cloned()
    };

    let server = match server {
        Some(s) => s,
        None => {
            println!("No servers configured. Use 'add' command to add a server first.");
            return Ok(());
        }
    };

    println!("Connecting to {}...", server.name);

    let mut client = MonitorClient::connect(
        &format!("http://{}:{}", server.address, server.port),
        5,
        false,
    )
    .await?;

    println!("Fetching services from {}...\n", server.name);

    let services = client.get_services().await?;

    println!("╔════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                       Services / Long-running Processes                        ║");
    println!("╠════════════════════════════════════════════════════════════════════════════════╣");
    println!(
        "║ {:<25} │ {:<10} │ {:>7} │ {:>7} │ {:>12} ║",
        "Name", "Status", "PID", "CPU%", "Memory"
    );
    println!("╟──────────────────────────┼────────────┼─────────┼─────────┼──────────────╢");

    for (i, service) in services.iter().enumerate() {
        if i >= limit as usize {
            break;
        }

        let status_str = match service.status {
            ServiceStatus::Running => "🟢 Running",
            ServiceStatus::Stopped => "🔴 Stopped",
            ServiceStatus::Failed => "❌ Failed",
            ServiceStatus::Unknown => "⚪ Unknown",
        };

        let pid_str = service
            .pid
            .map(|p| p.to_string())
            .unwrap_or("-".to_string());
        let memory_str = format_bytes(service.memory_usage_bytes);

        println!(
            "║ {:<25} │ {:<10} │ {:>7} │ {:>6.1}% │ {:>12} ║",
            truncate_str(&service.name, 25),
            status_str,
            pid_str,
            service.cpu_usage_percent,
            memory_str
        );
    }

    println!("╚════════════════════════════════════════════════════════════════════════════════╝");
    println!(
        "\nTotal: {} service(s) / process(es)",
        services.len().min(limit as usize)
    );

    Ok(())
}

async fn list_processes(
    config_manager: &ClientConfigManager,
    server_id: Option<uuid::Uuid>,
    limit: u32,
    filter: Option<String>,
) -> Result<()> {
    let config = config_manager.load_config()?;

    let server = if let Some(id) = server_id {
        config.servers.iter().find(|s| s.id == id).cloned()
    } else {
        config.servers.first().cloned()
    };

    let server = match server {
        Some(s) => s,
        None => {
            println!("No servers configured. Use 'add' command to add a server first.");
            return Ok(());
        }
    };

    println!("Connecting to {}...", server.name);

    let mut client = MonitorClient::connect(
        &format!("http://{}:{}", server.address, server.port),
        5,
        false,
    )
    .await?;

    let filter_msg = filter
        .as_ref()
        .map(|f| format!(" (filter: {})", f))
        .unwrap_or_default();
    println!("Fetching processes from {}{}...\n", server.name, filter_msg);

    let processes = client.get_processes(limit, filter).await?;

    println!(
        "╔══════════════════════════════════════════════════════════════════════════════════════╗"
    );
    println!(
        "║                                  Running Processes                                   ║"
    );
    println!(
        "╠══════════════════════════════════════════════════════════════════════════════════════╣"
    );
    println!(
        "║ {:>7} │ {:<30} │ {:>7} │ {:>12} │ {:<12} ║",
        "PID", "Name", "CPU%", "Memory", "Status"
    );
    println!(
        "╟─────────┼────────────────────────────────┼─────────┼──────────────┼──────────────╢"
    );

    for process in &processes {
        let memory_str = format_bytes(process.memory_usage_bytes);

        println!(
            "║ {:>7} │ {:<30} │ {:>6.1}% │ {:>12} │ {:<12} ║",
            process.pid,
            truncate_str(&process.name, 30),
            process.cpu_usage_percent,
            memory_str,
            truncate_str(&process.status, 12)
        );
    }

    println!(
        "╚══════════════════════════════════════════════════════════════════════════════════════╝"
    );
    println!("\nTotal: {} process(es)", processes.len());

    Ok(())
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    } else {
        s.to_string()
    }
}

async fn export_metrics(
    config_manager: &ClientConfigManager,
    server_id: Option<uuid::Uuid>,
    format: String,
    output: std::path::PathBuf,
    hours: i64,
) -> Result<()> {
    use crate::storage::MetricsStorage;

    let storage = MetricsStorage::new("code-monitor.db")?;

    let config = config_manager.load_config()?;

    // Determine which server(s) to export
    let server_ids: Vec<String> = if let Some(id) = server_id {
        vec![id.to_string()]
    } else {
        // Export all servers
        config.servers.iter().map(|s| s.id.to_string()).collect()
    };

    if server_ids.is_empty() {
        println!("No servers configured. Nothing to export.");
        return Ok(());
    }

    println!("Exporting metrics for {} server(s)...", server_ids.len());
    println!("Format: {}", format);
    println!("Hours: {}", hours);
    println!("Output: {}", output.display());
    println!();

    let mut total_records = 0;

    if format.to_lowercase() == "csv" {
        let mut wtr = csv::Writer::from_path(&output)?;
        wtr.write_record([
            "server_id",
            "timestamp",
            "cpu_usage",
            "memory_used",
            "memory_total",
        ])?;

        for server_id in &server_ids {
            let points = storage.get_history(server_id, hours)?;
            for point in &points {
                wtr.write_record([
                    &point.server_id,
                    &point.timestamp.to_rfc3339(),
                    &point.cpu_usage.to_string(),
                    &point.memory_used.to_string(),
                    &point.memory_total.to_string(),
                ])?;
            }
            total_records += points.len();
        }

        wtr.flush()?;
    } else if format.to_lowercase() == "json" {
        use crate::storage::MetricsPoint;

        let mut all_points: Vec<MetricsPoint> = Vec::new();

        for server_id in &server_ids {
            let points = storage.get_history(server_id, hours)?;
            all_points.extend(points);
        }

        let file = std::fs::File::create(&output)?;
        serde_json::to_writer_pretty(file, &all_points)?;
        total_records = all_points.len();
    } else {
        println!("❌ Unknown format: {}. Use 'csv' or 'json'.", format);
        return Ok(());
    }

    println!(
        "✅ Exported {} records to {}",
        total_records,
        output.display()
    );

    Ok(())
}

async fn show_storage_stats() -> Result<()> {
    use crate::storage::MetricsStorage;

    let storage = MetricsStorage::new("code-monitor.db")?;
    let stats = storage.get_stats()?;

    println!();
    println!("📊 Storage Statistics");
    println!();
    println!("  Total records: {}", stats.total_records);
    println!("  Database size: {} bytes", stats.size_bytes);

    if let Some(oldest) = stats.oldest_timestamp {
        println!(
            "  Oldest record: {}",
            oldest.format("%Y-%m-%d %H:%M:%S UTC")
        );
    }

    if let Some(newest) = stats.newest_timestamp {
        println!(
            "  Newest record: {}",
            newest.format("%Y-%m-%d %H:%M:%S UTC")
        );
    }

    println!();

    Ok(())
}

async fn purge_storage(days: i64) -> Result<()> {
    use crate::storage::MetricsStorage;

    println!("Purging metrics older than {} days...", days);

    let storage = MetricsStorage::new("code-monitor.db")?;
    let deleted = storage.purge_old(days)?;

    println!("✅ Deleted {} old records", deleted);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::TempDir;

    fn create_test_endpoint(name: &str, address: &str, port: u16) -> ServerEndpoint {
        ServerEndpoint {
            id: uuid::Uuid::new_v4(),
            name: name.to_string(),
            address: address.to_string(),
            port,
            description: None,
            access_token: None,
        }
    }

    fn create_test_system_info() -> SystemInfo {
        SystemInfo {
            hostname: "test-host".to_string(),
            os: "Linux".to_string(),
            kernel_version: "5.15.0".to_string(),
            uptime_seconds: 3600,
            cpu_count: 4,
            cpu_usage_percent: 42.5,
            memory_total_bytes: 16_000_000_000,
            memory_used_bytes: 8_000_000_000,
            memory_available_bytes: 8_000_000_000,
            disk_info: vec![DiskInfo {
                device: "/dev/sda1".to_string(),
                mount_point: "/".to_string(),
                filesystem_type: "ext4".to_string(),
                total_bytes: 500_000_000_000,
                used_bytes: 250_000_000_000,
                available_bytes: 250_000_000_000,
                usage_percent: 50.0,
            }],
            timestamp: Utc::now(),
        }
    }

    // ------------------------------------------------------------------
    // format_bytes
    // ------------------------------------------------------------------
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
    }

    #[test]
    fn test_format_bytes_mb() {
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn test_format_bytes_gb() {
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn test_format_bytes_tb() {
        assert_eq!(format_bytes(1024_u64 * 1024 * 1024 * 1024), "1.0 TB");
    }

    #[test]
    fn test_format_bytes_large() {
        // Values beyond TB should stay in TB
        let result = format_bytes(1024_u64 * 1024 * 1024 * 1024 * 5);
        assert!(result.contains("TB"));
    }

    // ------------------------------------------------------------------
    // truncate_str
    // ------------------------------------------------------------------
    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        assert_eq!(truncate_str("hello world", 8), "hello...");
    }

    #[test]
    fn test_truncate_str_empty() {
        assert_eq!(truncate_str("", 5), "");
    }

    // ------------------------------------------------------------------
    // draw_progress_bar (smoke: must not panic)
    // ------------------------------------------------------------------
    #[test]
    fn test_draw_progress_bar_various_values() {
        // These simply must not panic; output goes to test stdout.
        draw_progress_bar(0.0, 50);
        draw_progress_bar(50.0, 50);
        draw_progress_bar(100.0, 50);
        draw_progress_bar(150.0, 50); // over 100%
        draw_progress_bar(33.333, 20);
    }

    // ------------------------------------------------------------------
    // display_system_summary (smoke: must not panic)
    // ------------------------------------------------------------------
    #[test]
    fn test_display_system_summary_empty() {
        display_system_summary(&Vec::new());
    }

    #[test]
    fn test_display_system_summary_with_data() {
        let server = create_test_endpoint("srv", "192.168.1.1", 50051);
        let info = create_test_system_info();
        display_system_summary(&vec![(server, info)]);
    }

    // ------------------------------------------------------------------
    // add_server
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn test_add_server_success() {
        let temp = TempDir::new().unwrap();
        let mut manager = ClientConfigManager::new(&temp.path().join("cfg.toml")).unwrap();

        let result = add_server(
            &mut manager,
            "test-srv".to_string(),
            "192.168.1.10".to_string(),
            50051,
            Some("tok".to_string()),
            Some("desc".to_string()),
        )
        .await;

        assert!(result.is_ok());

        let cfg = manager.load_config().unwrap();
        assert_eq!(cfg.servers.len(), 1);
        assert_eq!(cfg.servers[0].name, "test-srv");
        assert_eq!(cfg.servers[0].access_token, Some("tok".to_string()));
    }

    // ------------------------------------------------------------------
    // remove_server
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn test_remove_server_success() {
        let temp = TempDir::new().unwrap();
        let mut manager = ClientConfigManager::new(&temp.path().join("cfg.toml")).unwrap();

        let ep = create_test_endpoint("srv", "192.168.1.10", 50051);
        manager.add_server(ep.clone()).unwrap();

        let result = remove_server(&mut manager, ep.id).await;
        assert!(result.is_ok());
        assert!(manager.load_config().unwrap().servers.is_empty());
    }

    #[tokio::test]
    async fn test_remove_server_not_found() {
        let temp = TempDir::new().unwrap();
        let mut manager = ClientConfigManager::new(&temp.path().join("cfg.toml")).unwrap();

        let result = remove_server(&mut manager, uuid::Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // set_server_token
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn test_set_server_token_success() {
        let temp = TempDir::new().unwrap();
        let mut manager = ClientConfigManager::new(&temp.path().join("cfg.toml")).unwrap();

        let ep = create_test_endpoint("srv", "192.168.1.10", 50051);
        manager.add_server(ep.clone()).unwrap();

        let result = set_server_token(&mut manager, ep.id, "new-token".to_string()).await;
        assert!(result.is_ok());

        let cfg = manager.load_config().unwrap();
        assert_eq!(cfg.servers[0].access_token, Some("new-token".to_string()));
    }

    #[tokio::test]
    async fn test_set_server_token_not_found() {
        let temp = TempDir::new().unwrap();
        let mut manager = ClientConfigManager::new(&temp.path().join("cfg.toml")).unwrap();

        // Should not error, just print a message
        let result = set_server_token(&mut manager, uuid::Uuid::new_v4(), "tok".to_string()).await;
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // list_servers
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn test_list_servers_empty() {
        let temp = TempDir::new().unwrap();
        let manager = ClientConfigManager::new(&temp.path().join("cfg.toml")).unwrap();

        let result = list_servers(&manager).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_servers_with_entries() {
        let temp = TempDir::new().unwrap();
        let mut manager = ClientConfigManager::new(&temp.path().join("cfg.toml")).unwrap();

        manager.add_server(create_test_endpoint("a", "10.0.0.1", 50051)).unwrap();
        manager.add_server(create_test_endpoint("b", "10.0.0.2", 50052)).unwrap();

        let result = list_servers(&manager).await;
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // start_server_mode
    // ------------------------------------------------------------------
    #[tokio::test]
    async fn test_start_server_mode_returns_ok() {
        let temp = TempDir::new().unwrap();
        let manager = ClientConfigManager::new(&temp.path().join("cfg.toml")).unwrap();

        let result = start_server_mode(&manager, "127.0.0.1".to_string(), 50052).await;
        assert!(result.is_ok());
    }
}
