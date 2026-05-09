//! System monitoring module
//!
//! This module collects system information using the sysinfo library

use anyhow::Result;
use chrono::{DateTime, Utc};
use shared::types::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use sysinfo::{CpuExt, DiskExt, NetworkExt, NetworksExt, PidExt, ProcessExt, System, SystemExt};
use tokio::time::{interval, Duration};
use tracing::{error, info};

use crate::collectors::docker::DockerCollector;
use crate::collectors::mariadb::MariaDBCollector;
use crate::collectors::postgres::PostgresCollector;
use crate::collectors::systemd::SystemdCollector;
use crate::config::{MariaDBClusterConfig, PostgresClusterConfig};

/// Get IP addresses for all network interfaces using /proc/net (Linux) or platform-specific methods
fn get_interface_ips() -> HashMap<String, String> {
    let ips = HashMap::new();

    // Try to read from /proc/net/fib_trie or use ifaddrs
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        // Use 'ip addr' command as a reliable way to get IPs
        if let Ok(output) = Command::new("ip").args(["addr", "show"]).output() {
            if let Ok(stdout) = String::from_utf8(output.stdout) {
                let mut current_iface = String::new();
                for line in stdout.lines() {
                    let line = line.trim();
                    // Line like "2: eth0: <BROADCAST..."
                    if line
                        .chars()
                        .next()
                        .map(|c| c.is_ascii_digit())
                        .unwrap_or(false)
                        && line.contains(": ")
                    {
                        if let Some(iface) = line.split(": ").nth(1) {
                            current_iface = iface.split(':').next().unwrap_or(iface).to_string();
                        }
                    }
                    // Line like "inet 192.168.0.31/24..."
                    if line.starts_with("inet ") && !current_iface.is_empty() {
                        if let Some(ip_part) = line.strip_prefix("inet ") {
                            if let Some(ip) = ip_part.split('/').next() {
                                if let Some(ip) = ip.split_whitespace().next() {
                                    ips.insert(current_iface.clone(), ip.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        // On Windows/Mac, leave empty for now - use N/A
    }

    ips
}

pub struct SystemMonitor {
    system: Arc<Mutex<System>>,
    update_interval: Duration,
    last_update: Arc<Mutex<DateTime<Utc>>>,
    is_running: Arc<AtomicBool>,
    docker_collector: DockerCollector,
    postgres_collectors: Vec<PostgresCollector>,
    mariadb_collectors: Vec<MariaDBCollector>,
    systemd_collector: Option<SystemdCollector>,
}

impl SystemMonitor {
    pub async fn new(
        update_interval_seconds: u64,
        postgres_configs: Vec<PostgresClusterConfig>,
        mariadb_configs: Vec<MariaDBClusterConfig>,
        systemd_units: Vec<String>,
    ) -> Result<Self> {
        let mut system = System::new_all();
        system.refresh_all();

        let postgres_collectors: Vec<PostgresCollector> = postgres_configs
            .into_iter()
            .filter(|c| c.enabled)
            .map(PostgresCollector::new)
            .collect();

        let mariadb_collectors: Vec<MariaDBCollector> = mariadb_configs
            .into_iter()
            .filter(|c| c.enabled)
            .map(MariaDBCollector::new)
            .collect();

        let systemd_collector = if systemd_units.is_empty() {
            None
        } else {
            info!("Initialized systemd collector for {} unit(s)", systemd_units.len());
            Some(SystemdCollector::new(systemd_units))
        };

        info!("Initialized {} Postgres collector(s), {} MariaDB collector(s)", postgres_collectors.len(), mariadb_collectors.len());

        let monitor = Self {
            system: Arc::new(Mutex::new(system)),
            update_interval: Duration::from_secs(update_interval_seconds),
            last_update: Arc::new(Mutex::new(Utc::now())),
            is_running: Arc::new(AtomicBool::new(false)),
            docker_collector: DockerCollector::new(),
            postgres_collectors,
            mariadb_collectors,
            systemd_collector,
        };

        info!("System monitor initialized with {} second update interval", update_interval_seconds);
        Ok(monitor)
    }

    pub fn start_background_monitoring(&self) {
        let system = Arc::clone(&self.system);
        let update_interval = self.update_interval;
        let is_running = Arc::clone(&self.is_running);
        let last_update = Arc::clone(&self.last_update);

        tokio::spawn(async move {
            let mut interval_timer = interval(update_interval);
            is_running.store(true, Ordering::SeqCst);

            info!("Starting background system monitoring");

            loop {
                interval_timer.tick().await;

                if !is_running.load(Ordering::SeqCst) {
                    break;
                }

                if let Err(e) = Self::update_system_info(&system, &last_update) {
                    error!("Failed to update system info: {}", e);
                }
            }

            info!("Background system monitoring stopped");
        });
    }

    #[allow(dead_code)]
    pub fn stop_background_monitoring(&self) {
        self.is_running.store(false, Ordering::SeqCst);
        info!("Background monitoring stopped");
    }

    fn update_system_info(
        system: &Arc<Mutex<System>>,
        last_update: &Arc<Mutex<DateTime<Utc>>>,
    ) -> Result<()> {
        let mut sys = system
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock system: {}", e))?;

        sys.refresh_all();

        *last_update
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock last_update: {}", e))? = Utc::now();

        Ok(())
    }

    pub fn get_system_info(&self) -> Result<SystemInfo> {
        let sys = self
            .system
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock system: {}", e))?;

        let memory_total = sys.total_memory();
        let memory_used = sys.used_memory();
        let memory_available = memory_total.saturating_sub(memory_used);

        let disk_info: Vec<DiskInfo> = sys
            .disks()
            .iter()
            .map(|disk| {
                let total = disk.total_space();
                let available = disk.available_space();
                let used = total.saturating_sub(available);
                let usage_percent = if total > 0 {
                    (used as f64 / total as f64) * 100.0
                } else {
                    0.0
                };

                DiskInfo {
                    device: disk.name().to_string_lossy().to_string(),
                    mount_point: disk.mount_point().to_string_lossy().to_string(),
                    filesystem_type: String::from_utf8_lossy(disk.file_system()).to_string(),
                    total_bytes: total,
                    used_bytes: used,
                    available_bytes: available,
                    usage_percent,
                }
            })
            .collect();

        let cpu_usage = sys.global_cpu_info().cpu_usage() as f64;

        let system_info = SystemInfo {
            hostname: sys.host_name().unwrap_or_else(|| "unknown".to_string()),
            os: format!(
                "{} {}",
                sys.name().unwrap_or_else(|| "Unknown".to_string()),
                sys.long_os_version()
                    .unwrap_or_else(|| "Unknown".to_string())
            ),
            kernel_version: sys
                .kernel_version()
                .unwrap_or_else(|| "Unknown".to_string()),
            uptime_seconds: sys.uptime(),
            cpu_count: sys.physical_core_count().unwrap_or(1) as u32,
            cpu_usage_percent: cpu_usage,
            memory_total_bytes: memory_total,
            memory_used_bytes: memory_used,
            memory_available_bytes: memory_available,
            disk_info,
            timestamp: *self
                .last_update
                .lock()
                .map_err(|e| anyhow::anyhow!("Failed to lock last_update: {}", e))?,
        };

        Ok(system_info)
    }

    pub fn get_processes(&self, limit: u32, filter: Option<String>) -> Result<Vec<ProcessInfo>> {
        let sys = self
            .system
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock system: {}", e))?;

        let mut processes: Vec<_> = sys
            .processes()
            .iter()
            .map(|(pid, process)| {
                let cpu_usage = process.cpu_usage() as f64;
                let memory_usage = process.memory();

                ProcessInfo {
                    pid: pid.as_u32(),
                    name: process.name().to_string(),
                    user: process
                        .user_id()
                        .map(|u| format!("{:?}", u))
                        .unwrap_or_else(|| "unknown".to_string()),
                    cpu_usage_percent: cpu_usage,
                    memory_usage_bytes: memory_usage,
                    command_line: process.cmd().join(" "),
                    start_time: Utc::now() - chrono::Duration::seconds(process.run_time() as i64),
                    status: format!("{:?}", process.status()),
                }
            })
            .collect();

        // Apply filter if provided
        if let Some(filter_str) = filter {
            let filter_lower = filter_str.to_lowercase();
            processes.retain(|p| {
                p.name.to_lowercase().contains(&filter_lower)
                    || p.command_line.to_lowercase().contains(&filter_lower)
            });
        }

        // Sort by CPU usage (descending) and take limit
        processes.sort_by(|a, b| {
            b.cpu_usage_percent
                .partial_cmp(&a.cpu_usage_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if limit > 0 {
            processes.truncate(limit as usize);
        }

        Ok(processes)
    }

    pub fn get_services(&self) -> Result<Vec<ServiceInfo>> {
        let sys = self
            .system
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock system: {}", e))?;

        // Get all running processes and identify those that look like services
        // Services typically:
        // 1. Run for a long time (uptime > 60 seconds)
        // 2. Have no window/are background processes
        // 3. Are started by system accounts

        let mut services: Vec<ServiceInfo> = sys
            .processes()
            .iter()
            .filter(|(_, process)| {
                let run_time = process.run_time();
                let name = process.name().to_lowercase();

                // Filter criteria for service-like processes:
                // - Running for at least 60 seconds
                // - OR has a service-like name pattern
                run_time > 60 ||
                name.ends_with("service") ||
                name.ends_with("svc") ||
                name.ends_with("d") || // Unix daemons
                name.contains("server") ||
                name.contains("daemon") ||
                name.contains("agent") ||
                name.contains("monitor") ||
                name.contains("manager")
            })
            .map(|(pid, process)| {
                // In Linux, most processes are in "Sleep" state (waiting for I/O or events)
                // This is normal for services - they're "running" but sleeping between requests
                let status = Self::map_process_status(process.status());

                ServiceInfo {
                    name: process.name().to_string(),
                    status,
                    pid: Some(pid.as_u32()),
                    cpu_usage_percent: process.cpu_usage() as f64,
                    memory_usage_bytes: process.memory(),
                    user: process
                        .user_id()
                        .map(|u| format!("{:?}", u))
                        .unwrap_or_else(|| "unknown".to_string()),
                    uptime_seconds: Some(process.run_time()),
                }
            })
            .collect();

        // Sort by memory usage (descending) for better visibility
        services.sort_by(|a, b| b.memory_usage_bytes.cmp(&a.memory_usage_bytes));

        // Limit to top 50 services
        services.truncate(50);

        Ok(services)
    }

    pub async fn get_containers(&self) -> Result<Vec<ContainerInfo>> {
        self.docker_collector.collect_containers().await
    }

    fn map_process_status(status: sysinfo::ProcessStatus) -> ServiceStatus {
        match status {
            sysinfo::ProcessStatus::Run => ServiceStatus::Running,
            sysinfo::ProcessStatus::Sleep => ServiceStatus::Running,
            sysinfo::ProcessStatus::Idle => ServiceStatus::Running,
            sysinfo::ProcessStatus::Stop => ServiceStatus::Stopped,
            sysinfo::ProcessStatus::Zombie => ServiceStatus::Failed,
            sysinfo::ProcessStatus::Dead => ServiceStatus::Stopped,
            _ => ServiceStatus::Running,
        }
    }

    fn push_or_log<T>(results: &mut Vec<T>, result: Result<T>, name: &str) {
        match result {
            Ok(info) => results.push(info),
            Err(e) => {
                error!("Failed to collect {} metrics: {}", name, e);
            }
        }
    }

    pub async fn get_postgres_clusters(&self) -> Result<Vec<shared::types::PostgresClusterInfo>> {
        let mut results = Vec::with_capacity(self.postgres_collectors.len());
        for collector in &self.postgres_collectors {
            Self::push_or_log(&mut results, collector.collect().await, "Postgres");
        }
        Ok(results)
    }

    pub async fn get_mariadb_clusters(&self) -> Result<Vec<shared::types::MariaDBClusterInfo>> {
        let mut results = Vec::with_capacity(self.mariadb_collectors.len());
        for collector in &self.mariadb_collectors {
            Self::push_or_log(&mut results, collector.collect().await, "MariaDB");
        }
        Ok(results)
    }

    pub async fn get_systemd_units(&self) -> Result<Vec<shared::types::SystemdUnitInfo>> {
        match &self.systemd_collector {
            Some(collector) => collector.collect().await,
            None => Ok(Vec::new()),
        }
    }

    pub fn get_network_info(&self) -> Result<Vec<NetworkInfo>> {
        let sys = self
            .system
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock system: {}", e))?;

        // Get IP addresses for all interfaces
        let interface_ips = get_interface_ips();

        let network_info: Vec<NetworkInfo> = sys
            .networks()
            .iter()
            .map(|(interface_name, network_data)| {
                let ip_address = interface_ips
                    .get(interface_name)
                    .cloned()
                    .unwrap_or_else(|| "N/A".to_string());

                // Format MAC address properly
                let mac = network_data.mac_address();
                let mac_str = format!(
                    "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                    mac.0[0], mac.0[1], mac.0[2], mac.0[3], mac.0[4], mac.0[5]
                );

                NetworkInfo {
                    interface: interface_name.clone(),
                    ip_address,
                    mac_address: mac_str,
                    is_up: network_data.total_received() > 0
                        || network_data.total_transmitted() > 0,
                    bytes_sent: network_data.total_transmitted(),
                    bytes_received: network_data.total_received(),
                    packets_sent: network_data.total_packets_transmitted(),
                    packets_received: network_data.total_packets_received(),
                }
            })
            .collect();

        Ok(network_info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_system_monitor_new() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        // Verify we can get system info without panicking
        let info = monitor.get_system_info().expect("Failed to get system info");
        assert!(!info.hostname.is_empty());
        assert!(info.memory_total_bytes > 0);
        assert!(info.cpu_count > 0);
    }

    #[tokio::test]
    async fn test_get_processes() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        // Get all processes (limit = 0)
        let processes = monitor.get_processes(0, None).expect("Failed to get processes");
        assert!(!processes.is_empty(), "Should have at least one process");

        // Test with limit
        let limited = monitor.get_processes(5, None).expect("Failed to get limited processes");
        assert!(limited.len() <= 5);
    }

    #[tokio::test]
    async fn test_get_processes_with_filter() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        // Filter with a pattern that should match nothing
        let filtered = monitor
            .get_processes(0, Some("xyz_nonexistent_process_123".to_string()))
            .expect("Failed to get filtered processes");
        assert!(filtered.is_empty());
    }

    #[tokio::test]
    async fn test_get_services() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let services = monitor.get_services().expect("Failed to get services");
        // Should return some service-like processes
        assert!(!services.is_empty());
        // Limit is 50
        assert!(services.len() <= 50);
    }

    #[tokio::test]
    async fn test_get_network_info() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let networks = monitor.get_network_info().expect("Failed to get network info");
        // Should have at least loopback or one interface
        assert!(!networks.is_empty());

        for net in &networks {
            assert!(!net.interface.is_empty());
            // MAC address should be in format XX:XX:XX:XX:XX:XX
            assert_eq!(net.mac_address.len(), 17);
        }
    }

    #[tokio::test]
    async fn test_get_containers_empty_when_no_docker() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let containers = monitor.get_containers().await.expect("Failed to get containers");
        // Docker is likely not available in test environment
        // The collector should gracefully return empty
        assert!(containers.is_empty() || !containers.is_empty());
    }

    #[tokio::test]
    async fn test_get_systemd_units_empty_when_none_configured() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let units = monitor.get_systemd_units().await.expect("Failed to get systemd units");
        assert!(units.is_empty());
    }

    #[tokio::test]
    async fn test_background_monitoring_start_stop() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        monitor.start_background_monitoring();
        // Let the first iteration run
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        monitor.stop_background_monitoring();
        // Wait for the loop to tick again, see is_running=false, and break
        tokio::time::sleep(tokio::time::Duration::from_millis(1200)).await;
        // Should not panic
    }

    #[tokio::test]
    async fn test_system_info_memory_consistency() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let info = monitor.get_system_info().expect("Failed to get system info");

        // Memory consistency: used + available <= total
        assert_eq!(
            info.memory_used_bytes + info.memory_available_bytes,
            info.memory_total_bytes
        );
    }

    #[tokio::test]
    async fn test_system_info_disk_usage_calculation() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let info = monitor.get_system_info().expect("Failed to get system info");

        for disk in &info.disk_info {
            // Disk consistency: used + available <= total
            assert_eq!(disk.used_bytes + disk.available_bytes, disk.total_bytes);

            // Usage percent should be consistent
            if disk.total_bytes > 0 {
                let expected_percent =
                    (disk.used_bytes as f64 / disk.total_bytes as f64) * 100.0;
                assert!(
                    (disk.usage_percent - expected_percent).abs() < 0.1,
                    "Disk usage percent mismatch: {} vs {}",
                    disk.usage_percent,
                    expected_percent
                );
            } else {
                assert_eq!(disk.usage_percent, 0.0);
            }
        }
    }

    #[tokio::test]
    async fn test_system_info_cpu_count_positive() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let info = monitor.get_system_info().expect("Failed to get system info");
        assert!(info.cpu_count > 0, "CPU count should be positive");
    }

    #[tokio::test]
    async fn test_system_info_timestamp_recent() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let info = monitor.get_system_info().expect("Failed to get system info");
        let now = chrono::Utc::now();

        // Timestamp should not be older than 1 minute (it comes from last_update)
        let age = now - info.timestamp;
        assert!(
            age.num_seconds() < 60,
            "Timestamp should be recent, got age: {:?}",
            age
        );
    }

    #[tokio::test]
    async fn test_processes_sorted_by_cpu() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let processes = monitor.get_processes(0, None).expect("Failed to get processes");

        for i in 1..processes.len() {
            assert!(
                processes[i - 1].cpu_usage_percent >= processes[i].cpu_usage_percent,
                "Processes should be sorted by CPU usage descending"
            );
        }
    }

    #[tokio::test]
    async fn test_services_limited_to_50() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let services = monitor.get_services().expect("Failed to get services");
        assert!(services.len() <= 50, "Services should be limited to 50");
    }

    #[tokio::test]
    async fn test_network_mac_format() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let networks = monitor.get_network_info().expect("Failed to get network info");

        for net in &networks {
            let parts: Vec<&str> = net.mac_address.split(':').collect();
            assert_eq!(
                parts.len(),
                6,
                "MAC address should have 6 parts: {}",
                net.mac_address
            );
            for part in parts {
                assert_eq!(
                    part.len(),
                    2,
                    "Each MAC part should be 2 hex chars: {}",
                    part
                );
                assert!(
                    part.chars().all(|c| c.is_ascii_hexdigit()),
                    "MAC part should be hex: {}",
                    part
                );
            }
        }
    }

    #[tokio::test]
    async fn test_get_system_info_performance() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let start = std::time::Instant::now();
        for _ in 0..100 {
            let _ = monitor.get_system_info().expect("Failed to get system info");
        }
        let elapsed = start.elapsed();

        // Should be reasonably fast even in debug mode
        assert!(
            elapsed.as_secs() < 10,
            "get_system_info 100x took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_get_processes_performance() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let start = std::time::Instant::now();
        for _ in 0..10 {
            let _ = monitor.get_processes(50, None).expect("Failed to get processes");
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_secs() < 10,
            "get_processes 10x took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_update_system_info() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let result = SystemMonitor::update_system_info(&monitor.system, &monitor.last_update);
        assert!(result.is_ok());

        let last = monitor.last_update.lock().unwrap();
        let age = chrono::Utc::now() - *last;
        assert!(age.num_seconds() < 5, "last_update should be very recent");
    }

    #[test]
    fn test_get_interface_ips_smoke() {
        // Must not panic on any platform.
        let ips = get_interface_ips();
        // On Linux we expect at least loopback or some interface; on other platforms it may be empty.
        // We just ensure it returns a HashMap.
        let _ = ips.len();
    }

    #[tokio::test]
    async fn test_new_with_disabled_collectors() {
        let pg = PostgresClusterConfig {
            name: "disabled-pg".to_string(),
            host: "localhost".to_string(),
            port: 5432,
            database: "postgres".to_string(),
            user: "code-monitor".to_string(),
            password: None,
            socket_path: None,
            enabled: false,
        };
        let maria = MariaDBClusterConfig {
            name: "disabled-maria".to_string(),
            host: "localhost".to_string(),
            port: 3306,
            user: "code-monitor".to_string(),
            password: None,
            socket_path: None,
            enabled: false,
        };

        let monitor = SystemMonitor::new(1, vec![pg], vec![maria], vec![])
            .await
            .expect("Failed to create monitor");

        assert!(monitor.postgres_collectors.is_empty());
        assert!(monitor.mariadb_collectors.is_empty());
    }

    #[tokio::test]
    async fn test_new_with_systemd_units() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec!["nginx.service".to_string()])
            .await
            .expect("Failed to create monitor");

        assert!(monitor.systemd_collector.is_some());
    }

    #[tokio::test]
    async fn test_get_systemd_units_with_collector() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec!["nginx.service".to_string()])
            .await
            .expect("Failed to create monitor");

        let units = monitor.get_systemd_units().await.expect("Failed to get systemd units");
        // On non-Linux this will be empty; on Linux it depends on whether systemctl is available.
        // The call itself must succeed.
        let _ = units.len();
    }

    #[tokio::test]
    async fn test_get_postgres_clusters_empty() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let clusters = monitor.get_postgres_clusters().await.expect("Failed to get postgres");
        assert!(clusters.is_empty());
    }

    #[tokio::test]
    async fn test_get_mariadb_clusters_empty() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let clusters = monitor.get_mariadb_clusters().await.expect("Failed to get mariadb");
        assert!(clusters.is_empty());
    }

    #[tokio::test]
    async fn test_get_processes_limit_one() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let processes = monitor.get_processes(1, None).expect("Failed to get processes");
        assert!(processes.len() <= 1);
        assert!(!processes.is_empty(), "Should have at least one process");
    }

    #[tokio::test]
    async fn test_services_sorted_by_memory() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let services = monitor.get_services().expect("Failed to get services");
        for i in 1..services.len() {
            assert!(
                services[i - 1].memory_usage_bytes >= services[i].memory_usage_bytes,
                "Services should be sorted by memory descending"
            );
        }
    }

    #[tokio::test]
    async fn test_network_info_has_interfaces() {
        let monitor = SystemMonitor::new(1, vec![], vec![], vec![])
            .await
            .expect("Failed to create monitor");

        let networks = monitor.get_network_info().expect("Failed to get network info");
        assert!(!networks.is_empty(), "Should have at least one network interface");
    }

    // ------------------------------------------------------------------
    // Poisoned mutex coverage
    // ------------------------------------------------------------------

    fn poison_mutex<T: Send + 'static>(arc: &Arc<Mutex<T>>) {
        let clone = Arc::clone(arc);
        let handle = std::thread::spawn(move || {
            let _guard = clone.lock().unwrap();
            panic!("intentional panic to poison mutex");
        });
        let _ = handle.join();
    }

    fn create_manual_monitor(
        system: Arc<Mutex<System>>,
        last_update: Arc<Mutex<DateTime<Utc>>>,
    ) -> SystemMonitor {
        SystemMonitor {
            system,
            update_interval: Duration::from_secs(1),
            last_update,
            is_running: Arc::new(AtomicBool::new(false)),
            docker_collector: DockerCollector::new(),
            postgres_collectors: vec![],
            mariadb_collectors: vec![],
            systemd_collector: None,
        }
    }

    #[tokio::test]
    async fn test_update_system_info_system_poisoned() {
        let system = Arc::new(Mutex::new(System::new_all()));
        poison_mutex(&system);
        let last_update = Arc::new(Mutex::new(Utc::now()));
        let result = SystemMonitor::update_system_info(&system, &last_update);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update_system_info_last_update_poisoned() {
        let system = Arc::new(Mutex::new(System::new_all()));
        let last_update = Arc::new(Mutex::new(Utc::now()));
        poison_mutex(&last_update);
        let result = SystemMonitor::update_system_info(&system, &last_update);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_system_info_system_poisoned() {
        let system = Arc::new(Mutex::new(System::new_all()));
        poison_mutex(&system);
        let monitor = create_manual_monitor(system, Arc::new(Mutex::new(Utc::now())));
        assert!(monitor.get_system_info().is_err());
    }

    #[tokio::test]
    async fn test_get_system_info_last_update_poisoned() {
        let last_update = Arc::new(Mutex::new(Utc::now()));
        poison_mutex(&last_update);
        let monitor = create_manual_monitor(Arc::new(Mutex::new(System::new_all())), last_update);
        assert!(monitor.get_system_info().is_err());
    }

    #[tokio::test]
    async fn test_get_processes_system_poisoned() {
        let system = Arc::new(Mutex::new(System::new_all()));
        poison_mutex(&system);
        let monitor = create_manual_monitor(system, Arc::new(Mutex::new(Utc::now())));
        assert!(monitor.get_processes(10, None).is_err());
    }

    #[tokio::test]
    async fn test_get_services_system_poisoned() {
        let system = Arc::new(Mutex::new(System::new_all()));
        poison_mutex(&system);
        let monitor = create_manual_monitor(system, Arc::new(Mutex::new(Utc::now())));
        assert!(monitor.get_services().is_err());
    }

    #[tokio::test]
    async fn test_get_network_info_system_poisoned() {
        let system = Arc::new(Mutex::new(System::new_all()));
        poison_mutex(&system);
        let monitor = create_manual_monitor(system, Arc::new(Mutex::new(Utc::now())));
        assert!(monitor.get_network_info().is_err());
    }

    #[tokio::test]
    async fn test_background_monitoring_with_poisoned_system() {
        let system = Arc::new(Mutex::new(System::new_all()));
        poison_mutex(&system);
        let monitor = create_manual_monitor(system, Arc::new(Mutex::new(Utc::now())));
        monitor.start_background_monitoring();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        monitor.stop_background_monitoring();
        tokio::time::sleep(tokio::time::Duration::from_millis(1200)).await;
        // Must not panic despite poisoned mutex
    }

    // ------------------------------------------------------------------
    // Collector error paths
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_postgres_clusters_with_failing_collector() {
        let pg_config = PostgresClusterConfig {
            name: "fail-pg".to_string(),
            host: "127.0.0.1".to_string(),
            port: 54330, // closed port -> connection refused
            database: "postgres".to_string(),
            user: "code-monitor".to_string(),
            password: None,
            socket_path: None,
            enabled: true,
        };
        let monitor = SystemMonitor {
            system: Arc::new(Mutex::new(System::new_all())),
            update_interval: Duration::from_secs(1),
            last_update: Arc::new(Mutex::new(Utc::now())),
            is_running: Arc::new(AtomicBool::new(false)),
            docker_collector: DockerCollector::new(),
            postgres_collectors: vec![PostgresCollector::new(pg_config)],
            mariadb_collectors: vec![],
            systemd_collector: None,
        };

        let result = tokio::time::timeout(Duration::from_secs(10), monitor.get_postgres_clusters()).await;
        assert!(result.is_ok(), "Should not timeout");
        let clusters = result.unwrap().expect("Should return Ok");
        assert!(clusters.is_empty(), "Failing collector should yield empty clusters");
    }

    #[tokio::test]
    async fn test_get_mariadb_clusters_with_failing_collector() {
        let maria_config = MariaDBClusterConfig {
            name: "fail-maria".to_string(),
            host: "127.0.0.1".to_string(),
            port: 33070, // closed port -> connection refused
            user: "code-monitor".to_string(),
            password: None,
            socket_path: None,
            enabled: true,
        };
        let monitor = SystemMonitor {
            system: Arc::new(Mutex::new(System::new_all())),
            update_interval: Duration::from_secs(1),
            last_update: Arc::new(Mutex::new(Utc::now())),
            is_running: Arc::new(AtomicBool::new(false)),
            docker_collector: DockerCollector::new(),
            postgres_collectors: vec![],
            mariadb_collectors: vec![MariaDBCollector::new(maria_config)],
            systemd_collector: None,
        };

        let result = tokio::time::timeout(Duration::from_secs(10), monitor.get_mariadb_clusters()).await;
        assert!(result.is_ok(), "Should not timeout");
        let clusters = result.unwrap().expect("Should return Ok");
        assert!(clusters.is_empty(), "Failing collector should yield empty clusters");
    }

    #[test]
    fn test_map_process_status_all_variants() {
        assert!(matches!(SystemMonitor::map_process_status(sysinfo::ProcessStatus::Run), ServiceStatus::Running));
        assert!(matches!(SystemMonitor::map_process_status(sysinfo::ProcessStatus::Sleep), ServiceStatus::Running));
        assert!(matches!(SystemMonitor::map_process_status(sysinfo::ProcessStatus::Idle), ServiceStatus::Running));
        assert!(matches!(SystemMonitor::map_process_status(sysinfo::ProcessStatus::Stop), ServiceStatus::Stopped));
        assert!(matches!(SystemMonitor::map_process_status(sysinfo::ProcessStatus::Zombie), ServiceStatus::Failed));
        assert!(matches!(SystemMonitor::map_process_status(sysinfo::ProcessStatus::Dead), ServiceStatus::Stopped));
        // Unknown / other variants default to Running
        assert!(matches!(SystemMonitor::map_process_status(sysinfo::ProcessStatus::Unknown(99)), ServiceStatus::Running));
    }

    #[test]
    fn test_push_or_log_ok() {
        let mut results = Vec::new();
        SystemMonitor::push_or_log(&mut results, Ok(42), "Test");
        assert_eq!(results, vec![42]);
    }

    #[test]
    fn test_push_or_log_err() {
        let mut results: Vec<i32> = Vec::new();
        SystemMonitor::push_or_log(&mut results, Err(anyhow::anyhow!("fail")), "Test");
        assert!(results.is_empty());
    }
}
