//! CLI command definitions and argument parsing

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "monitor-client")]
#[command(about = "System monitoring client - Run without arguments to open interactive dashboard")]
#[command(version = "0.1.0")]
pub struct CliArgs {
    /// Configuration file path
    #[arg(short, long, default_value = "client-config.toml")]
    pub config: PathBuf,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    pub log_level: String,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Add a new server to monitor
    Add {
        /// Server name (friendly name)
        #[arg(short, long)]
        name: String,

        /// Server address (IP or hostname)
        #[arg(short, long)]
        address: String,

        /// Server port
        #[arg(short, long, default_value = "50051")]
        port: u16,

        /// Access token from the server (run 'monitor-server show-token' on server)
        #[arg(short = 't', long)]
        token: Option<String>,

        /// Server description
        #[arg(short, long)]
        description: Option<String>,
    },

    /// Remove a server from monitoring
    Remove {
        /// Server ID to remove
        #[arg(short, long)]
        id: Uuid,
    },

    /// List configured servers
    List,

    /// Update server configuration
    SetToken {
        /// Server ID to update
        #[arg(short, long)]
        id: Uuid,

        /// New access token
        #[arg(short, long)]
        token: String,
    },

    /// Start monitoring servers
    Monitor {
        /// Specific server ID to monitor (optional, monitors all if not specified)
        #[arg(short, long)]
        server: Option<Uuid>,

        /// Update interval in seconds
        #[arg(short, long, default_value = "5")]
        interval: u64,
    },

    /// Start client in server mode (for remote access)
    Serve {
        /// Address to bind to
        #[arg(short, long, default_value = "127.0.0.1")]
        address: String,

        /// Port to listen on
        #[arg(short, long, default_value = "50052")]
        port: u16,
    },

    /// Open interactive dashboard to manage and monitor servers
    Dashboard,

    /// List services/processes from a server
    Services {
        /// Specific server ID to query (optional, uses first configured server if not specified)
        #[arg(short, long)]
        server: Option<Uuid>,

        /// Limit number of services to show
        #[arg(short, long, default_value = "20")]
        limit: u32,
    },

    /// List running processes from a server
    Processes {
        /// Specific server ID to query (optional, uses first configured server if not specified)
        #[arg(short, long)]
        server: Option<Uuid>,

        /// Limit number of processes to show
        #[arg(short, long, default_value = "20")]
        limit: u32,

        /// Filter processes by name
        #[arg(short, long)]
        filter: Option<String>,
    },

    /// Quick connect: Add server interactively
    Connect {
        /// Server address (IP or hostname)
        address: String,

        /// Server port
        #[arg(short, long, default_value = "50051")]
        port: u16,
    },

    /// Export metrics history to file
    Export {
        /// Server ID to export (optional, exports all if not specified)
        #[arg(short, long)]
        server: Option<Uuid>,

        /// Output format (csv or json)
        #[arg(short, long, default_value = "csv")]
        format: String,

        /// Output file path
        #[arg(short, long)]
        output: PathBuf,

        /// Hours of history to export (default: 24)
        #[arg(short = 'H', long, default_value = "24")]
        hours: i64,
    },

    /// Show storage statistics
    StorageStats,

    /// Purge old metrics data
    Purge {
        /// Days to keep (older data will be deleted)
        #[arg(short, long, default_value = "7")]
        days: i64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli() {
        CliArgs::command()
            .try_get_matches_from(vec!["monitor-client", "list"])
            .unwrap();
    }

    #[test]
    fn verify_add_command() {
        let args = CliArgs::parse_from(vec![
            "monitor-client",
            "add",
            "--name",
            "test-server",
            "--address",
            "192.168.1.100",
            "--port",
            "50051",
        ]);

        match args.command {
            Some(Commands::Add {
                name,
                address,
                port,
                token,
                description,
            }) => {
                assert_eq!(name, "test-server");
                assert_eq!(address, "192.168.1.100");
                assert_eq!(port, 50051);
                assert_eq!(token, None);
                assert_eq!(description, None);
            }
            _ => panic!("Expected Add command"),
        }
    }

    #[test]
    fn verify_remove_command() {
        let test_id = Uuid::new_v4();
        let args = CliArgs::parse_from(vec![
            "monitor-client",
            "remove",
            "--id",
            &test_id.to_string(),
        ]);

        match args.command {
            Some(Commands::Remove { id }) => {
                assert_eq!(id, test_id);
            }
            _ => panic!("Expected Remove command"),
        }
    }

    #[test]
    fn verify_set_token_command() {
        let test_id = Uuid::new_v4();
        let args = CliArgs::parse_from(vec![
            "monitor-client",
            "set-token",
            "--id",
            &test_id.to_string(),
            "--token",
            "new-secret-token",
        ]);

        match args.command {
            Some(Commands::SetToken { id, token }) => {
                assert_eq!(id, test_id);
                assert_eq!(token, "new-secret-token");
            }
            _ => panic!("Expected SetToken command"),
        }
    }

    #[test]
    fn verify_connect_command() {
        let args = CliArgs::parse_from(vec![
            "monitor-client",
            "connect",
            "192.168.1.50",
            "--port",
            "50052",
        ]);

        match args.command {
            Some(Commands::Connect { address, port }) => {
                assert_eq!(address, "192.168.1.50");
                assert_eq!(port, 50052);
            }
            _ => panic!("Expected Connect command"),
        }
    }

    #[test]
    fn verify_monitor_command_defaults() {
        let args = CliArgs::parse_from(vec!["monitor-client", "monitor"]);

        match args.command {
            Some(Commands::Monitor { server, interval }) => {
                assert!(server.is_none());
                assert_eq!(interval, 5);
            }
            _ => panic!("Expected Monitor command"),
        }
    }

    #[test]
    fn verify_export_command() {
        let test_id = Uuid::new_v4();
        let args = CliArgs::parse_from(vec![
            "monitor-client",
            "export",
            "--server",
            &test_id.to_string(),
            "--format",
            "json",
            "--output",
            "/tmp/export.json",
            "--hours",
            "48",
        ]);

        match args.command {
            Some(Commands::Export {
                server,
                format,
                output,
                hours,
            }) => {
                assert_eq!(server, Some(test_id));
                assert_eq!(format, "json");
                assert_eq!(output, PathBuf::from("/tmp/export.json"));
                assert_eq!(hours, 48);
            }
            _ => panic!("Expected Export command"),
        }
    }

    #[test]
    fn verify_purge_command_defaults() {
        let args = CliArgs::parse_from(vec!["monitor-client", "purge"]);

        match args.command {
            Some(Commands::Purge { days }) => {
                assert_eq!(days, 7);
            }
            _ => panic!("Expected Purge command"),
        }
    }

    #[test]
    fn verify_dashboard_command() {
        let args = CliArgs::parse_from(vec!["monitor-client", "dashboard"]);
        assert!(matches!(args.command, Some(Commands::Dashboard)));
    }

    #[test]
    fn verify_storage_stats_command() {
        let args = CliArgs::parse_from(vec!["monitor-client", "storage-stats"]);
        assert!(matches!(args.command, Some(Commands::StorageStats)));
    }

    #[test]
    fn verify_cli_default_config_path() {
        let args = CliArgs::parse_from(vec!["monitor-client"]);
        assert_eq!(args.config, PathBuf::from("client-config.toml"));
        assert_eq!(args.log_level, "info");
    }

    #[test]
    fn verify_list_command() {
        let args = CliArgs::parse_from(vec!["monitor-client", "list"]);
        assert!(matches!(args.command, Some(Commands::List)));
    }
}
