//! System Monitoring Server
//!
//! This is the server component that runs on the target system and collects
//! system information to be queried by the client.

use anyhow::Result;
use clap::{Parser, Subcommand};
use shared::proto::monitoring::monitor_service_server::MonitorServiceServer;
use shared::types::ServerConfig;
use std::net::SocketAddr;
use tonic::transport::Server;
use tracing::{info, warn};

use crate::config::Config;
use crate::monitor::SystemMonitor;
use crate::service::MonitorServiceImpl;

mod collectors;
mod config;
mod health;
mod monitor;
mod service;
mod tls;

#[derive(Parser, Debug)]
#[command(name = "monitor-server")]
#[command(about = "System monitoring server")]
struct Args {
    /// Server address to bind to
    #[arg(short, long, default_value = "0.0.0.0")]
    address: String,

    /// Server port to listen on (gRPC)
    #[arg(short, long, default_value = "50051")]
    port: u16,

    /// Health check HTTP port
    #[arg(long, default_value = "8080")]
    health_port: u16,

    /// Disable health check HTTP server
    #[arg(long)]
    no_health: bool,

    /// Configuration file path
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Create or update the server configuration interactively
    Init {
        /// Overwrite an existing config without asking
        #[arg(short, long)]
        force: bool,
    },

    /// Show the access token for clients to connect
    ShowToken,

    /// Generate a new access token
    NewToken,

    /// List authorized clients
    ListClients,

    /// Remove an authorized client
    RemoveClient {
        /// Client name to remove
        #[arg(short, long)]
        name: String,
    },

    /// Disable authentication (allow any client)
    DisableAuth,

    /// Enable authentication
    EnableAuth,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Handle subcommands first (before setting up logging for cleaner output)
    if let Some(cmd) = &args.command {
        return handle_command(cmd, &args.config);
    }

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

    info!("Starting system monitoring server...");

    // Load configuration
    let config = Config::load(&args.config).unwrap_or_else(|e| {
        warn!("Failed to load config from {}: {}", args.config, e);
        let mut cfg = Config::default();
        cfg.generate_access_token();
        cfg
    });

    // Show connection info
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                   🖥️  SYSTEM MONITOR SERVER                   ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║  gRPC:    {:51} ║",
        format!("{}:{}", args.address, args.port)
    );
    if !args.no_health {
        println!(
            "║  HTTP:    {:51} ║",
            format!("{}:{}", args.address, args.health_port)
        );
    }
    println!("╠══════════════════════════════════════════════════════════════╣");
    if config.enable_authentication {
        println!("║  🔐 Authentication: ENABLED                                   ║");
        println!("║                                                                ║");
        println!("║  Run 'monitor-server show-token' to display the access token  ║");
        println!("║                                                                ║");
        println!("║  Clients need: IP, Port, and Access Token to connect          ║");
    } else {
        println!("║  ⚠️  Authentication: DISABLED (any client can connect)         ║");
    }
    println!("╠══════════════════════════════════════════════════════════════╣");
    if tls::is_tls_available(&config.tls) {
        println!("║  🔒 TLS:        ENABLED (encrypted gRPC)                      ║");
        if config.tls.as_ref().unwrap().ca_path.is_some() {
            println!("║  🔐 mTLS:       ENABLED (client cert required)                ║");
        }
    } else {
        println!("║  ⚠️  TLS:        DISABLED (plaintext gRPC)                     ║");
    }
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Commands: show-token, new-token, list-clients               ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Override config with command line arguments
    let server_config = ServerConfig {
        address: args.address,
        port: args.port,
        update_interval_seconds: config.update_interval_seconds,
        max_clients: config.max_clients,
        enable_authentication: config.enable_authentication,
        log_level: args.log_level,
    };

    // Create system monitor
    let monitor = SystemMonitor::new(
        server_config.update_interval_seconds,
        config.postgres_clusters.clone(),
        config.mariadb_clusters.clone(),
        config.systemd_units.clone(),
    )
    .await?;
    monitor.start_background_monitoring();

    // Wrap in Arc for sharing
    let monitor = std::sync::Arc::new(monitor);

    // Create service implementation with config for auth
    let service = MonitorServiceImpl::from_arc(monitor.clone(), config.clone())?;

    // Parse address
    let addr: SocketAddr = format!("{}:{}", server_config.address, server_config.port)
        .parse()
        .expect("Invalid address format");

    info!("gRPC server listening on {}", addr);

    // Start health check server in parallel (if enabled)
    if !args.no_health {
        let health_port = args.health_port;
        tokio::spawn(async move {
            if let Err(e) = health::start_health_server(health_port).await {
                tracing::error!("Health check server error: {}", e);
            }
        });
    }

    // Start the gRPC server (with optional TLS)
    let mut server_builder = if tls::is_tls_available(&config.tls) {
        info!("TLS enabled for gRPC server");
        let tls_config = tls::load_tls_config(config.tls.as_ref().unwrap())?;
        Server::builder().tls_config(tls_config)?
    } else {
        Server::builder()
    };

    server_builder
        .add_service(MonitorServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}

fn handle_command(cmd: &Commands, config_path: &str) -> Result<()> {
    if let Commands::Init { force } = cmd {
        return init_config(config_path, *force);
    }

    let mut config = Config::load(config_path)?;

    match cmd {
        Commands::Init { .. } => unreachable!(),
        Commands::ShowToken => {
            println!();
            println!("🔑 Access Token for clients:");
            println!();
            println!("   {}", config.access_token);
            println!();
            println!("Share this token with clients. They need:");
            println!("  • Server IP/hostname");
            println!("  • Port (default: 50051)");
            println!("  • This access token");
            println!();
        }
        Commands::NewToken => {
            config.generate_access_token();
            config.save(config_path)?;
            println!();
            println!("✅ New access token generated:");
            println!();
            println!("   {}", config.access_token);
            println!();
            println!("⚠️  Old token is now invalid. Update your clients!");
            println!();
        }
        Commands::ListClients => {
            println!();
            if config.authorized_clients.is_empty() {
                println!("📋 No authorized clients yet.");
                println!();
                println!("Clients will be authorized automatically when they connect");
                println!("with the correct access token.");
            } else {
                println!("📋 Authorized Clients:");
                println!();
                for client in &config.authorized_clients {
                    println!("  • {} (authorized: {})", client.name, client.authorized_at);
                }
            }
            println!();
        }
        Commands::RemoveClient { name } => {
            if config.remove_authorized_client(name) {
                config.save(config_path)?;
                println!("✅ Client '{}' removed.", name);
            } else {
                println!("❌ Client '{}' not found.", name);
            }
        }
        Commands::DisableAuth => {
            config.enable_authentication = false;
            config.save(config_path)?;
            println!();
            println!("⚠️  Authentication DISABLED");
            println!("Any client can now connect without a token.");
            println!();
        }
        Commands::EnableAuth => {
            config.enable_authentication = true;
            if config.access_token.is_empty() {
                config.generate_access_token();
            }
            config.save(config_path)?;
            println!();
            println!("✅ Authentication ENABLED");
            println!();
            println!("Access Token: {}", config.access_token);
            println!();
        }
    }

    Ok(())
}

fn init_config(config_path: &str, force: bool) -> Result<()> {
    use std::io::{self, Write};
    use std::path::Path;

    let path = Path::new(config_path);
    if path.exists() && !force && !prompt_bool("Config exists. Update it?", true)? {
        println!("Keeping existing config: {}", config_path);
        return Ok(());
    }

    let mut config = if path.exists() {
        Config::load(path)?
    } else {
        Config::default()
    };

    println!();
    println!("Code Monitor server setup");
    println!();

    config.update_interval_seconds = prompt_u64(
        "Update interval seconds",
        config.update_interval_seconds,
    )?;
    config.max_clients = prompt_usize("Max clients", config.max_clients)?;
    config.enable_authentication = prompt_bool(
        "Enable token authentication",
        config.enable_authentication,
    )?;

    if config.enable_authentication
        && (config.access_token.is_empty() || prompt_bool("Generate a new access token", false)?)
    {
        config.generate_access_token();
    }

    if prompt_bool("Enable TLS paths in config", config.tls.is_some())? {
        let current = config.tls.clone();
        let cert_default = current
            .as_ref()
            .map(|tls| tls.cert_path.as_str())
            .unwrap_or("/etc/code-monitor/certs/server.crt");
        let key_default = current
            .as_ref()
            .map(|tls| tls.key_path.as_str())
            .unwrap_or("/etc/code-monitor/certs/server.key");
        let ca_default = current
            .as_ref()
            .and_then(|tls| tls.ca_path.as_deref())
            .unwrap_or("");

        let cert_path = prompt_string("Server certificate path", cert_default)?;
        let key_path = prompt_string("Server private key path", key_default)?;
        let ca_path = prompt_string("Client CA path for mTLS (blank to disable)", ca_default)?;

        config.tls = Some(config::TlsConfig {
            cert_path,
            key_path,
            ca_path: blank_to_none(ca_path),
        });
    } else {
        config.tls = None;
    }

    let units_default = if config.systemd_units.is_empty() {
        String::new()
    } else {
        config.systemd_units.join(",")
    };
    let units = prompt_string("systemd units to watch, comma-separated (blank for none)", &units_default)?;
    config.systemd_units = units
        .split(',')
        .map(str::trim)
        .filter(|unit| !unit.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    config.save(path)?;

    println!();
    println!("Server config written to {}", config_path);
    if config.enable_authentication {
        println!();
        println!("Access token:");
        println!("{}", config.access_token);
        println!();
        println!("Use this token when adding the server on monitor-client.");
    }
    println!();
    println!("Start server:");
    println!("monitor-server --config {}", config_path);
    println!();

    io::stdout().flush()?;
    Ok(())
}

fn prompt_string(label: &str, default: &str) -> Result<String> {
    use std::io::{self, Write};

    if default.is_empty() {
        print!("{}: ", label);
    } else {
        print!("{} [{}]: ", label, default);
    }
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();
    if input.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(input.to_string())
    }
}

fn prompt_bool(label: &str, default: bool) -> Result<bool> {
    let suffix = if default { "Y/n" } else { "y/N" };
    loop {
        let answer = prompt_string(&format!("{} ({})", label, suffix), "")?;
        if answer.is_empty() {
            return Ok(default);
        }
        match answer.to_ascii_lowercase().as_str() {
            "y" | "yes" | "s" | "sim" => return Ok(true),
            "n" | "no" | "nao" | "não" => return Ok(false),
            _ => println!("Please answer yes or no."),
        }
    }
}

fn prompt_u64(label: &str, default: u64) -> Result<u64> {
    loop {
        let answer = prompt_string(label, &default.to_string())?;
        match answer.parse() {
            Ok(value) => return Ok(value),
            Err(_) => println!("Please enter a valid number."),
        }
    }
}

fn prompt_usize(label: &str, default: usize) -> Result<usize> {
    loop {
        let answer = prompt_string(label, &default.to_string())?;
        match answer.parse() {
            Ok(value) => return Ok(value),
            Err(_) => println!("Please enter a valid number."),
        }
    }
}

fn blank_to_none(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn create_test_config_file() -> (NamedTempFile, String) {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let mut config = Config::default();
        config.access_token = "test-token-123".to_string();
        config.save(&path).unwrap();
        (temp_file, path)
    }

    #[test]
    fn test_handle_command_show_token() {
        let (_temp, path) = create_test_config_file();
        let result = handle_command(&Commands::ShowToken, &path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_command_new_token() {
        let (_temp, path) = create_test_config_file();
        let old_config = Config::load(&path).unwrap();
        let old_token = old_config.access_token.clone();

        let result = handle_command(&Commands::NewToken, &path);
        assert!(result.is_ok());

        let new_config = Config::load(&path).unwrap();
        assert_ne!(new_config.access_token, old_token);
        assert!(!new_config.access_token.is_empty());
    }

    #[test]
    fn test_handle_command_disable_auth() {
        let (_temp, path) = create_test_config_file();
        let result = handle_command(&Commands::DisableAuth, &path);
        assert!(result.is_ok());

        let config = Config::load(&path).unwrap();
        assert!(!config.enable_authentication);
    }

    #[test]
    fn test_handle_command_enable_auth() {
        let (_temp, path) = create_test_config_file();
        // First disable
        handle_command(&Commands::DisableAuth, &path).unwrap();

        // Then enable
        let result = handle_command(&Commands::EnableAuth, &path);
        assert!(result.is_ok());

        let config = Config::load(&path).unwrap();
        assert!(config.enable_authentication);
        assert!(!config.access_token.is_empty());
    }

    #[test]
    fn test_handle_command_enable_auth_generates_token() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_string_lossy().to_string();
        let mut config = Config::default();
        config.enable_authentication = false;
        config.access_token = String::new();
        config.save(&path).unwrap();

        let result = handle_command(&Commands::EnableAuth, &path);
        assert!(result.is_ok());

        let config = Config::load(&path).unwrap();
        assert!(config.enable_authentication);
        assert!(!config.access_token.is_empty());
    }

    #[test]
    fn test_handle_command_list_clients_empty() {
        let (_temp, path) = create_test_config_file();
        let result = handle_command(&Commands::ListClients, &path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_command_remove_client() {
        let (_temp, path) = create_test_config_file();
        let mut config = Config::load(&path).unwrap();
        config.authorized_clients.push(config::AuthorizedClient {
            name: "test-client".to_string(),
            public_key: "pk123".to_string(),
            authorized_at: "2024-01-01T00:00:00Z".to_string(),
        });
        config.save(&path).unwrap();

        let result = handle_command(&Commands::RemoveClient { name: "test-client".to_string() }, &path);
        assert!(result.is_ok());

        let config = Config::load(&path).unwrap();
        assert!(config.authorized_clients.is_empty());
    }

    #[test]
    fn test_handle_command_remove_client_not_found() {
        let (_temp, path) = create_test_config_file();
        let result = handle_command(&Commands::RemoveClient { name: "nonexistent".to_string() }, &path);
        assert!(result.is_ok());
    }
}
