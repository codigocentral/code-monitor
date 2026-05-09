//! Docker container metrics collector
//!
//! This collector gathers metrics from Docker containers using the
//! Docker Engine API via the bollard crate.

use anyhow::Result;
use bollard::container::ListContainersOptions;
use bollard::container::StatsOptions;
use bollard::Docker;
use futures_util::stream::StreamExt;
use shared::types::ContainerInfo;
use tracing::{error, info, warn};

/// Collector for Docker container metrics
pub struct DockerCollector {
    docker: Option<Docker>,
    _socket_path: String,
}

impl DockerCollector {
    #[cfg(test)]
    fn new_none() -> Self {
        Self {
            docker: None,
            _socket_path: String::new(),
        }
    }

    /// Create a new Docker collector
    pub fn new() -> Self {
        let socket_path =
            std::env::var("DOCKER_SOCKET").unwrap_or_else(|_| "/var/run/docker.sock".to_string());

        let docker = match Docker::connect_with_socket_defaults() {
            Ok(d) => {
                info!("Docker collector connected to {}", socket_path);
                Some(d)
            }
            Err(e) => {
                warn!(
                    "Docker collector failed to connect to {}: {}. Docker metrics will be unavailable.",
                    socket_path, e
                );
                None
            }
        };

        Self {
            docker,
            _socket_path: socket_path,
        }
    }

    /// Create a new Docker collector with a custom socket path
    #[allow(dead_code)]
    pub fn with_socket_path(socket_path: &str) -> Self {
        let docker = match Docker::connect_with_socket(
            socket_path,
            120,
            bollard::API_DEFAULT_VERSION,
        ) {
            Ok(d) => {
                info!("Docker collector connected to {}", socket_path);
                Some(d)
            }
            Err(e) => {
                warn!(
                    "Docker collector failed to connect to {}: {}. Docker metrics will be unavailable.",
                    socket_path, e
                );
                None
            }
        };

        Self {
            docker,
            _socket_path: socket_path.to_string(),
        }
    }

    /// Check if Docker is available
    #[allow(dead_code)]
    pub fn is_available(&self) -> bool {
        self.docker.is_some()
    }

    /// Collect container metrics
    pub async fn collect_containers(&self) -> Result<Vec<ContainerInfo>> {
        let docker = match &self.docker {
            Some(d) => d,
            None => return Ok(Vec::new()),
        };

        // List all containers (running and stopped)
        let options = ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        };

        let containers = match docker.list_containers(Some(options)).await {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to list Docker containers: {}", e);
                return Ok(Vec::new());
            }
        };

        let mut container_infos = Vec::new();

        for container in containers {
            let id = container.id.clone().unwrap_or_default();
            let name = container
                .names
                .as_ref()
                .and_then(|n| n.first())
                .map(|n| n.trim_start_matches('/').to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let image = container.image.clone().unwrap_or_default();
            let state = container.state.clone().unwrap_or_default();
            let status = container.status.clone().unwrap_or_default();

            // Get stats for running containers
            let (cpu_percent, memory_usage, memory_limit, memory_percent, network_rx, network_tx) =
                if state == "running" && !id.is_empty() {
                    match self.get_container_stats(docker, &id).await {
                        Ok(stats) => stats,
                        Err(e) => {
                            warn!("Failed to get stats for container {}: {}", name, e);
                            (0.0, 0, 1, 0.0, 0, 0)
                        }
                    }
                } else {
                    (0.0, 0, 1, 0.0, 0, 0)
                };

            // Get health status from container state
            let health = container
                .status
                .as_ref()
                .map(|s| {
                    if s.contains("healthy") {
                        "healthy"
                    } else if s.contains("unhealthy") {
                        "unhealthy"
                    } else {
                        "none"
                    }
                })
                .unwrap_or("none")
                .to_string();

            // Networks
            let networks: Vec<String> = container
                .network_settings
                .as_ref()
                .and_then(|ns| ns.networks.as_ref())
                .map(|n| n.keys().cloned().collect())
                .unwrap_or_default();

            container_infos.push(ContainerInfo {
                id,
                name,
                image,
                status,
                state,
                health,
                cpu_percent,
                memory_usage_bytes: memory_usage,
                memory_limit_bytes: memory_limit,
                memory_percent,
                restart_count: 0, // Would need container inspect for this
                network_rx_bytes: network_rx,
                network_tx_bytes: network_tx,
                networks,
            });
        }

        Ok(container_infos)
    }

    async fn get_container_stats(
        &self,
        docker: &Docker,
        container_id: &str,
    ) -> Result<(f64, u64, u64, f64, u64, u64)> {
        let stats_options = StatsOptions {
            stream: false,
            one_shot: true,
        };

        let stats = docker
            .stats(container_id, Some(stats_options))
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("No stats received"))??;

        // Calculate CPU percentage
        let cpu_delta = stats.cpu_stats.cpu_usage.total_usage as f64
            - stats.precpu_stats.cpu_usage.total_usage as f64;
        let system_delta = stats.cpu_stats.system_cpu_usage.unwrap_or(0) as f64
            - stats.precpu_stats.system_cpu_usage.unwrap_or(0) as f64;
        let cpu_count = stats.cpu_stats.online_cpus.unwrap_or(1) as f64;

        let cpu_percent = if system_delta > 0.0 && cpu_delta > 0.0 {
            (cpu_delta / system_delta) * cpu_count * 100.0
        } else {
            0.0
        };

        // Memory stats
        let memory_usage = stats.memory_stats.usage.unwrap_or(0);
        let memory_limit = stats.memory_stats.limit.unwrap_or(1);
        let memory_percent = if memory_limit > 0 {
            (memory_usage as f64 / memory_limit as f64) * 100.0
        } else {
            0.0
        };

        // Network stats
        let (network_rx, network_tx) = stats.networks.as_ref().map_or((0, 0), |nets| {
            nets.values().fold((0, 0), |(rx, tx), net| {
                (rx + net.rx_bytes, tx + net.tx_bytes)
            })
        });

        Ok((
            cpu_percent,
            memory_usage,
            memory_limit,
            memory_percent,
            network_rx,
            network_tx,
        ))
    }
}

#[async_trait::async_trait]
impl crate::collectors::Collector for DockerCollector {
    fn name(&self) -> &'static str {
        "docker"
    }

    fn is_enabled(&self) -> bool {
        self.docker.is_some()
    }

    async fn collect(&self) -> Result<()> {
        let _ = self.collect_containers().await?;
        Ok(())
    }
}

impl Default for DockerCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collectors::Collector;

    #[tokio::test]
    async fn test_docker_collector_unavailable_returns_empty() {
        // When Docker is not available, collect_containers should gracefully return an empty list
        let collector = DockerCollector::new_none();
        assert!(!collector.is_available());

        let containers = collector.collect_containers().await.unwrap();
        assert!(containers.is_empty());
    }

    #[test]
    fn test_docker_collector_default() {
        let collector = DockerCollector::default();
        // Should not panic
        assert_eq!(collector.name(), "docker");
    }

    #[test]
    fn test_docker_collector_trait_methods() {
        let collector = DockerCollector::new_none();
        assert_eq!(collector.name(), "docker");
        assert!(!collector.is_enabled());
    }

    #[tokio::test]
    async fn test_docker_collector_collect_returns_ok() {
        let collector = DockerCollector::new_none();
        // When docker is unavailable, collect should still return Ok
        let result = collector.collect().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_docker_collector_default_impl() {
        let collector = DockerCollector::default();
        assert_eq!(collector.name(), "docker");
    }

    #[test]
    fn test_docker_collector_with_socket_path_does_not_panic() {
        // Use a non-existent socket path; on some platforms Docker may still
        // be available, so we only assert that the method does not panic and
        // returns a valid collector.
        let collector = DockerCollector::with_socket_path("/nonexistent/docker.sock");
        assert_eq!(collector.name(), "docker");
    }

    #[test]
    fn test_docker_collector_new_with_env_var_does_not_panic() {
        // Set a custom socket path via env var; should not panic.
        let original = std::env::var("DOCKER_SOCKET").ok();
        std::env::set_var("DOCKER_SOCKET", "/tmp/test-docker.sock");
        let collector = DockerCollector::new();
        assert_eq!(collector.name(), "docker");
        // Restore original env
        match original {
            Some(v) => std::env::set_var("DOCKER_SOCKET", v),
            None => std::env::remove_var("DOCKER_SOCKET"),
        }
    }

    #[test]
    fn test_docker_collector_is_available_false_when_none() {
        let collector = DockerCollector::new_none();
        assert!(!collector.is_available());
        assert!(!collector.is_enabled());
    }

    #[tokio::test]
    async fn test_docker_collector_collect_containers_none() {
        let collector = DockerCollector::new_none();
        let containers = collector.collect_containers().await.unwrap();
        assert!(containers.is_empty());
    }

    #[tokio::test]
    async fn test_docker_collector_collect_when_unavailable() {
        let collector = DockerCollector::with_socket_path("/tmp/fake-docker.sock");
        let result = collector.collect().await;
        assert!(result.is_ok());
    }
}
