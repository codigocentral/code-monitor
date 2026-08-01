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

/// Conversions between the wire types and the domain types
///
/// Kept here so server and client cannot drift apart in how they map an enum.
pub mod convert {
    use crate::proto::monitoring::BindScope as ProtoBindScope;
    use crate::types::BindScope;

    impl From<BindScope> for ProtoBindScope {
        fn from(scope: BindScope) -> Self {
            match scope {
                BindScope::Loopback => ProtoBindScope::Loopback,
                BindScope::Private => ProtoBindScope::Private,
                BindScope::AllInterfaces => ProtoBindScope::AllInterfaces,
                BindScope::Public => ProtoBindScope::Public,
                BindScope::Unknown => ProtoBindScope::Unspecified,
            }
        }
    }

    impl From<ProtoBindScope> for BindScope {
        fn from(scope: ProtoBindScope) -> Self {
            match scope {
                ProtoBindScope::Loopback => BindScope::Loopback,
                ProtoBindScope::Private => BindScope::Private,
                ProtoBindScope::AllInterfaces => BindScope::AllInterfaces,
                ProtoBindScope::Public => BindScope::Public,
                ProtoBindScope::Unspecified => BindScope::Unknown,
            }
        }
    }

    impl BindScope {
        /// Decode the wire representation, treating an unknown value as
        /// [`BindScope::Unknown`] rather than failing the whole response.
        pub fn from_wire(value: i32) -> Self {
            ProtoBindScope::try_from(value)
                .map(BindScope::from)
                .unwrap_or(BindScope::Unknown)
        }

        /// Encode for the wire.
        pub fn to_wire(self) -> i32 {
            ProtoBindScope::from(self) as i32
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_bind_scope_round_trip() {
            for scope in [
                BindScope::Loopback,
                BindScope::Private,
                BindScope::AllInterfaces,
                BindScope::Public,
                BindScope::Unknown,
            ] {
                assert_eq!(BindScope::from_wire(scope.to_wire()), scope);
            }
        }

        #[test]
        fn test_bind_scope_unknown_wire_value_is_not_fatal() {
            // A newer server sending a scope this build does not know must not
            // be read as "loopback", which would hide an exposure.
            assert_eq!(BindScope::from_wire(9999), BindScope::Unknown);
            assert!(!BindScope::from_wire(9999).is_exposed());
        }

        #[test]
        fn test_bind_scope_unspecified_maps_to_unknown() {
            assert_eq!(BindScope::from_wire(0), BindScope::Unknown);
        }
    }
}

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
        #[serde(default)]
        pub swap: SwapInfo,
        #[serde(default)]
        pub memory_pressure: Option<MemoryPressure>,
    }

    /// Swap usage and, more importantly, swap activity
    ///
    /// Occupancy alone says nothing about pressure: pages parked on disk since
    /// an old spike cost nothing until something touches them. The rates are
    /// what indicate a host that is paging right now.
    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct SwapInfo {
        pub total_bytes: u64,
        pub used_bytes: u64,
        pub in_pages_per_sec: f64,
        pub out_pages_per_sec: f64,
    }

    impl SwapInfo {
        /// Fraction of swap occupied, for display only — never for alerting.
        pub fn used_percent(&self) -> f64 {
            if self.total_bytes == 0 {
                0.0
            } else {
                (self.used_bytes as f64 / self.total_bytes as f64) * 100.0
            }
        }

        /// Combined paging rate, the figure worth alerting on.
        pub fn activity_pages_per_sec(&self) -> f64 {
            self.in_pages_per_sec + self.out_pages_per_sec
        }
    }

    /// Memory pressure from `/proc/pressure/memory`
    ///
    /// Absent on kernels built without `CONFIG_PSI`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MemoryPressure {
        /// Share of the last 60s where at least one task stalled on memory
        pub some_avg60: f64,
        /// Share of the last 60s where every task stalled
        pub full_avg60: f64,
        /// Cumulative full-stall time since boot, best for comparing hosts
        pub full_total_seconds: f64,
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
        /// `VmSwap` from `/proc/[pid]/status`; `None` where unreadable, which is
        /// not the same as zero
        #[serde(default)]
        pub swap_bytes: Option<u64>,
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
        /// `None` when the container has no memory limit
        ///
        /// Docker reports the host's total RAM as the limit in that case, so a
        /// percentage computed from it makes an unprotected container look
        /// comfortable. See [`Self::memory_limit_set`].
        pub memory_percent: Option<f64>,
        pub restart_count: u32,
        pub network_rx_bytes: u64,
        pub network_tx_bytes: u64,
        pub networks: Vec<String>,
        /// Whether `HostConfig.Memory` is actually set on the container
        #[serde(default)]
        pub memory_limit_set: bool,
        /// Health check detail, needed to tell a sick app from a healthcheck
        /// that never worked
        #[serde(default)]
        pub health_detail: Option<ContainerHealthDetail>,
        /// Swap held by the container's processes; `None` where unavailable
        #[serde(default)]
        pub swap_bytes: Option<u64>,
    }

    impl ContainerInfo {
        /// Whether clearing the host's swap would push this container past its
        /// limit — the pre-flight check for `swapoff`.
        ///
        /// Always false without a limit, since there is nothing to exceed.
        pub fn would_exceed_limit_on_swapoff(&self) -> bool {
            match (self.memory_limit_set, self.swap_bytes) {
                (true, Some(swap)) => {
                    self.memory_usage_bytes.saturating_add(swap) > self.memory_limit_bytes
                }
                _ => false,
            }
        }
    }

    /// Result of a container's last health check
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ContainerHealthDetail {
        /// Consecutive failures. A high number with a constant output means the
        /// check itself is broken, not the application.
        pub failing_streak: u32,
        /// Output of the last check, truncated
        pub last_output: String,
        pub last_exit_code: i32,
        pub last_checked_at: Option<DateTime<Utc>>,
    }

    /// What an unhealthy container is actually telling us
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum HealthAssessment {
        /// The last check passed
        Passing,
        /// Failing recently: worth waking someone
        FailingNow,
        /// The check never worked — a configuration defect, not an incident
        BrokenCheck,
    }

    impl ContainerHealthDetail {
        /// Failures beyond which a check is treated as broken rather than
        /// failing.
        ///
        /// At the usual 30s interval this is roughly a day of uninterrupted
        /// failure. Nothing genuinely sick survives that long untouched, while
        /// a misconfigured check racks it up quietly — the fleet audit found
        /// streaks from 1,344 to 162,460.
        pub const BROKEN_CHECK_STREAK: u32 = 2_500;

        /// Output fragments that mean the probe itself never ran.
        const BROKEN_CHECK_MARKERS: &'static [&'static str] = &[
            "not found",
            "no such file",
            "executable file not found",
            "oci runtime exec failed",
            "permission denied",
            "cannot exec",
        ];

        /// Tell a sick application apart from a healthcheck that never worked.
        ///
        /// This matters because it is what makes the signal usable: with 21
        /// containers permanently red for configuration reasons, a container
        /// that genuinely falls over goes unnoticed.
        pub fn assess(&self) -> HealthAssessment {
            if self.failing_streak == 0 {
                return HealthAssessment::Passing;
            }

            let output = self.last_output.to_lowercase();
            if Self::BROKEN_CHECK_MARKERS
                .iter()
                .any(|marker| output.contains(marker))
            {
                return HealthAssessment::BrokenCheck;
            }

            if self.failing_streak >= Self::BROKEN_CHECK_STREAK {
                return HealthAssessment::BrokenCheck;
            }

            HealthAssessment::FailingNow
        }
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

    /// A TLS certificate found on the host
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct TlsCertificateInfo {
        pub name: String,
        /// Subject Alternative Names
        pub domains: Vec<String>,
        pub issuer: String,
        pub not_after: Option<DateTime<Utc>>,
        /// Negative once expired
        pub days_until_expiry: i64,
        /// A file path, or `host:port` when probed over TLS
        pub source: String,
        /// Renewed forever although no active vhost serves it
        pub orphaned: bool,
    }

    /// Certificate inventory plus the health of whatever renews it
    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct TlsSnapshot {
        pub certificates: Vec<TlsCertificateInfo>,
        /// The renewal unit and its state. A certificate is only as safe as the
        /// timer that renews it.
        pub renewal_unit_name: Option<String>,
        pub renewal_unit_status: Option<String>,
    }

    /// How widely a listening socket is bound
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum BindScope {
        /// Reachable only from the host itself
        Loopback,
        /// A private address, such as RFC1918 or a VPN interface
        Private,
        /// `0.0.0.0` or `::` — every interface, including any public one
        AllInterfaces,
        /// A specific routable address: one interface, still reachable from
        /// outside the host
        Public,
        /// Could not be classified
        Unknown,
    }

    impl BindScope {
        /// Classify a bind address as printed by the kernel.
        pub fn classify(address: &str) -> Self {
            let addr = address.trim();

            // `ss` renders the wildcard as `*`, which is not a parseable address
            if addr == "*" {
                return BindScope::AllInterfaces;
            }

            match addr.parse::<std::net::IpAddr>() {
                Ok(std::net::IpAddr::V4(v4)) => {
                    if v4.is_unspecified() {
                        BindScope::AllInterfaces
                    } else if v4.is_loopback() {
                        BindScope::Loopback
                    } else if v4.is_private() || v4.is_link_local() {
                        BindScope::Private
                    } else {
                        BindScope::Public
                    }
                }
                Ok(std::net::IpAddr::V6(v6)) => {
                    if v6.is_unspecified() {
                        BindScope::AllInterfaces
                    } else if v6.is_loopback() {
                        BindScope::Loopback
                    } else if v6.segments()[0] & 0xfe00 == 0xfc00
                        || v6.segments()[0] & 0xffc0 == 0xfe80
                    {
                        // Unique local (fc00::/7) or link local (fe80::/10)
                        BindScope::Private
                    } else {
                        BindScope::Public
                    }
                }
                Err(_) => BindScope::Unknown,
            }
        }

        /// Whether this scope reaches beyond the host.
        ///
        /// A specific public address counts: binding a database to one routable
        /// interface is no safer than binding it to all of them.
        pub fn is_exposed(&self) -> bool {
            matches!(self, BindScope::AllInterfaces | BindScope::Public)
        }
    }

    /// A socket in the listening state
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ListeningPortInfo {
        pub address: String,
        pub port: u16,
        /// `tcp` or `tcp6`
        pub protocol: String,
        pub bind_scope: BindScope,
        pub pid: Option<u32>,
        pub process_name: String,
        /// A well-known database, cache or search port
        pub sensitive: bool,
    }

    impl ListeningPortInfo {
        /// Ports that have no business listening on every interface.
        pub const SENSITIVE_PORTS: &'static [u16] = &[
            5432,  // postgres
            6432,  // pgbouncer
            3306,  // mysql/mariadb
            6379,  // redis
            27017, // mongodb
            9200,  // elasticsearch
            11211, // memcached
            5672,  // rabbitmq
            9042,  // cassandra
            2379,  // etcd
        ];

        /// Whether the port is a well-known data store port.
        pub fn is_sensitive_port(port: u16) -> bool {
            Self::SENSITIVE_PORTS.contains(&port)
        }

        /// A data store reachable from outside the host: the finding that
        /// matters, as opposed to the same service on loopback.
        pub fn is_exposed_datastore(&self) -> bool {
            self.sensitive && self.bind_scope.is_exposed()
        }
    }

    /// Everything a single `GetSystemdInfo` call returns
    ///
    /// Both lists arrive in one response, so they travel together instead of
    /// costing a second round trip.
    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct SystemdSnapshot {
        /// Units explicitly configured for monitoring
        pub units: Vec<SystemdUnitInfo>,
        /// Every unit on the host currently in the `failed` state
        pub failed_units: Vec<SystemdFailedUnit>,
    }

    /// A systemd unit currently in the `failed` state
    ///
    /// Collected by scanning the whole host, independently of the units listed
    /// in the server configuration: a unit nobody thought to configure is
    /// exactly the one worth surfacing.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SystemdFailedUnit {
        pub name: String,
        pub description: String,
        /// When the unit entered its current state, when systemd reports it
        pub since: Option<DateTime<Utc>>,
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
            swap: SwapInfo::default(),
            memory_pressure: None,
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
            swap_bytes: None,
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
            memory_percent: Some(50.0),
            restart_count: 0,
            network_rx_bytes: 100,
            network_tx_bytes: 200,
            networks: vec!["bridge".to_string()],
            memory_limit_set: false,
            health_detail: None,
            swap_bytes: None,
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

    // ─────────────────────────────────────────
    // Bind scope classification
    // ─────────────────────────────────────────

    #[test]
    fn test_bind_scope_wildcard_is_all_interfaces() {
        assert_eq!(BindScope::classify("0.0.0.0"), BindScope::AllInterfaces);
        assert_eq!(BindScope::classify("::"), BindScope::AllInterfaces);
        assert_eq!(BindScope::classify("*"), BindScope::AllInterfaces);
    }

    #[test]
    fn test_bind_scope_loopback() {
        assert_eq!(BindScope::classify("127.0.0.1"), BindScope::Loopback);
        assert_eq!(BindScope::classify("127.0.0.53"), BindScope::Loopback);
        assert_eq!(BindScope::classify("::1"), BindScope::Loopback);
    }

    #[test]
    fn test_bind_scope_private_ranges() {
        // The VPN addresses the fleet binds to
        assert_eq!(BindScope::classify("10.10.0.9"), BindScope::Private);
        assert_eq!(BindScope::classify("192.168.1.10"), BindScope::Private);
        assert_eq!(BindScope::classify("172.17.0.1"), BindScope::Private);
        assert_eq!(BindScope::classify("169.254.1.1"), BindScope::Private);
    }

    #[test]
    fn test_bind_scope_private_ipv6() {
        assert_eq!(BindScope::classify("fd00::1"), BindScope::Private);
        assert_eq!(BindScope::classify("fe80::1"), BindScope::Private);
    }

    #[test]
    fn test_bind_scope_public_address() {
        // A routable address is bound to one interface but still reachable
        assert_eq!(BindScope::classify("203.0.113.7"), BindScope::Public);
        assert_eq!(BindScope::classify("2001:db8::1"), BindScope::Public);
    }

    #[test]
    fn test_bind_scope_unparseable_is_unknown() {
        assert_eq!(BindScope::classify("not-an-address"), BindScope::Unknown);
        assert_eq!(BindScope::classify(""), BindScope::Unknown);
    }

    #[test]
    fn test_bind_scope_trims_whitespace() {
        assert_eq!(BindScope::classify("  0.0.0.0  "), BindScope::AllInterfaces);
    }

    #[test]
    fn test_bind_scope_exposure() {
        assert!(BindScope::AllInterfaces.is_exposed());
        assert!(
            BindScope::Public.is_exposed(),
            "a database on one routable interface is no safer than on all of them"
        );
        assert!(!BindScope::Loopback.is_exposed());
        assert!(!BindScope::Private.is_exposed());
        assert!(
            !BindScope::Unknown.is_exposed(),
            "an unclassifiable address must not be reported as an exposure"
        );
    }

    // ─────────────────────────────────────────
    // Listening ports
    // ─────────────────────────────────────────

    fn listening_port(port: u16, address: &str) -> ListeningPortInfo {
        ListeningPortInfo {
            address: address.to_string(),
            port,
            protocol: "tcp".to_string(),
            bind_scope: BindScope::classify(address),
            pid: Some(1234),
            process_name: "postgres".to_string(),
            sensitive: ListeningPortInfo::is_sensitive_port(port),
        }
    }

    #[test]
    fn test_sensitive_ports_cover_common_datastores() {
        assert!(ListeningPortInfo::is_sensitive_port(5432)); // postgres
        assert!(ListeningPortInfo::is_sensitive_port(3306)); // mysql
        assert!(ListeningPortInfo::is_sensitive_port(6379)); // redis
        assert!(ListeningPortInfo::is_sensitive_port(6432)); // pgbouncer
        assert!(!ListeningPortInfo::is_sensitive_port(443));
        assert!(!ListeningPortInfo::is_sensitive_port(50051));
    }

    #[test]
    fn test_exposed_datastore_detected() {
        let pg = listening_port(5432, "0.0.0.0");
        assert!(
            pg.is_exposed_datastore(),
            "postgres on 0.0.0.0 is the finding the audit was after"
        );
    }

    #[test]
    fn test_datastore_on_loopback_is_not_a_finding() {
        assert!(!listening_port(5432, "127.0.0.1").is_exposed_datastore());
    }

    #[test]
    fn test_datastore_on_vpn_is_not_a_finding() {
        assert!(!listening_port(5432, "10.10.0.9").is_exposed_datastore());
    }

    #[test]
    fn test_exposed_non_datastore_is_not_flagged() {
        // The monitor's own gRPC port is exposed but not a datastore; it is
        // reported, just not as a sensitive-port finding.
        let port = listening_port(50051, "0.0.0.0");
        assert!(!port.is_exposed_datastore());
        assert!(port.bind_scope.is_exposed());
    }

    // ─────────────────────────────────────────
    // Swap and memory pressure
    // ─────────────────────────────────────────

    #[test]
    fn test_swap_used_percent() {
        let swap = SwapInfo {
            total_bytes: 8_000_000_000,
            used_bytes: 4_000_000_000,
            ..Default::default()
        };
        assert!((swap.used_percent() - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_swap_used_percent_without_swap() {
        assert_eq!(SwapInfo::default().used_percent(), 0.0);
    }

    #[test]
    fn test_swap_activity_is_independent_of_occupancy() {
        // The audit's central lesson: a host can sit at 65% occupancy with no
        // paging at all, while another at 55% thrashes.
        let idle = SwapInfo {
            total_bytes: 8_000_000_000,
            used_bytes: 5_200_000_000,
            in_pages_per_sec: 0.0,
            out_pages_per_sec: 0.0,
        };
        let thrashing = SwapInfo {
            total_bytes: 8_000_000_000,
            used_bytes: 4_400_000_000,
            in_pages_per_sec: 180.0,
            out_pages_per_sec: 220.0,
        };

        assert!(idle.used_percent() > thrashing.used_percent());
        assert_eq!(idle.activity_pages_per_sec(), 0.0);
        assert_eq!(thrashing.activity_pages_per_sec(), 400.0);
    }

    // ─────────────────────────────────────────
    // Container swapoff pre-flight
    // ─────────────────────────────────────────

    fn container_with(limit_set: bool, limit: u64, usage: u64, swap: Option<u64>) -> ContainerInfo {
        ContainerInfo {
            id: "abc".to_string(),
            name: "sonarqube".to_string(),
            image: "sonarqube:latest".to_string(),
            status: "Up".to_string(),
            state: "running".to_string(),
            health: "healthy".to_string(),
            cpu_percent: 1.0,
            memory_usage_bytes: usage,
            memory_limit_bytes: limit,
            memory_percent: None,
            restart_count: 0,
            network_rx_bytes: 0,
            network_tx_bytes: 0,
            networks: vec![],
            memory_limit_set: limit_set,
            health_detail: None,
            swap_bytes: swap,
        }
    }

    // ─────────────────────────────────────────
    // Health assessment
    // ─────────────────────────────────────────

    fn health(streak: u32, output: &str) -> ContainerHealthDetail {
        ContainerHealthDetail {
            failing_streak: streak,
            last_output: output.to_string(),
            last_exit_code: if streak == 0 { 0 } else { 1 },
            last_checked_at: None,
        }
    }

    #[test]
    fn test_health_passing_when_streak_is_zero() {
        assert_eq!(health(0, "").assess(), HealthAssessment::Passing);
    }

    #[test]
    fn test_health_failing_now_for_a_short_streak() {
        assert_eq!(
            health(3, "HTTP 503 Service Unavailable").assess(),
            HealthAssessment::FailingNow
        );
    }

    #[test]
    fn test_health_missing_probe_binary_is_a_broken_check() {
        // The dominant cause across the fleet: the image has no curl
        assert_eq!(
            health(1, "curl: not found").assess(),
            HealthAssessment::BrokenCheck
        );
    }

    #[test]
    fn test_health_broken_check_markers() {
        for output in [
            "OCI runtime exec failed: exec failed",
            "exec: \"wget\": executable file not found in $PATH",
            "/bin/sh: 1: nc: not found",
            "permission denied",
        ] {
            assert_eq!(
                health(5, output).assess(),
                HealthAssessment::BrokenCheck,
                "should classify as broken: {}",
                output
            );
        }
    }

    #[test]
    fn test_health_marker_matching_is_case_insensitive() {
        assert_eq!(
            health(2, "CURL: NOT FOUND").assess(),
            HealthAssessment::BrokenCheck
        );
    }

    #[test]
    fn test_health_enormous_streak_is_a_broken_check() {
        // 162,460 consecutive failures since April is a configuration defect,
        // not an outage anyone is about to fix by being paged
        assert_eq!(
            health(162_460, "Connection refused").assess(),
            HealthAssessment::BrokenCheck
        );
    }

    #[test]
    fn test_health_streak_boundary() {
        let threshold = ContainerHealthDetail::BROKEN_CHECK_STREAK;
        assert_eq!(
            health(threshold - 1, "Connection refused").assess(),
            HealthAssessment::FailingNow
        );
        assert_eq!(
            health(threshold, "Connection refused").assess(),
            HealthAssessment::BrokenCheck
        );
    }

    #[test]
    fn test_health_genuine_failure_is_not_masked_by_a_long_streak_rule() {
        // A real outage must stay actionable for as long as it plausibly is one
        assert_eq!(
            health(20, "database is starting up").assess(),
            HealthAssessment::FailingNow
        );
    }

    #[test]
    fn test_swapoff_would_exceed_limit() {
        // The real incident: SonarQube capped at 2000M, holding ~1.1G in swap
        let c = container_with(true, 2_000_000_000, 1_400_000_000, Some(1_100_000_000));
        assert!(c.would_exceed_limit_on_swapoff());
    }

    #[test]
    fn test_swapoff_within_limit() {
        let c = container_with(true, 2_000_000_000, 500_000_000, Some(200_000_000));
        assert!(!c.would_exceed_limit_on_swapoff());
    }

    #[test]
    fn test_swapoff_without_limit_cannot_exceed() {
        // Nothing to exceed: the container is unbounded, so swapoff cannot
        // trigger a cgroup kill for it
        let c = container_with(false, 16_000_000_000, 8_000_000_000, Some(4_000_000_000));
        assert!(!c.would_exceed_limit_on_swapoff());
    }

    #[test]
    fn test_swapoff_unknown_swap_is_not_a_prediction() {
        let c = container_with(true, 2_000_000_000, 1_900_000_000, None);
        assert!(
            !c.would_exceed_limit_on_swapoff(),
            "unknown swap must not be reported as a predicted OOM"
        );
    }

    #[test]
    fn test_systemd_failed_unit_serialization() {
        let unit = SystemdFailedUnit {
            name: "certbot.service".to_string(),
            description: "Certbot".to_string(),
            since: Some(Utc::now()),
        };

        let json = serde_json::to_string(&unit).unwrap();
        let deserialized: SystemdFailedUnit = serde_json::from_str(&json).unwrap();
        assert_eq!(unit.name, deserialized.name);
        assert_eq!(unit.description, deserialized.description);
        assert!(deserialized.since.is_some());
    }

    #[test]
    fn test_systemd_failed_unit_serialization_without_since() {
        let unit = SystemdFailedUnit {
            name: "networking.service".to_string(),
            description: "Raise network interfaces".to_string(),
            since: None,
        };

        let json = serde_json::to_string(&unit).unwrap();
        let deserialized: SystemdFailedUnit = serde_json::from_str(&json).unwrap();
        assert_eq!(unit.name, deserialized.name);
        assert!(deserialized.since.is_none());
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
