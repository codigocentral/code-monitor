//! Docker container metrics collector
//!
//! This collector gathers metrics from Docker containers using the
//! Docker Engine API via the bollard crate.

use anyhow::Result;
use bollard::container::ListContainersOptions;
use bollard::container::StatsOptions;
use bollard::models::{ContainerInspectResponse, Health, HealthStatusEnum};
use bollard::Docker;
use chrono::{DateTime, Utc};
use futures_util::stream::StreamExt;
use shared::types::{ContainerHealthDetail, ContainerInfo};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, info, warn};
use crate::collectors::versions::VersionsCollector;

/// Cap on the stored healthcheck output.
///
/// Enough to recognise the failure — `curl: not found`, `Connection refused` —
/// without shipping a screenful of logs per container on every poll.
const MAX_HEALTH_OUTPUT_LEN: usize = 200;

/// What a single `inspect` adds on top of `list` and `stats`
#[derive(Debug, Default, Clone, PartialEq)]
struct InspectDetails {
    restart_count: u32,
    /// Whether `HostConfig.Memory` is actually set
    memory_limit_set: bool,
    /// The configured limit, when there is one
    memory_limit_bytes: Option<u64>,
    health_status: Option<String>,
    health_detail: Option<ContainerHealthDetail>,
}

/// Truncate healthcheck output on a character boundary.
fn truncate_health_output(output: &str, max_len: usize) -> String {
    let trimmed = output.trim();
    if trimmed.chars().count() <= max_len {
        return trimmed.to_string();
    }

    let truncated: String = trimmed.chars().take(max_len).collect();
    format!("{}…", truncated)
}

/// Map Docker's health status enum onto the string the UI shows.
fn health_status_label(status: Option<HealthStatusEnum>) -> String {
    match status {
        Some(HealthStatusEnum::HEALTHY) => "healthy",
        Some(HealthStatusEnum::UNHEALTHY) => "unhealthy",
        Some(HealthStatusEnum::STARTING) => "starting",
        Some(HealthStatusEnum::NONE) | Some(HealthStatusEnum::EMPTY) | None => "none",
    }
    .to_string()
}

/// Extract the last healthcheck result.
///
/// `Log` is oldest first, so the last entry is the most recent check.
fn extract_health_detail(health: &Health) -> ContainerHealthDetail {
    let last = health.log.as_ref().and_then(|log| log.last());

    ContainerHealthDetail {
        failing_streak: health.failing_streak.unwrap_or(0).max(0) as u32,
        last_output: last
            .and_then(|r| r.output.as_deref())
            .map(|o| truncate_health_output(o, MAX_HEALTH_OUTPUT_LEN))
            .unwrap_or_default(),
        last_exit_code: last.and_then(|r| r.exit_code).unwrap_or(0) as i32,
        last_checked_at: last.and_then(|r| r.start.as_ref()).and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        }),
    }
}

/// Pull the fields that only `inspect` exposes out of its response.
fn extract_inspect_details(inspect: &ContainerInspectResponse) -> InspectDetails {
    // Docker reports 0 for "no limit"; anything positive is a real cap
    let configured_limit = inspect
        .host_config
        .as_ref()
        .and_then(|hc| hc.memory)
        .filter(|m| *m > 0)
        .map(|m| m as u64);

    let health = inspect.state.as_ref().and_then(|s| s.health.as_ref());

    InspectDetails {
        restart_count: inspect.restart_count.unwrap_or(0).max(0) as u32,
        memory_limit_set: configured_limit.is_some(),
        memory_limit_bytes: configured_limit,
        health_status: health.map(|h| health_status_label(h.status)),
        health_detail: health.map(extract_health_detail),
    }
}

/// Memory usage against the limit, or `None` when there is no limit.
///
/// A container without a limit has no percentage: Docker fills `stats` with the
/// host's total RAM, which makes a container that could bring the machine down
/// look like it is using a comfortable fraction. Leaving it empty is honest.
fn memory_percent(usage: u64, limit: u64, limit_set: bool) -> Option<f64> {
    if !limit_set || limit == 0 {
        return None;
    }
    Some((usage as f64 / limit as f64) * 100.0)
}

/// Collector for Docker container metrics
pub struct DockerCollector {
    docker: Option<Docker>,
    _socket_path: String,
    connection_failed: AtomicBool,
    versions_collector: Arc<VersionsCollector>,
}

impl DockerCollector {
    #[cfg(test)]
    fn new_none() -> Self {
        Self {
            docker: None,
            _socket_path: String::new(),
            connection_failed: AtomicBool::new(false),
            versions_collector: Arc::new(VersionsCollector::default()),
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
            connection_failed: AtomicBool::new(false),
            versions_collector: Arc::new(VersionsCollector::default()),
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
            connection_failed: AtomicBool::new(false),
            versions_collector: Arc::new(VersionsCollector::default()),
        }
    }

    /// Check if Docker is available
    #[allow(dead_code)]
    pub fn is_available(&self) -> bool {
        self.docker.is_some() && !self.connection_failed.load(Ordering::SeqCst)
    }

    /// Collect container metrics
    ///
    /// `swap_by_container` maps container id to the swap its processes hold,
    /// which the caller reads once for the whole host rather than per
    /// container.
    pub async fn collect_containers(
        &self,
        swap_by_container: &std::collections::HashMap<String, u64>,
    ) -> Result<Vec<ContainerInfo>> {
        if self.connection_failed.load(Ordering::SeqCst) {
            return Ok(Vec::new());
        }

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
                let is_connect_error = e.to_string().to_lowercase().contains("connect");
                let already_failed = self.connection_failed.load(Ordering::SeqCst);
                if is_connect_error {
                    if !already_failed {
                        warn!(
                            "Docker daemon not available ({}). Docker metrics will be disabled.",
                            e
                        );
                        self.connection_failed.store(true, Ordering::SeqCst);
                    } else {
                        debug!("Docker daemon still unavailable; skipping container collection");
                    }
                } else {
                    warn!("Failed to list Docker containers: {}", e);
                }
                return Ok(Vec::new());
            }
        };

        let mut container_infos = Vec::new();

        for container in containers {
            let id = container.id.clone().unwrap_or_default();
            let id_for_swap = id.clone();
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
            let (cpu_percent, memory_usage, stats_limit, network_rx, network_tx) =
                if state == "running" && !id.is_empty() {
                    match self.get_container_stats(docker, &id).await {
                        Ok(stats) => stats,
                        Err(e) => {
                            warn!("Failed to get stats for container {}: {}", name, e);
                            (0.0, 0, 0, 0, 0)
                        }
                    }
                } else {
                    (0.0, 0, 0, 0, 0)
                };

            // One inspect answers three questions that list and stats cannot:
            // how often it restarted, whether a memory limit exists at all, and
            // what the healthcheck actually printed.
            let details = if id.is_empty() {
                InspectDetails::default()
            } else {
                match docker.inspect_container(&id, None).await {
                    Ok(inspect) => extract_inspect_details(&inspect),
                    Err(e) => {
                        warn!("Failed to inspect container {}: {}", name, e);
                        InspectDetails::default()
                    }
                }
            };

            // Prefer the configured limit; fall back to what stats reported,
            // which is the host's RAM when no limit is set.
            let memory_limit = details.memory_limit_bytes.unwrap_or(stats_limit);
            let memory_percent =
                memory_percent(memory_usage, memory_limit, details.memory_limit_set);

            // Health comes from inspect when available. The previous substring
            // match on the status text also matched "unhealthy" inside
            // "healthy", and carried no detail at all.
            let health = details.health_status.clone().unwrap_or_else(|| {
                let status_text = container.status.as_deref().unwrap_or("");
                if status_text.contains("unhealthy") {
                    "unhealthy".to_string()
                } else if status_text.contains("healthy") {
                    "healthy".to_string()
                } else {
                    "none".to_string()
                }
            });

            // Networks
            let networks: Vec<String> = container
                .network_settings
                .as_ref()
                .and_then(|ns| ns.networks.as_ref())
                .map(|n| n.keys().cloned().collect())
                .unwrap_or_default();

            let image_version = if !image.is_empty() {
                Some(self.versions_collector.check_image(docker, &image).await)
            } else {
                None
            };

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
                restart_count: details.restart_count,
                network_rx_bytes: network_rx,
                network_tx_bytes: network_tx,
                networks,
                memory_limit_set: details.memory_limit_set,
                health_detail: details.health_detail,
                swap_bytes: swap_by_container.get(&id_for_swap).copied(),
                image_version,
            });
        }

        Ok(container_infos)
    }

    /// Returns `(cpu_percent, memory_usage, memory_limit, network_rx, network_tx)`.
    ///
    /// The reported limit is the cgroup's, which equals the host's total RAM
    /// when the container has none; deciding whether it is a real limit is
    /// `inspect`'s job.
    async fn get_container_stats(
        &self,
        docker: &Docker,
        container_id: &str,
    ) -> Result<(f64, u64, u64, u64, u64)> {
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
        let memory_limit = stats.memory_stats.limit.unwrap_or(0);

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
        self.docker.is_some() && !self.connection_failed.load(Ordering::SeqCst)
    }

    async fn collect(&self) -> Result<()> {
        let _ = self
            .collect_containers(&std::collections::HashMap::new())
            .await?;
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

        let containers = collector
            .collect_containers(&std::collections::HashMap::new())
            .await
            .unwrap();
        assert!(containers.is_empty());
    }

    // ─────────────────────────────────────────
    // inspect extraction
    // ─────────────────────────────────────────

    fn inspect_with(memory: Option<i64>, restart_count: Option<i64>) -> ContainerInspectResponse {
        ContainerInspectResponse {
            restart_count,
            host_config: Some(bollard::models::HostConfig {
                memory,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_inspect_reports_real_restart_count() {
        // The mail server's netfilter container had restarted 412,813 times
        // without anything noticing, because the field was hardcoded to 0.
        let details = extract_inspect_details(&inspect_with(None, Some(412_813)));
        assert_eq!(details.restart_count, 412_813);
    }

    #[test]
    fn test_inspect_restart_count_absent_is_zero() {
        let details = extract_inspect_details(&ContainerInspectResponse::default());
        assert_eq!(details.restart_count, 0);
    }

    #[test]
    fn test_inspect_detects_configured_memory_limit() {
        let details = extract_inspect_details(&inspect_with(Some(2_000_000_000), None));
        assert!(details.memory_limit_set);
        assert_eq!(details.memory_limit_bytes, Some(2_000_000_000));
    }

    #[test]
    fn test_inspect_zero_memory_means_no_limit() {
        // Docker writes 0 for "unlimited" — not a limit of zero bytes
        let details = extract_inspect_details(&inspect_with(Some(0), None));
        assert!(!details.memory_limit_set);
        assert_eq!(details.memory_limit_bytes, None);
    }

    #[test]
    fn test_inspect_missing_host_config_means_no_limit() {
        let details = extract_inspect_details(&ContainerInspectResponse::default());
        assert!(!details.memory_limit_set);
    }

    // ─────────────────────────────────────────
    // memory percentage
    // ─────────────────────────────────────────

    #[test]
    fn test_memory_percent_with_limit() {
        let percent = memory_percent(1_000_000_000, 2_000_000_000, true).unwrap();
        assert!((percent - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_memory_percent_without_limit_is_none() {
        // wepper-pro-app held 1.3 GB with no limit and displayed "8.16% of
        // 15.62 GiB" — reassuring and meaningless.
        assert_eq!(memory_percent(1_300_000_000, 15_620_000_000, false), None);
    }

    #[test]
    fn test_memory_percent_zero_limit_is_none() {
        assert_eq!(memory_percent(100, 0, true), None);
    }

    // ─────────────────────────────────────────
    // health detail
    // ─────────────────────────────────────────

    fn health_with(streak: i64, output: &str, exit_code: i64) -> Health {
        Health {
            status: Some(HealthStatusEnum::UNHEALTHY),
            failing_streak: Some(streak),
            log: Some(vec![bollard::models::HealthcheckResult {
                start: Some("2026-07-31T13:39:13.123456789Z".to_string()),
                end: None,
                exit_code: Some(exit_code),
                output: Some(output.to_string()),
            }]),
        }
    }

    #[test]
    fn test_health_detail_carries_streak_and_output() {
        let detail = extract_health_detail(&health_with(162_460, "curl: not found", 127));

        assert_eq!(detail.failing_streak, 162_460);
        assert_eq!(detail.last_output, "curl: not found");
        assert_eq!(detail.last_exit_code, 127);
        assert!(detail.last_checked_at.is_some());
    }

    #[test]
    fn test_health_detail_uses_most_recent_log_entry() {
        // Docker orders Log oldest first
        let health = Health {
            status: Some(HealthStatusEnum::UNHEALTHY),
            failing_streak: Some(2),
            log: Some(vec![
                bollard::models::HealthcheckResult {
                    start: None,
                    end: None,
                    exit_code: Some(1),
                    output: Some("older failure".to_string()),
                },
                bollard::models::HealthcheckResult {
                    start: None,
                    end: None,
                    exit_code: Some(7),
                    output: Some("newest failure".to_string()),
                },
            ]),
        };

        let detail = extract_health_detail(&health);
        assert_eq!(detail.last_output, "newest failure");
        assert_eq!(detail.last_exit_code, 7);
    }

    #[test]
    fn test_health_detail_without_log() {
        let health = Health {
            status: Some(HealthStatusEnum::STARTING),
            failing_streak: Some(0),
            log: None,
        };

        let detail = extract_health_detail(&health);
        assert_eq!(detail.failing_streak, 0);
        assert!(detail.last_output.is_empty());
        assert!(detail.last_checked_at.is_none());
    }

    #[test]
    fn test_health_detail_parses_rfc3339_timestamp() {
        let detail = extract_health_detail(&health_with(1, "boom", 1));
        let checked = detail.last_checked_at.unwrap();
        assert_eq!(
            checked.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-07-31 13:39:13"
        );
    }

    #[test]
    fn test_health_detail_invalid_timestamp_is_none() {
        let health = Health {
            status: None,
            failing_streak: Some(1),
            log: Some(vec![bollard::models::HealthcheckResult {
                start: Some("not a date".to_string()),
                end: None,
                exit_code: Some(1),
                output: Some("x".to_string()),
            }]),
        };
        assert!(extract_health_detail(&health).last_checked_at.is_none());
    }

    // ─────────────────────────────────────────
    // output truncation
    // ─────────────────────────────────────────

    #[test]
    fn test_truncate_health_output_keeps_short_output() {
        assert_eq!(
            truncate_health_output("curl: not found", 200),
            "curl: not found"
        );
    }

    #[test]
    fn test_truncate_health_output_trims_whitespace() {
        assert_eq!(truncate_health_output("  boom\n", 200), "boom");
    }

    #[test]
    fn test_truncate_health_output_caps_long_output() {
        let long = "x".repeat(500);
        let truncated = truncate_health_output(&long, 200);
        assert_eq!(truncated.chars().count(), 201); // 200 plus the ellipsis
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn test_truncate_health_output_respects_char_boundaries() {
        // Truncating by bytes would panic here
        let multibyte = "ção ".repeat(100);
        let truncated = truncate_health_output(&multibyte, 10);
        assert_eq!(truncated.chars().count(), 11);
    }

    // ─────────────────────────────────────────
    // health status label
    // ─────────────────────────────────────────

    #[test]
    fn test_health_status_labels() {
        assert_eq!(
            health_status_label(Some(HealthStatusEnum::HEALTHY)),
            "healthy"
        );
        assert_eq!(
            health_status_label(Some(HealthStatusEnum::UNHEALTHY)),
            "unhealthy"
        );
        assert_eq!(
            health_status_label(Some(HealthStatusEnum::STARTING)),
            "starting"
        );
        assert_eq!(health_status_label(None), "none");
    }

    #[test]
    fn test_health_status_label_does_not_confuse_healthy_and_unhealthy() {
        // The old substring match found "healthy" inside "unhealthy"; order of
        // checks was the only thing keeping it correct.
        assert_ne!(
            health_status_label(Some(HealthStatusEnum::UNHEALTHY)),
            health_status_label(Some(HealthStatusEnum::HEALTHY))
        );
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
        let containers = collector
            .collect_containers(&std::collections::HashMap::new())
            .await
            .unwrap();
        assert!(containers.is_empty());
    }

    #[tokio::test]
    async fn test_docker_collector_collect_when_unavailable() {
        let collector = DockerCollector::with_socket_path("/tmp/fake-docker.sock");
        let result = collector.collect().await;
        assert!(result.is_ok());
    }
}
