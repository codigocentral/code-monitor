//! gRPC service implementation
//!
//! This module implements the MonitorService gRPC service

use anyhow::Result;
use chrono::{DateTime, Utc};
use shared::proto::monitoring::{
    monitor_service_server::MonitorService, system_update::UpdateType,
    ConnectionStateCount as ProtoConnectionStateCount, ContainerInfo as ProtoContainerInfo,
    ContainersRequest, ContainersResponse, DiskInfo as ProtoDiskInfo,
    MariaDbClusterInfo as ProtoMariaDBClusterInfo, MariaDbInfoResponse,
    MariaDbProcessInfo as ProtoMariaDBProcessInfo, MariaDbSchemaInfo as ProtoMariaDBSchemaInfo,
    NetworkInfo as ProtoNetworkInfo, NetworkInfoResponse,
    PostgresClusterInfo as ProtoPostgresClusterInfo,
    PostgresDatabaseInfo as ProtoPostgresDatabaseInfo, PostgresInfoResponse,
    ProcessInfo as ProtoProcessInfo, ProcessesRequest, ProcessesResponse,
    ServiceInfo as ProtoServiceInfo, ServicesResponse, SystemInfoResponse, SystemUpdate,
    SystemdInfoResponse, SystemdUnitInfo as ProtoSystemdUnitInfo, TopQuery as ProtoTopQuery,
    UpdatesRequest,
};
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::monitor::SystemMonitor;

fn datetime_to_timestamp(dt: DateTime<Utc>) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    }
}

fn now_timestamp() -> prost_types::Timestamp {
    datetime_to_timestamp(Utc::now())
}

pub struct MonitorServiceImpl {
    monitor: Arc<SystemMonitor>,
    config: Arc<RwLock<Config>>,
}

impl MonitorServiceImpl {
    #[allow(dead_code)]
    pub async fn new(monitor: SystemMonitor) -> Result<Self> {
        let service = Self {
            monitor: Arc::new(monitor),
            config: Arc::new(RwLock::new(Config::default())),
        };

        // Start background monitoring
        service.monitor.start_background_monitoring();

        Ok(service)
    }

    #[allow(dead_code)]
    pub async fn new_with_config(monitor: SystemMonitor, config: Config) -> Result<Self> {
        let service = Self {
            monitor: Arc::new(monitor),
            config: Arc::new(RwLock::new(config)),
        };

        // Start background monitoring
        service.monitor.start_background_monitoring();

        Ok(service)
    }

    /// Create a new service with shared monitor (for health check integration)
    pub fn from_arc(monitor: Arc<SystemMonitor>, config: Config) -> Result<Self> {
        let service = Self {
            monitor,
            config: Arc::new(RwLock::new(config)),
        };

        // Note: background monitoring should already be started

        Ok(service)
    }

    #[allow(dead_code)]
    pub fn shutdown(&self) {
        self.monitor.stop_background_monitoring();
        info!("Monitor service shutdown");
    }

    /// Validate access token from request metadata
    #[allow(clippy::result_large_err)]
    fn validate_request<T>(&self, request: &Request<T>) -> Result<(), Status> {
        let config = self
            .config
            .read()
            .map_err(|_| Status::internal("Config lock error"))?;

        // If auth is disabled, allow all
        if !config.enable_authentication {
            return Ok(());
        }

        // Check for access token in metadata
        if let Some(token) = request.metadata().get("x-access-token") {
            let token_str = token
                .to_str()
                .map_err(|_| Status::unauthenticated("Invalid token format"))?;
            if config.validate_access_token(token_str) {
                return Ok(());
            }
        }

        // Also check authorization header
        if let Some(auth) = request.metadata().get("authorization") {
            let auth_str = auth
                .to_str()
                .map_err(|_| Status::unauthenticated("Invalid auth format"))?;
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                if config.validate_access_token(token) {
                    return Ok(());
                }
            }
        }

        Err(Status::unauthenticated("Invalid or missing access token"))
    }
}

#[tonic::async_trait]
impl MonitorService for MonitorServiceImpl {
    async fn get_system_info(
        &self,
        request: Request<()>,
    ) -> Result<Response<SystemInfoResponse>, Status> {
        // Validate authentication
        self.validate_request(&request)?;

        info!("Handling GetSystemInfo request");

        let system_info = self.monitor.get_system_info().map_err(|e| {
            error!("Failed to get system info: {}", e);
            Status::internal("Failed to get system information")
        })?;

        let disk_info: Vec<ProtoDiskInfo> = system_info
            .disk_info
            .iter()
            .map(|disk| ProtoDiskInfo {
                device: disk.device.clone(),
                mount_point: disk.mount_point.clone(),
                filesystem_type: disk.filesystem_type.clone(),
                total_bytes: disk.total_bytes,
                used_bytes: disk.used_bytes,
                available_bytes: disk.available_bytes,
                usage_percent: disk.usage_percent,
            })
            .collect();

        let response = SystemInfoResponse {
            hostname: system_info.hostname,
            os: system_info.os,
            kernel_version: system_info.kernel_version,
            uptime_seconds: system_info.uptime_seconds,
            cpu_count: system_info.cpu_count,
            cpu_usage_percent: system_info.cpu_usage_percent,
            memory_total_bytes: system_info.memory_total_bytes,
            memory_used_bytes: system_info.memory_used_bytes,
            memory_available_bytes: system_info.memory_available_bytes,
            disk_info,
            timestamp: Some(datetime_to_timestamp(system_info.timestamp)),
        };

        Ok(Response::new(response))
    }

    async fn get_processes(
        &self,
        request: Request<ProcessesRequest>,
    ) -> Result<Response<ProcessesResponse>, Status> {
        // Validate authentication
        self.validate_request(&request)?;

        info!("Handling GetProcesses request");

        let req = request.into_inner();
        let limit = req.limit;
        let filter = if req.filter.is_empty() {
            None
        } else {
            Some(req.filter)
        };

        let processes = self.monitor.get_processes(limit, filter).map_err(|e| {
            error!("Failed to get processes: {}", e);
            Status::internal("Failed to get processes")
        })?;

        let process_infos: Vec<ProtoProcessInfo> = processes
            .iter()
            .map(|proc| ProtoProcessInfo {
                pid: proc.pid,
                name: proc.name.clone(),
                user: proc.user.clone(),
                cpu_usage_percent: proc.cpu_usage_percent,
                memory_usage_bytes: proc.memory_usage_bytes,
                command_line: proc.command_line.clone(),
                start_time: Some(datetime_to_timestamp(proc.start_time)),
                status: proc.status.clone(),
            })
            .collect();

        let response = ProcessesResponse {
            processes: process_infos,
            timestamp: Some(now_timestamp()),
        };

        Ok(Response::new(response))
    }

    async fn get_services(
        &self,
        request: Request<()>,
    ) -> Result<Response<ServicesResponse>, Status> {
        // Validate authentication
        self.validate_request(&request)?;

        info!("Handling GetServices request");

        let services = self.monitor.get_services().map_err(|e| {
            error!("Failed to get services: {}", e);
            Status::internal("Failed to get services")
        })?;

        let service_infos: Vec<ProtoServiceInfo> = services
            .iter()
            .map(|service| ProtoServiceInfo {
                name: service.name.clone(),
                status: format!("{:?}", service.status),
                pid: service.pid.unwrap_or(0),
                cpu_usage_percent: service.cpu_usage_percent,
                memory_usage_bytes: service.memory_usage_bytes,
                user: service.user.clone(),
                uptime_seconds: service.uptime_seconds.unwrap_or(0),
            })
            .collect();

        let response = ServicesResponse {
            services: service_infos,
            timestamp: Some(now_timestamp()),
        };

        Ok(Response::new(response))
    }

    async fn get_network_info(
        &self,
        request: Request<()>,
    ) -> Result<Response<NetworkInfoResponse>, Status> {
        // Validate authentication
        self.validate_request(&request)?;

        info!("Handling GetNetworkInfo request");

        let networks = self.monitor.get_network_info().map_err(|e| {
            error!("Failed to get network info: {}", e);
            Status::internal("Failed to get network information")
        })?;

        let network_infos: Vec<ProtoNetworkInfo> = networks
            .iter()
            .map(|network| ProtoNetworkInfo {
                interface: network.interface.clone(),
                ip_address: network.ip_address.clone(),
                mac_address: network.mac_address.clone(),
                is_up: network.is_up,
                bytes_sent: network.bytes_sent,
                bytes_received: network.bytes_received,
                packets_sent: network.packets_sent,
                packets_received: network.packets_received,
            })
            .collect();

        let response = NetworkInfoResponse {
            interfaces: network_infos,
            timestamp: Some(now_timestamp()),
        };

        Ok(Response::new(response))
    }

    type StreamSystemUpdatesStream =
        Pin<Box<dyn Stream<Item = Result<SystemUpdate, Status>> + Send>>;

    async fn stream_system_updates(
        &self,
        request: Request<UpdatesRequest>,
    ) -> Result<Response<Self::StreamSystemUpdatesStream>, Status> {
        // Validate authentication
        self.validate_request(&request)?;

        info!("Handling StreamSystemUpdates request");

        let req = request.into_inner();
        let update_interval = std::cmp::max(req.update_interval_seconds, 1);

        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let monitor = Arc::clone(&self.monitor);

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(update_interval));

            loop {
                interval.tick().await;

                match monitor.get_system_info() {
                    Ok(system_info) => {
                        let disk_info: Vec<ProtoDiskInfo> = system_info
                            .disk_info
                            .iter()
                            .map(|disk| ProtoDiskInfo {
                                device: disk.device.clone(),
                                mount_point: disk.mount_point.clone(),
                                filesystem_type: disk.filesystem_type.clone(),
                                total_bytes: disk.total_bytes,
                                used_bytes: disk.used_bytes,
                                available_bytes: disk.available_bytes,
                                usage_percent: disk.usage_percent,
                            })
                            .collect();

                        let update = SystemUpdate {
                            update_type: Some(UpdateType::SystemInfo(SystemInfoResponse {
                                hostname: system_info.hostname,
                                os: system_info.os,
                                kernel_version: system_info.kernel_version,
                                uptime_seconds: system_info.uptime_seconds,
                                cpu_count: system_info.cpu_count,
                                cpu_usage_percent: system_info.cpu_usage_percent,
                                memory_total_bytes: system_info.memory_total_bytes,
                                memory_used_bytes: system_info.memory_used_bytes,
                                memory_available_bytes: system_info.memory_available_bytes,
                                disk_info,
                                timestamp: Some(datetime_to_timestamp(system_info.timestamp)),
                            })),
                            timestamp: Some(now_timestamp()),
                        };

                        if tx.send(Ok(update)).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to get system info for stream: {}", e);
                        let update = SystemUpdate {
                            update_type: None,
                            timestamp: Some(now_timestamp()),
                        };

                        if tx.send(Ok(update)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Response::new(
            Box::pin(stream) as Self::StreamSystemUpdatesStream
        ))
    }

    async fn get_containers(
        &self,
        request: Request<ContainersRequest>,
    ) -> Result<Response<ContainersResponse>, Status> {
        // Validate authentication
        self.validate_request(&request)?;

        info!("Handling GetContainers request");

        let containers = self.monitor.get_containers().await.map_err(|e| {
            error!("Failed to get containers: {}", e);
            Status::internal("Failed to get containers")
        })?;

        let container_infos: Vec<ProtoContainerInfo> = containers
            .iter()
            .map(|c| ProtoContainerInfo {
                id: c.id.clone(),
                name: c.name.clone(),
                image: c.image.clone(),
                status: c.status.clone(),
                state: c.state.clone(),
                health: c.health.clone(),
                cpu_percent: c.cpu_percent,
                memory_usage_bytes: c.memory_usage_bytes,
                memory_limit_bytes: c.memory_limit_bytes,
                memory_percent: c.memory_percent,
                restart_count: c.restart_count,
                network_rx_bytes: c.network_rx_bytes,
                network_tx_bytes: c.network_tx_bytes,
                networks: c.networks.clone(),
            })
            .collect();

        let response = ContainersResponse {
            containers: container_infos,
            timestamp: Some(now_timestamp()),
        };

        Ok(Response::new(response))
    }

    async fn get_postgres_info(
        &self,
        request: Request<()>,
    ) -> Result<Response<PostgresInfoResponse>, Status> {
        self.validate_request(&request)?;

        info!("Handling GetPostgresInfo request");

        let clusters = self.monitor.get_postgres_clusters().await.map_err(|e| {
            error!("Failed to get postgres info: {}", e);
            Status::internal("Failed to get postgres info")
        })?;

        let cluster_infos: Vec<ProtoPostgresClusterInfo> = clusters
            .iter()
            .map(|c| ProtoPostgresClusterInfo {
                name: c.name.clone(),
                host: c.host.clone(),
                port: c.port as u32,
                databases: c
                    .databases
                    .iter()
                    .map(|d| ProtoPostgresDatabaseInfo {
                        name: d.name.clone(),
                        size_bytes: d.size_bytes,
                        num_backends: d.num_backends,
                        cache_hit_ratio: d.cache_hit_ratio,
                    })
                    .collect(),
                connections_total: c.connections_total,
                connections_by_state: c
                    .connections_by_state
                    .iter()
                    .map(|s| ProtoConnectionStateCount {
                        state: s.state.clone(),
                        count: s.count,
                    })
                    .collect(),
                cache_hit_ratio: c.cache_hit_ratio,
                top_queries: c
                    .top_queries
                    .iter()
                    .map(|q| ProtoTopQuery {
                        query: q.query.clone(),
                        calls: q.calls,
                        total_exec_time_ms: q.total_exec_time_ms,
                        mean_exec_time_ms: q.mean_exec_time_ms,
                    })
                    .collect(),
                timestamp: Some(datetime_to_timestamp(c.timestamp)),
            })
            .collect();

        let response = PostgresInfoResponse {
            clusters: cluster_infos,
            timestamp: Some(now_timestamp()),
        };

        Ok(Response::new(response))
    }

    async fn get_maria_db_info(
        &self,
        request: Request<()>,
    ) -> Result<Response<MariaDbInfoResponse>, Status> {
        self.validate_request(&request)?;

        info!("Handling GetMariaDBInfo request");

        let clusters = self.monitor.get_mariadb_clusters().await.map_err(|e| {
            error!("Failed to get MariaDB info: {}", e);
            Status::internal("Failed to get MariaDB info")
        })?;

        let cluster_infos: Vec<ProtoMariaDBClusterInfo> = clusters
            .iter()
            .map(|c| ProtoMariaDBClusterInfo {
                name: c.name.clone(),
                host: c.host.clone(),
                port: c.port as u32,
                schemas: c
                    .schemas
                    .iter()
                    .map(|s| ProtoMariaDBSchemaInfo {
                        name: s.name.clone(),
                        size_bytes: s.size_bytes,
                        table_count: s.table_count,
                    })
                    .collect(),
                connections_active: c.connections_active,
                connections_total: c.connections_total,
                innodb_status: c.innodb_status.clone().unwrap_or_default(),
                processes: c
                    .processes
                    .iter()
                    .map(|p| ProtoMariaDBProcessInfo {
                        id: p.id,
                        user: p.user.clone(),
                        host: p.host.clone(),
                        db: p.db.clone().unwrap_or_default(),
                        command: p.command.clone(),
                        time_seconds: p.time_seconds,
                        state: p.state.clone(),
                        info: p.info.clone().unwrap_or_default(),
                    })
                    .collect(),
                timestamp: Some(datetime_to_timestamp(c.timestamp)),
            })
            .collect();

        let response = MariaDbInfoResponse {
            clusters: cluster_infos,
            timestamp: Some(now_timestamp()),
        };

        Ok(Response::new(response))
    }

    async fn get_systemd_info(
        &self,
        request: Request<()>,
    ) -> Result<Response<SystemdInfoResponse>, Status> {
        self.validate_request(&request)?;

        info!("Handling GetSystemdInfo request");

        let units = self.monitor.get_systemd_units().await.map_err(|e| {
            error!("Failed to get systemd info: {}", e);
            Status::internal("Failed to get systemd info")
        })?;

        let unit_infos: Vec<ProtoSystemdUnitInfo> = units
            .iter()
            .map(|u| ProtoSystemdUnitInfo {
                name: u.name.clone(),
                status: u.status.clone(),
                is_active: u.is_active,
                pid: u.pid.unwrap_or(0),
                memory_current_bytes: u.memory_current_bytes,
                started_at: u.started_at.map(datetime_to_timestamp),
            })
            .collect();

        let response = SystemdInfoResponse {
            units: unit_infos,
            timestamp: Some(now_timestamp()),
        };

        Ok(Response::new(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::metadata::MetadataValue;
    use tonic::transport::Server;

    async fn create_test_service(auth_enabled: bool) -> MonitorServiceImpl {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");
        let mut config = Config::default();
        config.enable_authentication = auth_enabled;
        config.access_token = "test-secret-token".to_string();
        MonitorServiceImpl::from_arc(Arc::new(monitor), config)
            .expect("Failed to create service")
    }

    #[tokio::test]
    async fn test_validate_request_disabled_auth() {
        let service = create_test_service(false).await;
        let request = Request::new(());
        let result = service.validate_request(&request);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_request_missing_token() {
        let service = create_test_service(true).await;
        let request = Request::new(());
        let result = service.validate_request(&request);
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn test_validate_request_valid_x_access_token() {
        let service = create_test_service(true).await;
        let mut request = Request::new(());
        request
            .metadata_mut()
            .insert("x-access-token", MetadataValue::from_static("test-secret-token"));
        let result = service.validate_request(&request);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_request_valid_bearer_token() {
        let service = create_test_service(true).await;
        let mut request = Request::new(());
        request
            .metadata_mut()
            .insert("authorization", MetadataValue::from_static("Bearer test-secret-token"));
        let result = service.validate_request(&request);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_request_invalid_token() {
        let service = create_test_service(true).await;
        let mut request = Request::new(());
        request
            .metadata_mut()
            .insert("x-access-token", MetadataValue::from_static("wrong-token"));
        let result = service.validate_request(&request);
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn test_validate_request_invalid_bearer_format() {
        let service = create_test_service(true).await;
        let mut request = Request::new(());
        request
            .metadata_mut()
            .insert("authorization", MetadataValue::from_static("Basic dXNlcjpwYXNz"));
        let result = service.validate_request(&request);
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn test_datetime_to_timestamp() {
        let now = Utc::now();
        let ts = datetime_to_timestamp(now);
        assert_eq!(ts.seconds, now.timestamp());
    }

    /// Integration test: spin up a real gRPC server and connect via tonic client
    #[tokio::test]
    async fn test_integration_get_system_info() {
        use shared::proto::monitoring::{
            monitor_service_client::MonitorServiceClient,
            monitor_service_server::MonitorServiceServer,
        };
        use tokio::time::timeout;

        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");
        monitor.start_background_monitoring();

        let mut config = Config::default();
        config.enable_authentication = false;
        let service = MonitorServiceImpl::from_arc(Arc::new(monitor), config)
            .expect("Failed to create service");

        // Bind to a random port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind");
        let port = listener.local_addr().unwrap().port();

        let server = Server::builder()
            .add_service(MonitorServiceServer::new(service));

        tokio::spawn(async move {
            let _ = server
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        // Wait for server to start
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let channel = timeout(
            std::time::Duration::from_secs(5),
            tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{}", port))
                .unwrap()
                .connect(),
        )
        .await
        .expect("Connection timeout")
        .expect("Failed to connect");

        let mut client = MonitorServiceClient::new(channel);
        let response = client.get_system_info(Request::new(())).await.unwrap();
        let info = response.into_inner();

        assert!(!info.hostname.is_empty());
        assert!(info.memory_total_bytes > 0);
        assert!(info.cpu_count > 0);
    }

    #[tokio::test]
    async fn test_integration_auth_required() {
        use shared::proto::monitoring::{
            monitor_service_client::MonitorServiceClient,
            monitor_service_server::MonitorServiceServer,
        };

        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");
        monitor.start_background_monitoring();

        let mut config = Config::default();
        config.enable_authentication = true;
        config.access_token = "integration-token".to_string();
        let service = MonitorServiceImpl::from_arc(Arc::new(monitor), config)
            .expect("Failed to create service");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind");
        let port = listener.local_addr().unwrap().port();

        let server = Server::builder()
            .add_service(MonitorServiceServer::new(service));

        tokio::spawn(async move {
            let _ = server
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let channel = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{}", port))
            .unwrap()
            .connect()
            .await
            .expect("Failed to connect");

        let mut client = MonitorServiceClient::new(channel);

        // No token → Unauthenticated
        let result = client.get_system_info(Request::new(())).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);

        // Valid token → OK
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "x-access-token",
            tonic::metadata::MetadataValue::from_static("integration-token"),
        );
        let result = client.get_system_info(request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_integration_get_processes() {
        use shared::proto::monitoring::{
            monitor_service_client::MonitorServiceClient,
            monitor_service_server::MonitorServiceServer,
            ProcessesRequest,
        };

        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");
        monitor.start_background_monitoring();

        let mut config = Config::default();
        config.enable_authentication = false;
        let service = MonitorServiceImpl::from_arc(Arc::new(monitor), config)
            .expect("Failed to create service");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind");
        let port = listener.local_addr().unwrap().port();

        let server = Server::builder()
            .add_service(MonitorServiceServer::new(service));

        tokio::spawn(async move {
            let _ = server
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let channel = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{}", port))
            .unwrap()
            .connect()
            .await
            .expect("Failed to connect");

        let mut client = MonitorServiceClient::new(channel);
        let request = Request::new(ProcessesRequest {
            limit: 5,
            filter: String::new(),
        });
        let response = client.get_processes(request).await.unwrap();
        let processes = response.into_inner().processes;
        assert!(!processes.is_empty());
    }

    #[test]
    fn test_now_timestamp() {
        let before = Utc::now().timestamp();
        let ts = now_timestamp();
        let after = Utc::now().timestamp();
        assert!(ts.seconds >= before);
        assert!(ts.seconds <= after);
        assert!(ts.nanos >= 0);
    }

    #[tokio::test]
    async fn test_integration_get_services() {
        use shared::proto::monitoring::{
            monitor_service_client::MonitorServiceClient,
            monitor_service_server::MonitorServiceServer,
        };

        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");
        monitor.start_background_monitoring();

        let mut config = Config::default();
        config.enable_authentication = false;
        let service = MonitorServiceImpl::from_arc(Arc::new(monitor), config)
            .expect("Failed to create service");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind");
        let port = listener.local_addr().unwrap().port();

        let server = Server::builder()
            .add_service(MonitorServiceServer::new(service));

        tokio::spawn(async move {
            let _ = server
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let channel = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{}", port))
            .unwrap()
            .connect()
            .await
            .expect("Failed to connect");

        let mut client = MonitorServiceClient::new(channel);
        let response = client.get_services(Request::new(())).await.unwrap();
        let services = response.into_inner().services;
        // Services may be empty on some platforms, but the call should succeed
        let _ = services;
    }

    #[tokio::test]
    async fn test_integration_get_network_info() {
        use shared::proto::monitoring::{
            monitor_service_client::MonitorServiceClient,
            monitor_service_server::MonitorServiceServer,
        };

        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");
        monitor.start_background_monitoring();

        let mut config = Config::default();
        config.enable_authentication = false;
        let service = MonitorServiceImpl::from_arc(Arc::new(monitor), config)
            .expect("Failed to create service");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind");
        let port = listener.local_addr().unwrap().port();

        let server = Server::builder()
            .add_service(MonitorServiceServer::new(service));

        tokio::spawn(async move {
            let _ = server
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let channel = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{}", port))
            .unwrap()
            .connect()
            .await
            .expect("Failed to connect");

        let mut client = MonitorServiceClient::new(channel);
        let response = client.get_network_info(Request::new(())).await.unwrap();
        let networks = response.into_inner().interfaces;
        assert!(!networks.is_empty());
    }

    #[tokio::test]
    async fn test_integration_get_containers() {
        use shared::proto::monitoring::{
            monitor_service_client::MonitorServiceClient,
            monitor_service_server::MonitorServiceServer,
            ContainersRequest,
        };

        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");
        monitor.start_background_monitoring();

        let mut config = Config::default();
        config.enable_authentication = false;
        let service = MonitorServiceImpl::from_arc(Arc::new(monitor), config)
            .expect("Failed to create service");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind");
        let port = listener.local_addr().unwrap().port();

        let server = Server::builder()
            .add_service(MonitorServiceServer::new(service));

        tokio::spawn(async move {
            let _ = server
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let channel = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{}", port))
            .unwrap()
            .connect()
            .await
            .expect("Failed to connect");

        let mut client = MonitorServiceClient::new(channel);
        let request = Request::new(ContainersRequest {
            filter: String::new(),
        });
        let response = client.get_containers(request).await.unwrap();
        let containers = response.into_inner().containers;
        // Docker may not be available, but call should succeed
        let _ = containers;
    }

    #[tokio::test]
    async fn test_integration_get_systemd_info() {
        use shared::proto::monitoring::{
            monitor_service_client::MonitorServiceClient,
            monitor_service_server::MonitorServiceServer,
        };

        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");
        monitor.start_background_monitoring();

        let mut config = Config::default();
        config.enable_authentication = false;
        let service = MonitorServiceImpl::from_arc(Arc::new(monitor), config)
            .expect("Failed to create service");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind");
        let port = listener.local_addr().unwrap().port();

        let server = Server::builder()
            .add_service(MonitorServiceServer::new(service));

        tokio::spawn(async move {
            let _ = server
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let channel = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{}", port))
            .unwrap()
            .connect()
            .await
            .expect("Failed to connect");

        let mut client = MonitorServiceClient::new(channel);
        let response = client.get_systemd_info(Request::new(())).await.unwrap();
        let units = response.into_inner().units;
        // systemd may not be available, but call should succeed
        let _ = units;
    }

    #[tokio::test]
    async fn test_integration_get_postgres_info() {
        use shared::proto::monitoring::{
            monitor_service_client::MonitorServiceClient,
            monitor_service_server::MonitorServiceServer,
        };

        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");
        monitor.start_background_monitoring();

        let mut config = Config::default();
        config.enable_authentication = false;
        let service = MonitorServiceImpl::from_arc(Arc::new(monitor), config)
            .expect("Failed to create service");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind");
        let port = listener.local_addr().unwrap().port();

        let server = Server::builder()
            .add_service(MonitorServiceServer::new(service));

        tokio::spawn(async move {
            let _ = server
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let channel = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{}", port))
            .unwrap()
            .connect()
            .await
            .expect("Failed to connect");

        let mut client = MonitorServiceClient::new(channel);
        let response = client.get_postgres_info(Request::new(())).await.unwrap();
        let clusters = response.into_inner().clusters;
        // Postgres may not be configured, but call should succeed
        let _ = clusters;
    }

    #[tokio::test]
    async fn test_integration_get_maria_db_info() {
        use shared::proto::monitoring::{
            monitor_service_client::MonitorServiceClient,
            monitor_service_server::MonitorServiceServer,
        };

        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");
        monitor.start_background_monitoring();

        let mut config = Config::default();
        config.enable_authentication = false;
        let service = MonitorServiceImpl::from_arc(Arc::new(monitor), config)
            .expect("Failed to create service");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind");
        let port = listener.local_addr().unwrap().port();

        let server = Server::builder()
            .add_service(MonitorServiceServer::new(service));

        tokio::spawn(async move {
            let _ = server
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let channel = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{}", port))
            .unwrap()
            .connect()
            .await
            .expect("Failed to connect");

        let mut client = MonitorServiceClient::new(channel);
        let response = client.get_maria_db_info(Request::new(())).await.unwrap();
        let clusters = response.into_inner().clusters;
        // MariaDB may not be configured, but call should succeed
        let _ = clusters;
    }
}
