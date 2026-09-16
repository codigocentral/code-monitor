//! Memory commitment: what the configuration promises versus what exists
//!
//! Conventional alerts are reactive — they fire once memory has already run
//! out. This is the arithmetic that can be done beforehand, because every term
//! is written in a configuration file.
//!
//! A postgres cluster with `work_mem = 32MB` and `max_connections = 500` can
//! ask for 16GB in sort buffers alone, on top of its shared buffers. That is a
//! defect in a config file, visible without waiting for the incident. Add the
//! container limits and the JVM heaps on the same host and the question
//! becomes answerable: does this machine promise more than it has?

use crate::types::{ContainerInfo, PostgresClusterInfo, PostgresSetting};
use serde::{Deserialize, Serialize};

/// One term of the commitment, kept separate so the total can be explained
///
/// A number without a culprit is not actionable: "your server is
/// overcommitted" sends someone hunting, "this instance promises 16GB in sort
/// buffers" does not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommitmentComponent {
    /// What promises the memory, in the terms an operator would grep for
    pub source: String,
    /// How the figure was arrived at, e.g. `work_mem × max_connections`
    pub basis: String,
    pub bytes: u64,
}

/// How badly a host is overcommitted
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommitmentLevel {
    /// Promises fit within a reasonable multiple of physical memory
    Ok,
    Warning,
    Critical,
}

/// What a host's configuration promises, against what it has
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryCommitment {
    pub components: Vec<CommitmentComponent>,
    pub physical_bytes: u64,
}

impl MemoryCommitment {
    /// Ratio above which the configuration is a warning.
    ///
    /// Some overcommit is normal and healthy: not every connection sorts at
    /// once, and containers rarely all peak together. Half again as much as
    /// the machine has is where it stops being prudent headroom.
    pub const WARNING_RATIO: f64 = 1.5;

    /// Ratio at which it becomes critical.
    pub const CRITICAL_RATIO: f64 = 3.0;

    /// Total promised memory.
    pub fn total_bytes(&self) -> u64 {
        self.components.iter().map(|c| c.bytes).sum()
    }

    /// Promised memory as a multiple of physical memory.
    pub fn ratio(&self) -> f64 {
        if self.physical_bytes == 0 {
            return 0.0;
        }
        self.total_bytes() as f64 / self.physical_bytes as f64
    }

    /// How badly the host is overcommitted.
    pub fn level(&self) -> CommitmentLevel {
        let ratio = self.ratio();
        if ratio >= Self::CRITICAL_RATIO {
            CommitmentLevel::Critical
        } else if ratio >= Self::WARNING_RATIO {
            CommitmentLevel::Warning
        } else {
            CommitmentLevel::Ok
        }
    }

    /// The largest contributors, so the alert can name a culprit.
    pub fn top_contributors(&self, count: usize) -> Vec<&CommitmentComponent> {
        let mut sorted: Vec<&CommitmentComponent> = self.components.iter().collect();
        sorted.sort_by_key(|c| std::cmp::Reverse(c.bytes));
        sorted.into_iter().take(count).collect()
    }
}

/// Convert a postgres setting to bytes, honouring the unit it reports.
///
/// `pg_settings` returns values in units of its own choosing — `shared_buffers`
/// in 8kB blocks, `work_mem` in kB — so the raw number means nothing on its
/// own. Settings measured in time return `None`.
pub fn setting_bytes(setting: &PostgresSetting) -> Option<u64> {
    let value: f64 = setting.value.parse().ok()?;
    if value < 0.0 {
        // -1 means "unset" or "derive from another setting" in several places
        return None;
    }

    let multiplier = match setting.unit.as_deref() {
        None | Some("") => 1.0,
        Some("B") => 1.0,
        Some("kB") => 1024.0,
        Some("MB") => 1024.0 * 1024.0,
        Some("GB") => 1024.0 * 1024.0 * 1024.0,
        Some("8kB") => 8.0 * 1024.0,
        Some("16kB") => 16.0 * 1024.0,
        Some("32kB") => 32.0 * 1024.0,
        Some("64kB") => 64.0 * 1024.0,
        // Time units: not memory
        Some(_) => return None,
    };

    Some((value * multiplier) as u64)
}

/// Look up a setting by name.
fn find<'a>(settings: &'a [PostgresSetting], name: &str) -> Option<&'a PostgresSetting> {
    settings.iter().find(|s| s.name == name)
}

/// A setting's value as a plain count, for settings without a unit.
fn setting_count(settings: &[PostgresSetting], name: &str) -> Option<u64> {
    find(settings, name)?.value.parse().ok()
}

/// Extract a JVM's maximum heap from its command line.
///
/// Accepts the forms `-Xmx2g`, `-Xmx512m`, `-Xmx1024k` and a bare byte count.
/// A JVM without an explicit `-Xmx` defaults to a quarter of physical memory,
/// which is not a promise anyone wrote down, so it is not counted.
pub fn jvm_heap_bytes(command_line: &str) -> Option<u64> {
    let start = command_line.find("-Xmx")? + "-Xmx".len();
    let rest = &command_line[start..];

    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }

    let value: u64 = digits.parse().ok()?;
    let suffix = rest[digits.len()..].chars().next().unwrap_or(' ');

    let multiplier = match suffix.to_ascii_lowercase() {
        'k' => 1024,
        'm' => 1024 * 1024,
        'g' => 1024 * 1024 * 1024,
        _ => 1,
    };

    value.checked_mul(multiplier)
}

/// Everything a postgres cluster's configuration promises.
///
/// Sort memory is the term that surprises people: `work_mem` is per sort
/// operation per connection, so it multiplies by `max_connections`. Four of
/// the fleet's instances allow 500 connections; the busiest ever used 71.
pub fn postgres_commitment(cluster: &PostgresClusterInfo) -> Vec<CommitmentComponent> {
    let settings = &cluster.settings;
    let mut components = Vec::new();

    if let Some(shared_buffers) = find(settings, "shared_buffers").and_then(setting_bytes) {
        components.push(CommitmentComponent {
            source: format!("postgres {}", cluster.name),
            basis: "shared_buffers".to_string(),
            bytes: shared_buffers,
        });
    }

    if let (Some(work_mem), Some(max_connections)) = (
        find(settings, "work_mem").and_then(setting_bytes),
        setting_count(settings, "max_connections"),
    ) {
        components.push(CommitmentComponent {
            source: format!("postgres {}", cluster.name),
            basis: format!("work_mem × max_connections ({})", max_connections),
            bytes: work_mem.saturating_mul(max_connections),
        });
    }

    if let (Some(maintenance), Some(workers)) = (
        find(settings, "maintenance_work_mem").and_then(setting_bytes),
        setting_count(settings, "autovacuum_max_workers"),
    ) {
        components.push(CommitmentComponent {
            source: format!("postgres {}", cluster.name),
            basis: format!("maintenance_work_mem × autovacuum workers ({})", workers),
            bytes: maintenance.saturating_mul(workers),
        });
    }

    components
}

/// What the container limits on a host promise.
///
/// Containers without a limit contribute nothing here — not because they are
/// harmless, but because they promise nothing measurable. They are a separate
/// finding, reported on the containers tab.
pub fn container_commitment(containers: &[ContainerInfo]) -> Option<CommitmentComponent> {
    let total: u64 = containers
        .iter()
        .filter(|c| c.memory_limit_set)
        .map(|c| c.memory_limit_bytes)
        .sum();

    if total == 0 {
        return None;
    }

    let limited = containers.iter().filter(|c| c.memory_limit_set).count();
    Some(CommitmentComponent {
        source: "containers".to_string(),
        basis: format!("sum of mem_limit across {} container(s)", limited),
        bytes: total,
    })
}

/// What the JVMs running on a host promise.
pub fn jvm_commitment(processes: &[(String, String)]) -> Option<CommitmentComponent> {
    let heaps: Vec<u64> = processes
        .iter()
        .filter_map(|(_, command_line)| jvm_heap_bytes(command_line))
        .collect();

    if heaps.is_empty() {
        return None;
    }

    Some(CommitmentComponent {
        source: "jvm".to_string(),
        basis: format!("-Xmx across {} process(es)", heaps.len()),
        bytes: heaps.iter().sum(),
    })
}

/// Assemble the whole picture for a host.
pub fn assess(
    physical_bytes: u64,
    postgres_clusters: &[PostgresClusterInfo],
    containers: &[ContainerInfo],
    processes: &[(String, String)],
) -> MemoryCommitment {
    let mut components: Vec<CommitmentComponent> = postgres_clusters
        .iter()
        .flat_map(postgres_commitment)
        .collect();

    components.extend(container_commitment(containers));
    components.extend(jvm_commitment(processes));

    MemoryCommitment {
        components,
        physical_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn setting(name: &str, value: &str, unit: Option<&str>) -> PostgresSetting {
        PostgresSetting {
            name: name.to_string(),
            value: value.to_string(),
            unit: unit.map(str::to_string),
            source: "configuration file".to_string(),
            source_file: Some("/etc/postgresql/postgresql.conf".to_string()),
            source_line: Some(1),
        }
    }

    fn cluster(name: &str, settings: Vec<PostgresSetting>) -> PostgresClusterInfo {
        PostgresClusterInfo {
            name: name.to_string(),
            host: "localhost".to_string(),
            port: 5432,
            databases: vec![],
            connections_total: 0,
            connections_by_state: vec![],
            cache_hit_ratio: 99.0,
            top_queries: vec![],
            timestamp: Utc::now(),
            settings,
        }
    }

    fn container(name: &str, limit_set: bool, limit: u64) -> ContainerInfo {
        ContainerInfo {
            id: name.to_string(),
            name: name.to_string(),
            image: "img".to_string(),
            status: "Up".to_string(),
            state: "running".to_string(),
            health: "none".to_string(),
            cpu_percent: 0.0,
            memory_usage_bytes: 0,
            memory_limit_bytes: limit,
            memory_percent: None,
            restart_count: 0,
            network_rx_bytes: 0,
            network_tx_bytes: 0,
            networks: vec![],
            memory_limit_set: limit_set,
            health_detail: None,
            swap_bytes: None,
            image_version: None,
        }
    }

    // ─────────────────────────────────────────
    // Unit conversion
    // ─────────────────────────────────────────

    #[test]
    fn test_shared_buffers_in_8kb_blocks() {
        // 524288 blocks of 8kB is 4GB
        let bytes = setting_bytes(&setting("shared_buffers", "524288", Some("8kB"))).unwrap();
        assert_eq!(bytes, 4 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_work_mem_in_kb() {
        let bytes = setting_bytes(&setting("work_mem", "32768", Some("kB"))).unwrap();
        assert_eq!(bytes, 32 * 1024 * 1024);
    }

    #[test]
    fn test_unitless_setting() {
        let bytes = setting_bytes(&setting("max_connections", "500", None)).unwrap();
        assert_eq!(bytes, 500);
    }

    #[test]
    fn test_time_units_are_not_memory() {
        assert!(setting_bytes(&setting("statement_timeout", "30000", Some("ms"))).is_none());
        assert!(setting_bytes(&setting("idle_timeout", "60", Some("s"))).is_none());
    }

    #[test]
    fn test_negative_setting_is_not_a_size() {
        // -1 means "unset" or "derive from elsewhere" in several settings
        assert!(setting_bytes(&setting("effective_cache_size", "-1", Some("8kB"))).is_none());
    }

    #[test]
    fn test_unparseable_setting() {
        assert!(setting_bytes(&setting("shared_buffers", "on", None)).is_none());
    }

    // ─────────────────────────────────────────
    // JVM heap
    // ─────────────────────────────────────────

    #[test]
    fn test_jvm_heap_suffixes() {
        assert_eq!(
            jvm_heap_bytes("java -Xmx2g -jar app.jar"),
            Some(2 * 1024 * 1024 * 1024)
        );
        assert_eq!(jvm_heap_bytes("java -Xmx512m"), Some(512 * 1024 * 1024));
        assert_eq!(jvm_heap_bytes("java -Xmx1024k"), Some(1024 * 1024));
        assert_eq!(jvm_heap_bytes("java -Xmx2048"), Some(2048));
    }

    #[test]
    fn test_jvm_heap_is_case_insensitive_on_the_suffix() {
        assert_eq!(jvm_heap_bytes("java -Xmx2G"), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(jvm_heap_bytes("java -Xmx512M"), Some(512 * 1024 * 1024));
    }

    #[test]
    fn test_jvm_heap_within_a_longer_command_line() {
        let cmd = "/usr/bin/java -Dfoo=bar -Xms512m -Xmx4g -XX:+UseG1GC -jar /opt/sonarqube.jar";
        assert_eq!(jvm_heap_bytes(cmd), Some(4 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_no_heap_flag() {
        // A JVM without -Xmx defaults to a quarter of RAM, which nobody
        // promised in writing
        assert!(jvm_heap_bytes("java -jar app.jar").is_none());
        assert!(jvm_heap_bytes("nginx: master process").is_none());
    }

    #[test]
    fn test_malformed_heap_flag() {
        assert!(jvm_heap_bytes("java -Xmx -jar app.jar").is_none());
        assert!(jvm_heap_bytes("java -Xmxbig").is_none());
    }

    // ─────────────────────────────────────────
    // Postgres commitment
    // ─────────────────────────────────────────

    #[test]
    fn test_postgres_sort_memory_multiplies_by_connections() {
        // Server pg-5433 configuration: 2GB shared, 16MB × 500
        let c = cluster(
            "pg-5433",
            vec![
                setting("shared_buffers", "262144", Some("8kB")),
                setting("work_mem", "16384", Some("kB")),
                setting("max_connections", "500", None),
            ],
        );

        let components = postgres_commitment(&c);
        assert_eq!(components.len(), 2);

        let shared = &components[0];
        assert_eq!(shared.bytes, 2 * 1024 * 1024 * 1024);

        let sort = &components[1];
        assert_eq!(sort.bytes, 16 * 1024 * 1024 * 500);
        assert!(sort.basis.contains("500"));
    }

    #[test]
    fn test_postgres_maintenance_memory() {
        let c = cluster(
            "pg",
            vec![
                setting("maintenance_work_mem", "65536", Some("kB")),
                setting("autovacuum_max_workers", "3", None),
            ],
        );

        let components = postgres_commitment(&c);
        assert_eq!(components[0].bytes, 64 * 1024 * 1024 * 3);
    }

    #[test]
    fn test_postgres_without_settings_promises_nothing() {
        assert!(postgres_commitment(&cluster("pg", vec![])).is_empty());
    }

    #[test]
    fn test_postgres_partial_settings() {
        // work_mem without max_connections cannot be multiplied out
        let c = cluster("pg", vec![setting("work_mem", "4096", Some("kB"))]);
        assert!(postgres_commitment(&c).is_empty());
    }

    // ─────────────────────────────────────────
    // Containers and JVMs
    // ─────────────────────────────────────────

    #[test]
    fn test_container_commitment_counts_only_limited_containers() {
        let containers = vec![
            container("a", true, 2_000_000_000),
            container("b", true, 1_000_000_000),
            container("c", false, 16_000_000_000), // no limit: promises nothing
        ];

        let component = container_commitment(&containers).unwrap();
        assert_eq!(component.bytes, 3_000_000_000);
        assert!(component.basis.contains('2'));
    }

    #[test]
    fn test_container_commitment_without_limits() {
        // Host with 20 of 20 containers unlimited
        let containers: Vec<ContainerInfo> = (0..20)
            .map(|i| container(&format!("c{}", i), false, 0))
            .collect();
        assert!(container_commitment(&containers).is_none());
    }

    #[test]
    fn test_jvm_commitment_sums_heaps() {
        let processes = vec![
            (
                "java".to_string(),
                "java -Xmx2g -jar sonarqube.jar".to_string(),
            ),
            (
                "java".to_string(),
                "java -Xmx1g -jar elasticsearch.jar".to_string(),
            ),
            ("nginx".to_string(), "nginx: master".to_string()),
        ];

        let component = jvm_commitment(&processes).unwrap();
        assert_eq!(component.bytes, 3 * 1024 * 1024 * 1024);
        assert!(component.basis.contains('2'));
    }

    #[test]
    fn test_jvm_commitment_without_jvms() {
        let processes = vec![("nginx".to_string(), "nginx: master".to_string())];
        assert!(jvm_commitment(&processes).is_none());
    }

    // ─────────────────────────────────────────
    // Overall assessment
    // ─────────────────────────────────────────

    #[test]
    fn test_healthy_host_is_not_overcommitted() {
        let commitment = assess(
            16 * 1024 * 1024 * 1024,
            &[cluster(
                "pg",
                vec![
                    setting("shared_buffers", "16384", Some("8kB")), // 128MB
                    setting("work_mem", "4096", Some("kB")),         // 4MB
                    setting("max_connections", "100", None),
                ],
            )],
            &[],
            &[],
        );

        assert_eq!(commitment.level(), CommitmentLevel::Ok);
        assert!(commitment.ratio() < 1.0);
    }

    #[test]
    fn test_postgres_alone_may_not_cross_the_threshold() {
        // Dev instance: 4GB shared plus 32MB × 500 is roughly 21GB
        // promised against 15.6GB — real overcommit, but 1.35× is still below
        // the warning line. Some overcommit is normal; the rule is about the
        // whole host, not one service.
        let commitment = assess(
            15_600_000_000,
            &[cluster(
                "pg-dev",
                vec![
                    setting("shared_buffers", "524288", Some("8kB")),
                    setting("work_mem", "32768", Some("kB")),
                    setting("max_connections", "500", None),
                ],
            )],
            &[],
            &[],
        );

        assert!(commitment.ratio() > 1.3 && commitment.ratio() < 1.5);
        assert_eq!(commitment.level(), CommitmentLevel::Ok);
    }

    #[test]
    fn test_heavy_host_is_overcommitted() {
        // The host as it actually stood: the dev postgres, the JVMs of
        // SonarQube and its Elasticsearch, and the containers that carried a
        // limit. Together they promise well past what the machine has.
        let commitment = assess(
            15_600_000_000,
            &[cluster(
                "pg-dev",
                vec![
                    setting("shared_buffers", "524288", Some("8kB")),
                    setting("work_mem", "32768", Some("kB")),
                    setting("max_connections", "500", None),
                ],
            )],
            &[
                container("sonarqube", true, 2_000_000_000),
                container("gitlab", true, 4_000_000_000),
                container("keycloak", true, 1_000_000_000),
            ],
            &[
                (
                    "java".to_string(),
                    "java -Xmx2g -jar sonarqube.jar".to_string(),
                ),
                (
                    "java".to_string(),
                    "java -Xmx1g -jar elasticsearch.jar".to_string(),
                ),
            ],
        );

        assert_eq!(commitment.level(), CommitmentLevel::Warning);
        assert!(
            commitment.ratio() > 1.5,
            "expected overcommit, got {:.2}×",
            commitment.ratio()
        );
    }

    #[test]
    fn test_top_contributor_names_the_culprit() {
        let commitment = assess(
            15_600_000_000,
            &[cluster(
                "pg-dev",
                vec![
                    setting("shared_buffers", "524288", Some("8kB")), // 4GB
                    setting("work_mem", "32768", Some("kB")),         // 32MB
                    setting("max_connections", "500", None),          // → 16GB
                ],
            )],
            &[container("app", true, 1_000_000_000)],
            &[],
        );

        let top = commitment.top_contributors(1);
        assert!(
            top[0].basis.contains("work_mem"),
            "sort memory is the largest term, not shared_buffers: {}",
            top[0].basis
        );
    }

    #[test]
    fn test_ratio_thresholds() {
        let make = |promised: u64| MemoryCommitment {
            components: vec![CommitmentComponent {
                source: "x".to_string(),
                basis: "y".to_string(),
                bytes: promised,
            }],
            physical_bytes: 1_000,
        };

        assert_eq!(make(1_400).level(), CommitmentLevel::Ok);
        assert_eq!(make(1_500).level(), CommitmentLevel::Warning);
        assert_eq!(make(2_900).level(), CommitmentLevel::Warning);
        assert_eq!(make(3_000).level(), CommitmentLevel::Critical);
    }

    #[test]
    fn test_empty_commitment_is_not_a_division_by_zero() {
        let commitment = MemoryCommitment::default();
        assert_eq!(commitment.ratio(), 0.0);
        assert_eq!(commitment.level(), CommitmentLevel::Ok);
        assert_eq!(commitment.total_bytes(), 0);
    }

    #[test]
    fn test_host_without_physical_memory_reading() {
        // Never divide by an unknown total
        let commitment = MemoryCommitment {
            components: vec![CommitmentComponent {
                source: "x".to_string(),
                basis: "y".to_string(),
                bytes: 1_000,
            }],
            physical_bytes: 0,
        };
        assert_eq!(commitment.ratio(), 0.0);
        assert_eq!(commitment.level(), CommitmentLevel::Ok);
    }

    #[test]
    fn test_components_from_every_source_are_combined() {
        let commitment = assess(
            8_000_000_000,
            &[cluster(
                "pg",
                vec![setting("shared_buffers", "131072", Some("8kB"))],
            )],
            &[container("app", true, 2_000_000_000)],
            &[("java".to_string(), "java -Xmx1g".to_string())],
        );

        let sources: Vec<&str> = commitment
            .components
            .iter()
            .map(|c| c.source.as_str())
            .collect();
        assert!(sources.contains(&"postgres pg"));
        assert!(sources.contains(&"containers"));
        assert!(sources.contains(&"jvm"));
    }
}
