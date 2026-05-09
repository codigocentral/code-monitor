//! Client implementation for connecting to monitoring servers
//!
//! This module handles the gRPC communication with monitoring servers

use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use shared::proto::monitoring::{
    monitor_service_client::MonitorServiceClient, ProcessesRequest, SystemUpdate, UpdatesRequest,
};
use shared::types::{
    ConnectionStateCount, ContainerInfo, DiskInfo, MariaDBClusterInfo, MariaDBProcessInfo,
    MariaDBSchemaInfo, NetworkInfo, PostgresClusterInfo, PostgresDatabaseInfo, ProcessInfo,
    ServiceInfo, ServiceStatus, SystemInfo, SystemdUnitInfo, TopQuery,
};
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;
use tonic::Streaming;
use tracing::info;

fn timestamp_to_datetime(ts: prost_types::Timestamp) -> DateTime<Utc> {
    Utc.timestamp_opt(ts.seconds, ts.nanos as u32)
        .single()
        .unwrap_or_else(Utc::now)
}

pub struct MonitorClient {
    channel: Channel,
    #[allow(dead_code)]
    update_interval: u64,
    auto_reconnect: bool,
    /// Access token for authentication
    access_token: Option<String>,
}

impl MonitorClient {
    pub async fn connect(
        server_address: &str,
        update_interval: u64,
        auto_reconnect: bool,
    ) -> Result<Self> {
        Self::connect_with_token(server_address, update_interval, auto_reconnect, None, None).await
    }

    pub async fn connect_with_token(
        server_address: &str,
        update_interval: u64,
        auto_reconnect: bool,
        access_token: Option<&str>,
        tls_config: Option<&shared::types::ClientTlsConfig>,
    ) -> Result<Self> {
        let endpoint =
            Channel::from_shared(server_address.to_string()).context("Invalid server address")?;

        let (endpoint, custom_connector) = if let Some(tls) = tls_config {
            if crate::tls::is_tls_valid(Some(tls)) {
                let setup = crate::tls::configure_tls(endpoint, tls)?;
                (setup.endpoint, setup.connector)
            } else {
                (endpoint, None)
            }
        } else {
            (endpoint, None)
        };

        let channel = if let Some(connector) = custom_connector {
            endpoint
                .connect_with_connector(connector)
                .await
                .context("Failed to connect to server")?
        } else {
            endpoint
                .connect()
                .await
                .context("Failed to connect to server")?
        };

        info!("Connected to monitoring server at {}", server_address);

        Ok(Self {
            channel,
            update_interval,
            auto_reconnect,
            access_token: access_token.map(String::from),
        })
    }

    /// Set the access token for this client
    #[allow(dead_code)]
    pub fn set_access_token(&mut self, token: Option<String>) {
        self.access_token = token;
    }

    /// Create a request with authentication metadata
    fn create_request<T>(&self, payload: T) -> tonic::Request<T> {
        Self::create_request_with_token(payload, self.access_token.as_deref())
    }

    /// Create a request with optional authentication token
    fn create_request_with_token<T>(
        payload: T,
        access_token: Option<&str>,
    ) -> tonic::Request<T> {
        let mut request = tonic::Request::new(payload);

        if let Some(token) = access_token {
            if let Ok(value) = MetadataValue::try_from(token) {
                request.metadata_mut().insert("x-access-token", value);
            }
        }

        request
    }

    pub fn is_auto_reconnect(&self) -> bool {
        self.auto_reconnect
    }

    pub async fn get_system_info(&mut self) -> Result<SystemInfo> {
        let mut client = MonitorServiceClient::new(self.channel.clone());

        let request = self.create_request(());
        let response = client
            .get_system_info(request)
            .await
            .context("Failed to get system info")?;

        let resp = response.into_inner();

        let disk_info: Vec<DiskInfo> = resp
            .disk_info
            .into_iter()
            .map(|disk| DiskInfo {
                device: disk.device,
                mount_point: disk.mount_point,
                filesystem_type: disk.filesystem_type,
                total_bytes: disk.total_bytes,
                used_bytes: disk.used_bytes,
                available_bytes: disk.available_bytes,
                usage_percent: disk.usage_percent,
            })
            .collect();

        let system_info = SystemInfo {
            hostname: resp.hostname,
            os: resp.os,
            kernel_version: resp.kernel_version,
            uptime_seconds: resp.uptime_seconds,
            cpu_count: resp.cpu_count,
            cpu_usage_percent: resp.cpu_usage_percent,
            memory_total_bytes: resp.memory_total_bytes,
            memory_used_bytes: resp.memory_used_bytes,
            memory_available_bytes: resp.memory_available_bytes,
            disk_info,
            timestamp: resp
                .timestamp
                .map(timestamp_to_datetime)
                .unwrap_or_else(Utc::now),
        };

        Ok(system_info)
    }

    pub async fn get_processes(
        &mut self,
        limit: u32,
        filter: Option<String>,
    ) -> Result<Vec<ProcessInfo>> {
        let mut client = MonitorServiceClient::new(self.channel.clone());

        let request = self.create_request(ProcessesRequest {
            limit,
            filter: filter.unwrap_or_default(),
        });

        let response = client
            .get_processes(request)
            .await
            .context("Failed to get processes")?;

        let resp = response.into_inner();

        let processes: Vec<ProcessInfo> = resp
            .processes
            .into_iter()
            .map(|proc| ProcessInfo {
                pid: proc.pid,
                name: proc.name,
                user: proc.user,
                cpu_usage_percent: proc.cpu_usage_percent,
                memory_usage_bytes: proc.memory_usage_bytes,
                command_line: proc.command_line,
                start_time: proc
                    .start_time
                    .map(timestamp_to_datetime)
                    .unwrap_or_else(Utc::now),
                status: proc.status,
            })
            .collect();

        Ok(processes)
    }

    pub async fn get_services(&mut self) -> Result<Vec<ServiceInfo>> {
        let mut client = MonitorServiceClient::new(self.channel.clone());

        let request = self.create_request(());
        let response = client
            .get_services(request)
            .await
            .context("Failed to get services")?;

        let resp = response.into_inner();

        let services: Vec<ServiceInfo> = resp
            .services
            .into_iter()
            .map(|service| ServiceInfo {
                name: service.name,
                status: match service.status.as_str() {
                    "Running" => ServiceStatus::Running,
                    "Stopped" => ServiceStatus::Stopped,
                    "Failed" => ServiceStatus::Failed,
                    _ => ServiceStatus::Unknown,
                },
                pid: if service.pid > 0 {
                    Some(service.pid)
                } else {
                    None
                },
                cpu_usage_percent: service.cpu_usage_percent,
                memory_usage_bytes: service.memory_usage_bytes,
                user: service.user,
                uptime_seconds: if service.uptime_seconds > 0 {
                    Some(service.uptime_seconds)
                } else {
                    None
                },
            })
            .collect();

        Ok(services)
    }

    pub async fn get_network_info(&mut self) -> Result<Vec<NetworkInfo>> {
        let mut client = MonitorServiceClient::new(self.channel.clone());

        let request = self.create_request(());
        let response = client
            .get_network_info(request)
            .await
            .context("Failed to get network info")?;

        let resp = response.into_inner();

        let networks: Vec<NetworkInfo> = resp
            .interfaces
            .into_iter()
            .map(|net| NetworkInfo {
                interface: net.interface,
                ip_address: net.ip_address,
                mac_address: net.mac_address,
                is_up: net.is_up,
                bytes_sent: net.bytes_sent,
                bytes_received: net.bytes_received,
                packets_sent: net.packets_sent,
                packets_received: net.packets_received,
            })
            .collect();

        Ok(networks)
    }

    pub async fn get_containers(&mut self, filter: Option<String>) -> Result<Vec<ContainerInfo>> {
        let mut client = MonitorServiceClient::new(self.channel.clone());

        let request = self.create_request(shared::proto::monitoring::ContainersRequest {
            filter: filter.unwrap_or_default(),
        });
        let response = client
            .get_containers(request)
            .await
            .context("Failed to get containers")?;

        let resp = response.into_inner();

        let containers: Vec<ContainerInfo> = resp
            .containers
            .into_iter()
            .map(|c| ContainerInfo {
                id: c.id,
                name: c.name,
                image: c.image,
                status: c.status,
                state: c.state,
                health: c.health,
                cpu_percent: c.cpu_percent,
                memory_usage_bytes: c.memory_usage_bytes,
                memory_limit_bytes: c.memory_limit_bytes,
                memory_percent: c.memory_percent,
                restart_count: c.restart_count,
                network_rx_bytes: c.network_rx_bytes,
                network_tx_bytes: c.network_tx_bytes,
                networks: c.networks,
            })
            .collect();

        Ok(containers)
    }

    pub async fn get_postgres_info(&mut self) -> Result<Vec<PostgresClusterInfo>> {
        let mut client = MonitorServiceClient::new(self.channel.clone());

        let request = self.create_request(());
        let response = client
            .get_postgres_info(request)
            .await
            .context("Failed to get postgres info")?;

        let resp = response.into_inner();

        let clusters: Vec<PostgresClusterInfo> = resp
            .clusters
            .into_iter()
            .map(|c| PostgresClusterInfo {
                name: c.name,
                host: c.host,
                port: c.port as u16,
                databases: c
                    .databases
                    .into_iter()
                    .map(|d| PostgresDatabaseInfo {
                        name: d.name,
                        size_bytes: d.size_bytes,
                        num_backends: d.num_backends,
                        cache_hit_ratio: d.cache_hit_ratio,
                    })
                    .collect(),
                connections_total: c.connections_total,
                connections_by_state: c
                    .connections_by_state
                    .into_iter()
                    .map(|s| ConnectionStateCount {
                        state: s.state,
                        count: s.count,
                    })
                    .collect(),
                cache_hit_ratio: c.cache_hit_ratio,
                top_queries: c
                    .top_queries
                    .into_iter()
                    .map(|q| TopQuery {
                        query: q.query,
                        calls: q.calls,
                        total_exec_time_ms: q.total_exec_time_ms,
                        mean_exec_time_ms: q.mean_exec_time_ms,
                    })
                    .collect(),
                timestamp: c
                    .timestamp
                    .map(timestamp_to_datetime)
                    .unwrap_or_else(Utc::now),
            })
            .collect();

        Ok(clusters)
    }

    pub async fn get_mariadb_info(&mut self) -> Result<Vec<MariaDBClusterInfo>> {
        let mut client = MonitorServiceClient::new(self.channel.clone());

        let request = self.create_request(());
        let response = client
            .get_maria_db_info(request)
            .await
            .context("Failed to get MariaDB info")?;

        let resp = response.into_inner();

        let clusters: Vec<MariaDBClusterInfo> = resp
            .clusters
            .into_iter()
            .map(|c| MariaDBClusterInfo {
                name: c.name,
                host: c.host,
                port: c.port as u16,
                schemas: c
                    .schemas
                    .into_iter()
                    .map(|s| MariaDBSchemaInfo {
                        name: s.name,
                        size_bytes: s.size_bytes,
                        table_count: s.table_count,
                    })
                    .collect(),
                connections_active: c.connections_active,
                connections_total: c.connections_total,
                innodb_status: if c.innodb_status.is_empty() {
                    None
                } else {
                    Some(c.innodb_status)
                },
                processes: c
                    .processes
                    .into_iter()
                    .map(|p| MariaDBProcessInfo {
                        id: p.id,
                        user: p.user,
                        host: p.host,
                        db: if p.db.is_empty() { None } else { Some(p.db) },
                        command: p.command,
                        time_seconds: p.time_seconds,
                        state: p.state,
                        info: if p.info.is_empty() {
                            None
                        } else {
                            Some(p.info)
                        },
                    })
                    .collect(),
                timestamp: c
                    .timestamp
                    .map(timestamp_to_datetime)
                    .unwrap_or_else(Utc::now),
            })
            .collect();

        Ok(clusters)
    }

    pub async fn get_systemd_info(&mut self) -> Result<Vec<SystemdUnitInfo>> {
        let mut client = MonitorServiceClient::new(self.channel.clone());

        let request = self.create_request(());
        let response = client
            .get_systemd_info(request)
            .await
            .context("Failed to get systemd info")?;

        let resp = response.into_inner();

        let units: Vec<SystemdUnitInfo> = resp
            .units
            .into_iter()
            .map(|u| SystemdUnitInfo {
                name: u.name,
                status: u.status,
                is_active: u.is_active,
                pid: if u.pid > 0 { Some(u.pid) } else { None },
                memory_current_bytes: u.memory_current_bytes,
                started_at: u.started_at.map(timestamp_to_datetime),
            })
            .collect();

        Ok(units)
    }

    #[allow(dead_code)]
    pub async fn stream_system_updates(&mut self) -> Result<Streaming<SystemUpdate>> {
        let mut client = MonitorServiceClient::new(self.channel.clone());

        let request = self.create_request(UpdatesRequest {
            update_interval_seconds: self.update_interval,
        });

        let response = client
            .stream_system_updates(request)
            .await
            .context("Failed to start system updates stream")?;

        Ok(response.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_request_without_token() {
        let request = MonitorClient::create_request_with_token((), None);
        assert!(!request.metadata().contains_key("x-access-token"));
    }

    #[test]
    fn test_create_request_with_token() {
        let request = MonitorClient::create_request_with_token((), Some("my-secret-token"));
        let token = request
            .metadata()
            .get("x-access-token")
            .expect("Token should be present");
        assert_eq!(token.to_str().unwrap(), "my-secret-token");
    }

    #[test]
    fn test_create_request_with_special_chars_token() {
        let request = MonitorClient::create_request_with_token((), Some("token-with-chars_123"));
        let token = request
            .metadata()
            .get("x-access-token")
            .expect("Token should be present");
        assert_eq!(token.to_str().unwrap(), "token-with-chars_123");
    }

    #[test]
    fn test_create_request_with_invalid_token_chars() {
        // Characters not allowed in gRPC metadata should be silently ignored
        let request = MonitorClient::create_request_with_token((), Some("token\nwith\nnewlines"));
        assert!(!request.metadata().contains_key("x-access-token"));
    }

    #[test]
    fn test_timestamp_to_datetime_valid() {
        let ts = prost_types::Timestamp {
            seconds: 1_700_000_000,
            nanos: 500_000_000,
        };
        let dt = timestamp_to_datetime(ts);
        assert_eq!(dt.timestamp(), 1_700_000_000);
        assert_eq!(dt.timestamp_subsec_nanos(), 500_000_000);
    }

    #[test]
    fn test_timestamp_to_datetime_zero() {
        let ts = prost_types::Timestamp {
            seconds: 0,
            nanos: 0,
        };
        let dt = timestamp_to_datetime(ts);
        assert_eq!(dt.timestamp(), 0);
    }

    #[test]
    fn test_timestamp_to_datetime_negative_seconds() {
        let ts = prost_types::Timestamp {
            seconds: -100,
            nanos: 0,
        };
        let dt = timestamp_to_datetime(ts);
        assert_eq!(dt.timestamp(), -100);
    }

    #[test]
    fn test_timestamp_to_datetime_invalid_nanos() {
        // prost_types::Timestamp normalizes nanos, but we test the fallback path
        let ts = prost_types::Timestamp {
            seconds: 1_700_000_000,
            nanos: 2_000_000_000,
        };
        let dt = timestamp_to_datetime(ts);
        // Utc.timestamp_opt with out-of-range nanos falls back to Utc::now
        // We just verify it doesn't panic
        let _ = dt;
    }
}
