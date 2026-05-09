//! Shared data structures and protocols for the system monitoring application
//!
//! This module contains common data structures, error types, and protocol definitions
//! used by both the server and client components.

pub mod proto {
    pub mod monitoring {
        include!(concat!(env!("OUT_DIR"), "/monitoring.rs"));
    }
}

pub mod alerts;
pub mod notifications;

pub mod error {
    use thiserror::Error;
    use tonic::Status;

    #[derive(Error, Debug)]
    pub enum MonitorError {
        #[error("System information error: {0}")]
        SystemInfo(String),

        #[error("Network error: {0}")]
        Network(String),

        #[error("Authentication error: {0}")]
        Auth(String),

        #[error("Configuration error: {0}")]
        Config(String),

        #[error("IO error: {0}")]
        Io(String),

        #[error("Internal error: {0}")]
        Internal(String),
    }

    impl From<MonitorError> for Status {
        fn from(error: MonitorError) -> Self {
            match error {
                MonitorError::SystemInfo(_) => {
                    Status::failed_precondition("System information unavailable")
                }
                MonitorError::Network(_) => Status::unavailable("Network connection failed"),
                MonitorError::Auth(_) => Status::unauthenticated("Authentication failed"),
                MonitorError::Config(_) => Status::invalid_argument("Invalid configuration"),
                MonitorError::Io(_) => Status::internal("IO error"),
                MonitorError::Internal(_) => Status::internal("Internal error"),
            }
        }
    }
}

pub mod types {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    /// System information structure
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SystemInfo {
        pub hostname: String,
        pub os: String,
        pub kernel_version: String,
        pub uptime_seconds: u64,
        pub cpu_count: u32,
        pub cpu_usage_percent: f64,
        pub memory_total_bytes: u64,
        pub memory_used_bytes: u64,
        pub memory_available_bytes: u64,
        pub disk_info: Vec<DiskInfo>,
        pub timestamp: DateTime<Utc>,
    }

    /// Disk information structure
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DiskInfo {
        pub device: String,
        pub mount_point: String,
        pub filesystem_type: String,
        pub total_bytes: u64,
        pub used_bytes: u64,
        pub available_bytes: u64,
        pub usage_percent: f64,
    }

    /// Process information structure
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ProcessInfo {
        pub pid: u32,
        pub name: String,
        pub user: String,
        pub cpu_usage_percent: f64,
        pub memory_usage_bytes: u64,
        pub command_line: String,
        pub start_time: DateTime<Utc>,
        pub status: String,
    }

    /// Service information structure
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ServiceInfo {
        pub name: String,
        pub status: ServiceStatus,
        pub pid: Option<u32>,
        pub cpu_usage_percent: f64,
        pub memory_usage_bytes: u64,
        pub user: String,
        pub uptime_seconds: Option<u64>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum ServiceStatus {
        Running,
        Stopped,
        Failed,
        Unknown,
    }

    /// Network connection information
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct NetworkInfo {
        pub interface: String,
        pub ip_address: String,
        pub mac_address: String,
        pub is_up: bool,
        pub bytes_sent: u64,
        pub bytes_received: u64,
        pub packets_sent: u64,
        pub packets_received: u64,
    }

    /// Server configuration
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ServerConfig {
        pub address: String,
        pub port: u16,
        pub update_interval_seconds: u64,
        pub max_clients: usize,
        pub enable_authentication: bool,
        pub log_level: String,
    }

    /// Client TLS configuration
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ClientTlsConfig {
        /// Path to CA certificate (PEM) for verifying server
        pub ca_cert_path: String,
        /// Path to client certificate (PEM) for mTLS (optional)
        pub client_cert_path: Option<String>,
        /// Path to client private key (PEM) for mTLS (optional)
        pub client_key_path: Option<String>,
        /// Skip hostname verification (dangerous, for testing only)
        #[serde(default)]
        pub danger_skip_verify: bool,
    }

    /// Client configuration
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ClientConfig {
        pub servers: Vec<ServerEndpoint>,
        pub update_interval_seconds: u64,
        pub auto_reconnect: bool,
        pub reconnect_delay_seconds: u64,
        pub private_key_path: Option<String>,
        pub public_key_path: Option<String>,
        /// TLS configuration for client connections
        #[serde(default)]
        pub tls: Option<ClientTlsConfig>,
        /// Notification channels for alerts
        #[serde(default)]
        pub notifications: crate::notifications::NotificationConfig,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ServerEndpoint {
        pub id: Uuid,
        pub name: String,
        pub address: String,
        pub port: u16,
        #[serde(default)]
        pub description: Option<String>,
        /// Access token for authentication with this server
        #[serde(default)]
        pub access_token: Option<String>,
    }

    /// Container information
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ContainerInfo {
        pub id: String,
        pub name: String,
        pub image: String,
        pub status: String,
        pub state: String,
        pub health: String,
        pub cpu_percent: f64,
        pub memory_usage_bytes: u64,
        pub memory_limit_bytes: u64,
        pub memory_percent: f64,
        pub restart_count: u32,
        pub network_rx_bytes: u64,
        pub network_tx_bytes: u64,
        pub networks: Vec<String>,
    }

    /// Authentication token
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct AuthToken {
        pub token: String,
        pub expires_at: DateTime<Utc>,
        pub server_id: Uuid,
    }

    /// Postgres cluster metrics
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PostgresClusterInfo {
        pub name: String,
        pub host: String,
        pub port: u16,
        pub databases: Vec<PostgresDatabaseInfo>,
        pub connections_total: u32,
        pub connections_by_state: Vec<ConnectionStateCount>,
        pub cache_hit_ratio: f64,
        pub top_queries: Vec<TopQuery>,
        pub timestamp: DateTime<Utc>,
    }

    /// Postgres database information
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PostgresDatabaseInfo {
        pub name: String,
        pub size_bytes: u64,
        pub num_backends: u32,
        pub cache_hit_ratio: f64,
    }

    /// Connection state count
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ConnectionStateCount {
        pub state: String,
        pub count: u32,
    }

    /// Top query information
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct TopQuery {
        pub query: String,
        pub calls: u64,
        pub total_exec_time_ms: f64,
        pub mean_exec_time_ms: f64,
    }

    /// MariaDB cluster metrics
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MariaDBClusterInfo {
        pub name: String,
        pub host: String,
        pub port: u16,
        pub schemas: Vec<MariaDBSchemaInfo>,
        pub connections_active: u32,
        pub connections_total: u32,
        pub innodb_status: Option<String>,
        pub processes: Vec<MariaDBProcessInfo>,
        pub timestamp: DateTime<Utc>,
    }

    /// MariaDB schema information
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MariaDBSchemaInfo {
        pub name: String,
        pub size_bytes: u64,
        pub table_count: u32,
    }

    /// MariaDB process information
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MariaDBProcessInfo {
        pub id: u64,
        pub user: String,
        pub host: String,
        pub db: Option<String>,
        pub command: String,
        pub time_seconds: u32,
        pub state: String,
        pub info: Option<String>,
    }

    /// systemd unit information
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SystemdUnitInfo {
        pub name: String,
        pub status: String,
        pub is_active: bool,
        pub pid: Option<u32>,
        pub memory_current_bytes: u64,
        pub started_at: Option<DateTime<Utc>>,
    }
}

#[cfg(test)]
mod tests {
    use super::error::MonitorError;
    use super::types::*;
    use chrono::Utc;
    use tonic::Status;

    #[test]
    fn test_system_info_serialization() {
        let info = SystemInfo {
            hostname: "test-host".to_string(),
            os: "Linux".to_string(),
            kernel_version: "5.15".to_string(),
            uptime_seconds: 3600,
            cpu_count: 4,
            cpu_usage_percent: 25.5,
            memory_total_bytes: 16_000_000_000,
            memory_used_bytes: 8_000_000_000,
            memory_available_bytes: 8_000_000_000,
            disk_info: vec![],
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: SystemInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info.hostname, deserialized.hostname);
        assert_eq!(info.cpu_count, deserialized.cpu_count);
    }

    #[test]
    fn test_disk_info_serialization() {
        let disk = DiskInfo {
            device: "/dev/sda1".to_string(),
            mount_point: "/".to_string(),
            filesystem_type: "ext4".to_string(),
            total_bytes: 1_000_000,
            used_bytes: 500_000,
            available_bytes: 500_000,
            usage_percent: 50.0,
        };

        let json = serde_json::to_string(&disk).unwrap();
        let deserialized: DiskInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(disk.device, deserialized.device);
        assert_eq!(disk.usage_percent, deserialized.usage_percent);
    }

    #[test]
    fn test_process_info_serialization() {
        let proc = ProcessInfo {
            pid: 1234,
            name: "test-process".to_string(),
            user: "root".to_string(),
            cpu_usage_percent: 10.0,
            memory_usage_bytes: 1024,
            command_line: "/bin/test".to_string(),
            start_time: Utc::now(),
            status: "Running".to_string(),
        };

        let json = serde_json::to_string(&proc).unwrap();
        let deserialized: ProcessInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(proc.pid, deserialized.pid);
        assert_eq!(proc.name, deserialized.name);
    }

    #[test]
    fn test_container_info_serialization() {
        let container = ContainerInfo {
            id: "abc123".to_string(),
            name: "test-container".to_string(),
            image: "test-image".to_string(),
            status: "running".to_string(),
            state: "running".to_string(),
            health: "healthy".to_string(),
            cpu_percent: 5.0,
            memory_usage_bytes: 1024,
            memory_limit_bytes: 2048,
            memory_percent: 50.0,
            restart_count: 0,
            network_rx_bytes: 100,
            network_tx_bytes: 200,
            networks: vec!["bridge".to_string()],
        };

        let json = serde_json::to_string(&container).unwrap();
        let deserialized: ContainerInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(container.id, deserialized.id);
        assert_eq!(container.networks, deserialized.networks);
    }

    #[test]
    fn test_postgres_cluster_info_serialization() {
        let cluster = PostgresClusterInfo {
            name: "pg-main".to_string(),
            host: "localhost".to_string(),
            port: 5432,
            databases: vec![PostgresDatabaseInfo {
                name: "app".to_string(),
                size_bytes: 1_000_000,
                num_backends: 5,
                cache_hit_ratio: 99.5,
            }],
            connections_total: 10,
            connections_by_state: vec![ConnectionStateCount {
                state: "active".to_string(),
                count: 5,
            }],
            cache_hit_ratio: 99.5,
            top_queries: vec![TopQuery {
                query: "SELECT * FROM users".to_string(),
                calls: 100,
                total_exec_time_ms: 1000.0,
                mean_exec_time_ms: 10.0,
            }],
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&cluster).unwrap();
        let deserialized: PostgresClusterInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(cluster.name, deserialized.name);
        assert_eq!(cluster.databases.len(), deserialized.databases.len());
    }

    #[test]
    fn test_mariadb_cluster_info_serialization() {
        let cluster = MariaDBClusterInfo {
            name: "mdb-main".to_string(),
            host: "localhost".to_string(),
            port: 3306,
            schemas: vec![MariaDBSchemaInfo {
                name: "app".to_string(),
                size_bytes: 1_000_000,
                table_count: 10,
            }],
            connections_active: 5,
            connections_total: 10,
            innodb_status: Some("OK".to_string()),
            processes: vec![MariaDBProcessInfo {
                id: 1,
                user: "root".to_string(),
                host: "localhost".to_string(),
                db: Some("app".to_string()),
                command: "Query".to_string(),
                time_seconds: 0,
                state: "executing".to_string(),
                info: Some("SELECT 1".to_string()),
            }],
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&cluster).unwrap();
        let deserialized: MariaDBClusterInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(cluster.name, deserialized.name);
        assert_eq!(cluster.schemas.len(), deserialized.schemas.len());
    }

    #[test]
    fn test_systemd_unit_info_serialization() {
        let unit = SystemdUnitInfo {
            name: "nginx.service".to_string(),
            status: "active".to_string(),
            is_active: true,
            pid: Some(1234),
            memory_current_bytes: 1024,
            started_at: Some(Utc::now()),
        };

        let json = serde_json::to_string(&unit).unwrap();
        let deserialized: SystemdUnitInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(unit.name, deserialized.name);
        assert_eq!(unit.is_active, deserialized.is_active);
    }

    #[test]
    fn test_client_tls_config_serialization() {
        let config = ClientTlsConfig {
            ca_cert_path: "/etc/certs/ca.pem".to_string(),
            client_cert_path: Some("/etc/certs/client.pem".to_string()),
            client_key_path: Some("/etc/certs/client.key".to_string()),
            danger_skip_verify: true,
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ClientTlsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.ca_cert_path, deserialized.ca_cert_path);
        assert!(deserialized.danger_skip_verify);
    }

    #[test]
    fn test_service_status_variants() {
        assert!(matches!(ServiceStatus::Running, ServiceStatus::Running));
        assert!(matches!(ServiceStatus::Stopped, ServiceStatus::Stopped));
        assert!(matches!(ServiceStatus::Failed, ServiceStatus::Failed));
        assert!(matches!(ServiceStatus::Unknown, ServiceStatus::Unknown));
    }

    #[test]
    fn test_monitor_error_into_status_system_info() {
        let err = MonitorError::SystemInfo("test".into());
        let status: Status = err.into();
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(status.message(), "System information unavailable");
    }

    #[test]
    fn test_monitor_error_into_status_network() {
        let err = MonitorError::Network("test".into());
        let status: Status = err.into();
        assert_eq!(status.code(), tonic::Code::Unavailable);
        assert_eq!(status.message(), "Network connection failed");
    }

    #[test]
    fn test_monitor_error_into_status_auth() {
        let err = MonitorError::Auth("test".into());
        let status: Status = err.into();
        assert_eq!(status.code(), tonic::Code::Unauthenticated);
        assert_eq!(status.message(), "Authentication failed");
    }

    #[test]
    fn test_monitor_error_into_status_config() {
        let err = MonitorError::Config("test".into());
        let status: Status = err.into();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        assert_eq!(status.message(), "Invalid configuration");
    }

    #[test]
    fn test_monitor_error_into_status_io() {
        let err = MonitorError::Io("test".into());
        let status: Status = err.into();
        assert_eq!(status.code(), tonic::Code::Internal);
        assert_eq!(status.message(), "IO error");
    }

    #[test]
    fn test_monitor_error_into_status_internal() {
        let err = MonitorError::Internal("test".into());
        let status: Status = err.into();
        assert_eq!(status.code(), tonic::Code::Internal);
        assert_eq!(status.message(), "Internal error");
    }
}
