//! Periodic audit of the host, exported in the Prometheus text format
//!
//! The collectors used to answer only when a client asked over gRPC, so a
//! finding existed only while someone had the dashboard open. This module runs
//! the same collectors on a timer, keeps the latest snapshot in memory, and
//! renders it — collector figures plus every audit finding — for `/metrics`.
//!
//! Nothing is persisted and no alerting policy lives here: Prometheus scrapes,
//! stores, and decides with its own rules how long a finding must hold before
//! it pages anyone.

use shared::alerts::AlertSeverity;
use shared::findings::{self, Check, Finding, HostSnapshot, Source};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::{Arc, RwLock};

/// Most targets exported per check.
///
/// The total per check is always exported in `code_monitor_findings`; beyond
/// this many, individual series are dropped so a host with a hundred
/// unlimited containers cannot flood the time-series database.
pub const MAX_TARGETS_PER_CHECK: usize = 25;

/// Longest label value exported; longer targets are truncated.
pub const MAX_LABEL_LEN: usize = 128;

/// One completed pass over the host.
#[derive(Debug, Clone)]
pub struct AuditReport {
    pub snapshot: HostSnapshot,
    pub findings: Vec<Finding>,
    /// Unix time at which the pass finished
    pub completed_at: i64,
    pub duration_seconds: f64,
}

impl AuditReport {
    pub fn new(
        snapshot: HostSnapshot,
        now: chrono::DateTime<chrono::Utc>,
        duration_seconds: f64,
    ) -> Self {
        let findings = findings::evaluate(&snapshot, now);
        Self {
            snapshot,
            findings,
            completed_at: now.timestamp(),
            duration_seconds,
        }
    }
}

/// The latest report, shared between the audit task and the HTTP handler.
pub type SharedAudit = Arc<RwLock<Option<AuditReport>>>;

pub fn new_shared() -> SharedAudit {
    Arc::new(RwLock::new(None))
}

/// Escape a label value as the exposition format requires, and bound it.
pub fn escape_label(value: &str) -> String {
    let bounded: String = value.chars().take(MAX_LABEL_LEN).collect();
    let mut out = String::with_capacity(bounded.len());
    for c in bounded.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

fn header(out: &mut String, name: &str, kind: &str, help: &str) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
}

fn sample(out: &mut String, name: &str, labels: &[(&str, &str)], value: f64) {
    out.push_str(name);
    if !labels.is_empty() {
        out.push('{');
        for (i, (key, val)) in labels.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(out, "{key}=\"{}\"", escape_label(val));
        }
        out.push('}');
    }
    if value.is_finite() {
        let _ = writeln!(out, " {value}");
    } else {
        let _ = writeln!(out, " NaN");
    }
}

/// Render the report in the Prometheus text exposition format.
///
/// Before the first pass completes only `code_monitor_audit_up 0` is written,
/// so a scrape right after a restart is distinguishable from a clean host.
pub fn render(report: Option<&AuditReport>, out: &mut String) {
    header(
        out,
        "code_monitor_audit_up",
        "gauge",
        "Whether at least one audit pass has completed",
    );
    let Some(report) = report else {
        sample(out, "code_monitor_audit_up", &[], 0.0);
        return;
    };
    sample(out, "code_monitor_audit_up", &[], 1.0);

    header(
        out,
        "code_monitor_audit_last_run_timestamp_seconds",
        "gauge",
        "Unix time the last audit pass completed",
    );
    sample(
        out,
        "code_monitor_audit_last_run_timestamp_seconds",
        &[],
        report.completed_at as f64,
    );
    header(
        out,
        "code_monitor_audit_duration_seconds",
        "gauge",
        "How long the last audit pass took",
    );
    sample(
        out,
        "code_monitor_audit_duration_seconds",
        &[],
        report.duration_seconds,
    );

    let snapshot = &report.snapshot;
    header(
        out,
        "code_monitor_audit_source_up",
        "gauge",
        "Whether the source was read in the last pass; checks of a source that is down are not evaluated",
    );
    for source in Source::ALL {
        sample(
            out,
            "code_monitor_audit_source_up",
            &[("source", source.as_str())],
            if snapshot.has(*source) { 1.0 } else { 0.0 },
        );
    }

    render_findings(report, out);
    render_collectors(snapshot, out);
}

fn render_findings(report: &AuditReport, out: &mut String) {
    let snapshot = &report.snapshot;

    let mut by_check: BTreeMap<Check, Vec<&Finding>> = BTreeMap::new();
    for finding in &report.findings {
        by_check.entry(finding.check).or_default().push(finding);
    }

    header(
        out,
        "code_monitor_findings",
        "gauge",
        "Number of findings per audit check (absent when the check's source could not be read)",
    );
    for check in Check::ALL {
        if !snapshot.has(check.source()) {
            continue;
        }
        let count = by_check.get(check).map_or(0, Vec::len);
        sample(
            out,
            "code_monitor_findings",
            &[("check", check.as_str())],
            count as f64,
        );
    }

    header(
        out,
        "code_monitor_finding",
        "gauge",
        "An audit finding that holds now; always 1, see code_monitor_finding_value for the figure behind it",
    );
    let mut exported: Vec<&Finding> = Vec::new();
    for findings in by_check.values() {
        // Worst first, so truncation drops the least urgent targets
        let mut ordered = findings.clone();
        ordered.sort_by_key(|f| std::cmp::Reverse(severity_rank(f.severity)));
        exported.extend(ordered.into_iter().take(MAX_TARGETS_PER_CHECK));
    }
    for finding in &exported {
        let severity = finding.severity.to_string();
        sample(
            out,
            "code_monitor_finding",
            &[
                ("check", finding.check.as_str()),
                ("severity", &severity),
                ("target", &finding.target),
            ],
            1.0,
        );
    }

    header(
        out,
        "code_monitor_finding_value",
        "gauge",
        "The figure behind each finding: days left, ratio, pages/s, bytes or count, depending on the check",
    );
    for finding in &exported {
        sample(
            out,
            "code_monitor_finding_value",
            &[
                ("check", finding.check.as_str()),
                ("target", &finding.target),
            ],
            finding.value,
        );
    }
}

/// The latest report as JSON: every finding with its detail and guidance.
///
/// `{"audit_up": false}` before the first pass completes.
pub fn findings_json(report: Option<&AuditReport>) -> serde_json::Value {
    let Some(report) = report else {
        return serde_json::json!({ "audit_up": false, "findings": [] });
    };
    let findings: Vec<serde_json::Value> = report
        .findings
        .iter()
        .map(|f| {
            serde_json::json!({
                "check": f.check.as_str(),
                "severity": f.severity.to_string(),
                "target": f.target,
                "value": if f.value.is_finite() { serde_json::json!(f.value) } else { serde_json::Value::Null },
                "detail": f.detail,
                "guidance": f.guidance,
            })
        })
        .collect();
    serde_json::json!({
        "audit_up": true,
        "completed_at": report.completed_at,
        "scheduled_jobs": report.snapshot.scheduled_jobs,
        "findings": findings,
    })
}

fn severity_rank(severity: AlertSeverity) -> u8 {
    match severity {
        AlertSeverity::Info => 0,
        AlertSeverity::Warning => 1,
        AlertSeverity::Critical => 2,
    }
}

fn render_collectors(snapshot: &HostSnapshot, out: &mut String) {
    if let Some(jobs) = &snapshot.scheduled_jobs {
        header(
            out,
            "code_monitor_scheduled_jobs_configured",
            "gauge",
            "Explicitly monitored scheduled jobs",
        );
        sample(
            out,
            "code_monitor_scheduled_jobs_configured",
            &[],
            jobs.len() as f64,
        );
        let fields = [
            ("collection_up", "Whether the job evidence was read"),
            ("timer_active", "Whether the timer is active"),
            ("running", "Whether the service is currently executing"),
            (
                "last_run_timestamp_seconds",
                "Last attempt, zero means unknown",
            ),
            (
                "last_success_timestamp_seconds",
                "Last successful completion, zero means unknown",
            ),
            (
                "next_run_timestamp_seconds",
                "Next scheduled run, zero means unknown",
            ),
            ("last_exit_code", "Last exit status"),
            (
                "max_interval_seconds",
                "Maximum allowed time between attempts",
            ),
        ];
        for (suffix, help) in fields {
            let name = format!("code_monitor_scheduled_job_{suffix}");
            header(out, &name, "gauge", help);
            for job in jobs {
                if !job.collection_up && suffix != "collection_up" {
                    continue;
                }
                let value = match suffix {
                    "collection_up" => u8::from(job.collection_up) as f64,
                    "timer_active" => u8::from(job.timer_active) as f64,
                    "running" => u8::from(job.running) as f64,
                    "last_run_timestamp_seconds" => job.last_run as f64,
                    "last_success_timestamp_seconds" => job.last_success as f64,
                    "next_run_timestamp_seconds" => job.next_run as f64,
                    "last_exit_code" => job.exit_code as f64,
                    _ => job.max_interval_seconds as f64,
                };
                sample(out, &name, &[("job_name", &job.name)], value);
            }
        }
    }

    if let Some(system) = &snapshot.system {
        header(
            out,
            "code_monitor_cpu_usage_percent",
            "gauge",
            "Global CPU usage",
        );
        sample(
            out,
            "code_monitor_cpu_usage_percent",
            &[],
            system.cpu_usage_percent,
        );

        header(
            out,
            "code_monitor_memory_bytes",
            "gauge",
            "Physical memory by kind",
        );
        for (kind, value) in [
            ("total", system.memory_total_bytes),
            ("used", system.memory_used_bytes),
            ("available", system.memory_available_bytes),
        ] {
            sample(
                out,
                "code_monitor_memory_bytes",
                &[("kind", kind)],
                value as f64,
            );
        }

        header(out, "code_monitor_swap_bytes", "gauge", "Swap by kind");
        for (kind, value) in [
            ("total", system.swap.total_bytes),
            ("used", system.swap.used_bytes),
        ] {
            sample(
                out,
                "code_monitor_swap_bytes",
                &[("kind", kind)],
                value as f64,
            );
        }

        header(
            out,
            "code_monitor_swap_pages_per_second",
            "gauge",
            "Paging rate by direction, the signal of memory pressure",
        );
        for (direction, value) in [
            ("in", system.swap.in_pages_per_sec),
            ("out", system.swap.out_pages_per_sec),
        ] {
            sample(
                out,
                "code_monitor_swap_pages_per_second",
                &[("direction", direction)],
                value,
            );
        }

        if let Some(pressure) = &system.memory_pressure {
            header(
                out,
                "code_monitor_memory_pressure_avg60_percent",
                "gauge",
                "Share of the last minute stalled on memory (PSI)",
            );
            for (kind, value) in [("some", pressure.some_avg60), ("full", pressure.full_avg60)] {
                sample(
                    out,
                    "code_monitor_memory_pressure_avg60_percent",
                    &[("kind", kind)],
                    value,
                );
            }
        }

        let disks: Vec<_> = system
            .disk_info
            .iter()
            .filter(|d| shared::operations::real_disk(d))
            .filter(|d| d.total_bytes > 0)
            .collect();
        if !disks.is_empty() {
            header(
                out,
                "code_monitor_disk_usage_percent",
                "gauge",
                "Disk usage per mount point",
            );
            for disk in &disks {
                sample(
                    out,
                    "code_monitor_disk_usage_percent",
                    &[("mount", &disk.mount_point)],
                    disk.usage_percent,
                );
            }
            header(
                out,
                "code_monitor_disk_available_bytes",
                "gauge",
                "Free disk space per mount point",
            );
            for disk in &disks {
                sample(
                    out,
                    "code_monitor_disk_available_bytes",
                    &[("mount", &disk.mount_point)],
                    disk.available_bytes as f64,
                );
            }
        }

        if let Some(commitment) = snapshot.memory_commitment() {
            header(
                out,
                "code_monitor_memory_commitment_ratio",
                "gauge",
                "Memory promised by configuration (postgres, container limits, JVM heaps) over physical memory",
            );
            sample(
                out,
                "code_monitor_memory_commitment_ratio",
                &[],
                commitment.ratio(),
            );
        }
    }

    if let Some(containers) = &snapshot.containers {
        let mut by_state: BTreeMap<&str, usize> = BTreeMap::new();
        for c in containers {
            *by_state.entry(c.state.as_str()).or_default() += 1;
        }
        header(
            out,
            "code_monitor_containers",
            "gauge",
            "Containers by state",
        );
        for (state, count) in &by_state {
            sample(
                out,
                "code_monitor_containers",
                &[("state", state)],
                *count as f64,
            );
        }

        header(
            out,
            "code_monitor_container_restarts_total",
            "counter",
            "Restarts reported by the container engine; alert on increase(), never on the total",
        );
        for c in containers {
            sample(
                out,
                "code_monitor_container_restarts_total",
                &[("container", &c.name)],
                c.restart_count as f64,
            );
        }
    }

    if let Some(failed) = &snapshot.systemd_failed {
        header(
            out,
            "code_monitor_systemd_failed_units",
            "gauge",
            "systemd units in the failed state",
        );
        sample(
            out,
            "code_monitor_systemd_failed_units",
            &[],
            failed.len() as f64,
        );
    }

    if let Some(tls) = &snapshot.tls {
        header(
            out,
            "code_monitor_tls_certificate_expiry_days",
            "gauge",
            "Days until each certificate expires; negative once expired",
        );
        // A name can appear twice: an orphaned lineage on disk and the
        // certificate actually served for it. `source` keeps the series
        // distinct and `orphaned` lets a rule look only at what is in use.
        let mut seen = std::collections::BTreeSet::new();
        for cert in &tls.certificates {
            if !seen.insert((cert.name.as_str(), cert.source.as_str())) {
                continue;
            }
            let orphaned = if cert.orphaned { "true" } else { "false" };
            sample(
                out,
                "code_monitor_tls_certificate_expiry_days",
                &[
                    ("certificate", &cert.name),
                    ("source", &cert.source),
                    ("orphaned", orphaned),
                ],
                cert.days_until_expiry as f64,
            );
        }
    }

    if let Some(ports) = &snapshot.ports {
        let mut by_scope: BTreeMap<String, usize> = BTreeMap::new();
        for p in ports {
            let scope = format!("{:?}", p.bind_scope).to_lowercase();
            *by_scope.entry(scope).or_default() += 1;
        }
        header(
            out,
            "code_monitor_listening_ports",
            "gauge",
            "Listening sockets by bind scope",
        );
        for (scope, count) in &by_scope {
            sample(
                out,
                "code_monitor_listening_ports",
                &[("scope", scope)],
                *count as f64,
            );
        }
    }

    if let Some(clusters) = &snapshot.postgres {
        header(
            out,
            "code_monitor_postgres_connections",
            "gauge",
            "Open connections per postgres cluster",
        );
        for c in clusters {
            sample(
                out,
                "code_monitor_postgres_connections",
                &[("cluster", &c.name)],
                c.connections_total as f64,
            );
        }
    }

    if let Some(clusters) = &snapshot.mariadb {
        header(
            out,
            "code_monitor_mariadb_connections",
            "gauge",
            "Active connections per MariaDB instance",
        );
        for c in clusters {
            sample(
                out,
                "code_monitor_mariadb_connections",
                &[("cluster", &c.name)],
                c.connections_active as f64,
            );
        }
    }
}

/// Collect a snapshot from the live monitor.
///
/// Each source is attempted independently: one failing collector costs only
/// its own checks.
#[cfg(not(all(test, feature = "unit-doubles")))]
pub async fn collect(
    monitor: &crate::monitor::SystemMonitor,
    firewall_snapshot_dir: &std::path::Path,
    disk_policy: &shared::operations::DiskPolicy,
    jobs: &[shared::operations::ScheduledJobConfig],
) -> HostSnapshot {
    use tracing::warn;

    fn keep<T>(source: &str, result: anyhow::Result<T>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(e) => {
                warn!("audit: {} unavailable: {}", source, e);
                None
            }
        }
    }

    let processes = monitor
        .get_processes(0, None)
        .map(|procs| {
            procs
                .into_iter()
                .map(|p| (p.name, p.command_line))
                .collect()
        })
        .unwrap_or_default();

    let systemd_failed = keep("systemd", monitor.get_systemd_failed_units().await);
    // networkctl is only worth a fork when there is a wait-online failure to judge
    let network = systemd_failed
        .as_ref()
        .filter(|units| {
            units
                .iter()
                .any(|u| shared::network::is_wait_online_unit(&u.name))
        })
        .and_then(|_| read_network_state());

    HostSnapshot {
        system: keep("system", monitor.get_system_info()),
        disk_policy: disk_policy.clone(),
        scheduled_jobs: Some(crate::scheduled::collect(jobs).await),
        // An unreachable daemon returns an empty list, not an error: report
        // the source as down rather than as a host with no containers.
        containers: keep("docker", monitor.get_containers().await)
            .filter(|_| monitor.docker_reachable()),
        processes,
        postgres: keep("postgres", monitor.get_postgres_clusters().await),
        mariadb: keep("mariadb", monitor.get_mariadb_clusters().await),
        systemd_failed,
        tls: keep("tls", monitor.get_tls_info().await),
        ports: keep("ports", monitor.get_listening_ports().await),
        firewall: Some(read_firewall(firewall_snapshot_dir)),
        network,
    }
}

/// Where the iptables tools live on the distributions this agent targets;
/// a systemd service's PATH does not always include `/usr/sbin`.
#[cfg(not(all(test, feature = "unit-doubles")))]
const SBIN_DIRS: &[&str] = &["/usr/sbin", "/sbin", "/usr/bin", "/bin"];

/// Run a command and return its stdout, or `None` if it is missing or fails.
#[cfg(not(all(test, feature = "unit-doubles")))]
fn run_stdout(program: &str, args: &[&str]) -> Option<String> {
    let path = SBIN_DIRS
        .iter()
        .map(|dir| std::path::Path::new(dir).join(program))
        .find(|p| p.exists())?;
    match std::process::Command::new(&path).args(args).output() {
        Ok(out) if out.status.success() => Some(String::from_utf8_lossy(&out.stdout).into_owned()),
        Ok(out) => {
            tracing::debug!(
                "audit: {} failed: {}",
                program,
                String::from_utf8_lossy(&out.stderr).trim()
            );
            None
        }
        Err(e) => {
            tracing::debug!("audit: cannot run {}: {}", program, e);
            None
        }
    }
}

/// The v4 and v6 rule sets from the snapshot files under `dir`.
///
/// The agent holds no `CAP_NET_ADMIN`: `code-monitor-firewall-snapshot.timer`
/// saves `iptables-save` / `ip6tables-save` there every minute. A family whose
/// file is missing or older than `SNAPSHOT_MAX_AGE` is read with `fallback`
/// (given the program name) instead; if either family is still unavailable
/// both are `None`, so `source="firewall"` goes to 0 and every exposed port
/// keeps the unverified warning.
fn firewall_rules(
    dir: &std::path::Path,
    now: std::time::SystemTime,
    fallback: impl Fn(&str) -> Option<String>,
) -> (
    Option<shared::firewall::Ruleset>,
    Option<shared::firewall::Ruleset>,
) {
    use shared::firewall::*;
    let family = |file: &str, program: &str| {
        let path = dir.join(file);
        match read_rules_file(&path, now, SNAPSHOT_MAX_AGE) {
            Ok(rules) => Some(rules),
            Err(e) => {
                tracing::debug!("audit: firewall snapshot {}: {}", path.display(), e);
                fallback(program).map(|t| parse_iptables_save(&t))
            }
        }
    };
    match (
        family(SNAPSHOT_V4_FILE, "iptables-save"),
        family(SNAPSHOT_V6_FILE, "ip6tables-save"),
    ) {
        (Some(v4), Some(v6)) => (Some(v4), Some(v6)),
        // Both files come from the same timer run: one missing or stale means
        // the snapshot is broken, so the whole source is down
        _ => (None, None),
    }
}

/// Read the packet filter from the timer's snapshot files.
///
/// Only when the agent itself runs as root does it fall back to asking the
/// kernel directly; no capability is ever required.
#[cfg(not(all(test, feature = "unit-doubles")))]
fn read_firewall(dir: &std::path::Path) -> shared::firewall::FirewallSnapshot {
    use shared::firewall::*;
    let read = |file: &str| std::fs::read_to_string(file).unwrap_or_default();
    let (v4, v6) = firewall_rules(dir, std::time::SystemTime::now(), |program| {
        running_as_root()
            .then(|| run_stdout(program, &[]))
            .flatten()
    });
    let snapshot = FirewallSnapshot {
        v4,
        v6,
        public_ifaces_v4: parse_ipv4_default_ifaces(&read("/proc/net/route")),
        public_ifaces_v6: parse_ipv6_default_ifaces(&read("/proc/net/ipv6_route")),
    };
    if !snapshot.complete() {
        tracing::debug!(
            "audit: firewall unavailable (is code-monitor-firewall-snapshot.timer active?)"
        );
    }
    snapshot
}

#[cfg(not(all(test, feature = "unit-doubles")))]
fn running_as_root() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail
    unsafe { libc::geteuid() == 0 }
}

/// What networkd reports now, or `None` without networkctl.
#[cfg(not(all(test, feature = "unit-doubles")))]
fn read_network_state() -> Option<shared::network::NetworkState> {
    use shared::network::*;
    let list = run_stdout("networkctl", &["list", "--no-pager", "--no-legend"])?;
    let status = run_stdout("networkctl", &["status", "--no-pager"]).unwrap_or_default();
    Some(NetworkState {
        online_state: parse_online_state(&status),
        links: parse_networkctl_list(&list),
    })
}

/// Run the audit every `interval`, publishing each report to `shared`.
#[cfg(not(all(test, feature = "unit-doubles")))]
pub fn spawn(
    monitor: Arc<crate::monitor::SystemMonitor>,
    interval: std::time::Duration,
    firewall_snapshot_dir: std::path::PathBuf,
    disk_policy: shared::operations::DiskPolicy,
    jobs: Vec<shared::operations::ScheduledJobConfig>,
    shared: SharedAudit,
) {
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(interval);
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            timer.tick().await;
            let started = std::time::Instant::now();
            let snapshot = collect(&monitor, &firewall_snapshot_dir, &disk_policy, &jobs).await;
            let report = AuditReport::new(
                snapshot,
                chrono::Utc::now(),
                started.elapsed().as_secs_f64(),
            );
            tracing::debug!("audit pass: {} finding(s)", report.findings.len());
            match shared.write() {
                Ok(mut slot) => *slot = Some(report),
                Err(e) => tracing::error!("audit: failed to publish report: {}", e),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::types::*;

    fn now() -> chrono::DateTime<chrono::Utc> {
        "2026-10-29T12:00:00Z".parse().unwrap()
    }

    fn port(addr: &str, port: u16) -> ListeningPortInfo {
        ListeningPortInfo {
            address: addr.into(),
            port,
            protocol: "tcp".into(),
            bind_scope: BindScope::classify(addr),
            pid: None,
            process_name: "db".into(),
            sensitive: ListeningPortInfo::is_sensitive_port(port),
        }
    }

    fn rendered(report: Option<&AuditReport>) -> String {
        let mut out = String::new();
        render(report, &mut out);
        out
    }

    #[test]
    fn scheduled_metrics_do_not_publish_fake_success_when_evidence_is_missing() {
        let report = AuditReport::new(
            HostSnapshot {
                scheduled_jobs: Some(vec![shared::operations::ScheduledJob {
                    name: "backup".into(),
                    result: "collection-unavailable".into(),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            now(),
            0.1,
        );
        let out = rendered(Some(&report));
        assert!(out.contains("code_monitor_scheduled_job_collection_up{job_name=\"backup\"} 0"));
        assert!(!out.contains("code_monitor_scheduled_job_last_exit_code{job_name="));
        assert!(out.contains("scheduled_job_unavailable"));
        assert_eq!(
            findings_json(Some(&report))["scheduled_jobs"][0]["collection_up"],
            false
        );
    }

    #[test]
    fn before_first_pass_only_reports_down() {
        let out = rendered(None);
        assert!(out.contains("code_monitor_audit_up 0\n"));
        assert!(!out.contains("code_monitor_finding"));
    }

    #[test]
    fn findings_become_labelled_series() {
        let snapshot = HostSnapshot {
            ports: Some(vec![port("0.0.0.0", 5432), port("127.0.0.1", 6379)]),
            systemd_failed: Some(vec![SystemdFailedUnit {
                name: "backup.service".into(),
                description: String::new(),
                since: None,
            }]),
            ..Default::default()
        };
        let report = AuditReport::new(snapshot, now(), 0.5);
        let out = rendered(Some(&report));

        assert!(out.contains("code_monitor_audit_up 1\n"));
        assert!(out.contains(
            "code_monitor_finding{check=\"datastore_exposed\",severity=\"warning\",target=\"0.0.0.0:5432\"} 1\n"
        ));
        assert!(out.contains(
            "code_monitor_finding{check=\"systemd_unit_failed\",severity=\"warning\",target=\"backup.service\"} 1\n"
        ));
        assert!(out.contains("code_monitor_findings{check=\"datastore_exposed\"} 1\n"));
        assert!(out.contains("code_monitor_findings{check=\"systemd_unit_failed\"} 1\n"));
        // Sources that were not read get no count at all, not a false zero
        assert!(!out.contains("code_monitor_findings{check=\"image_drift\"}"));
        assert!(out.contains("code_monitor_audit_source_up{source=\"docker\"} 0\n"));
        assert!(out.contains("code_monitor_audit_source_up{source=\"ports\"} 1\n"));
        assert!(out.contains("code_monitor_listening_ports{scope=\"allinterfaces\"} 1\n"));
    }

    #[test]
    fn targets_per_check_are_capped_but_counted() {
        let units: Vec<SystemdFailedUnit> = (0..(MAX_TARGETS_PER_CHECK + 10))
            .map(|i| SystemdFailedUnit {
                name: format!("u{i}.service"),
                description: String::new(),
                since: None,
            })
            .collect();
        let report = AuditReport::new(
            HostSnapshot {
                systemd_failed: Some(units),
                ..Default::default()
            },
            now(),
            0.0,
        );
        let out = rendered(Some(&report));
        let series = out
            .lines()
            .filter(|l| l.starts_with("code_monitor_finding{"))
            .count();
        assert_eq!(series, MAX_TARGETS_PER_CHECK);
        assert!(out.contains(&format!(
            "code_monitor_findings{{check=\"systemd_unit_failed\"}} {}\n",
            MAX_TARGETS_PER_CHECK + 10
        )));
    }

    #[test]
    fn firewall_verdict_keeps_the_label_set() {
        let closed = "*filter\n:INPUT DROP [0:0]\n-A INPUT -s 10.0.0.0/8 -j ACCEPT\nCOMMIT\n";
        let firewall = shared::firewall::FirewallSnapshot {
            v4: Some(shared::firewall::parse_iptables_save(closed)),
            v6: Some(shared::firewall::parse_iptables_save(closed)),
            public_ifaces_v4: vec!["eth0".into()],
            public_ifaces_v6: vec!["eth0".into()],
        };
        let report = AuditReport::new(
            HostSnapshot {
                ports: Some(vec![port("0.0.0.0", 5432)]),
                firewall: Some(firewall),
                ..Default::default()
            },
            now(),
            0.1,
        );
        let out = rendered(Some(&report));
        // Same three labels as every other finding; info never matches the
        // warning|critical alert rule
        assert!(out.contains(
            "code_monitor_finding{check=\"datastore_bound_all_interfaces\",severity=\"info\",target=\"0.0.0.0:5432\"} 1\n"
        ));
        assert!(out.contains("code_monitor_findings{check=\"datastore_exposed\"} 0\n"));
        assert!(out.contains("code_monitor_audit_source_up{source=\"firewall\"} 1\n"));

        let json = findings_json(Some(&report));
        assert_eq!(json["audit_up"], true);
        let finding = &json["findings"][0];
        assert_eq!(finding["check"], "datastore_bound_all_interfaces");
        assert_eq!(finding["severity"], "info");
        assert!(finding["detail"]
            .as_str()
            .unwrap()
            .contains("firewall drops"));
        assert!(finding["guidance"]["why_pt_br"].as_str().is_some());
        assert!(finding["guidance"]["fix_en"].as_str().is_some());
        assert_eq!(findings_json(None)["audit_up"], false);
    }

    /// Render a report whose firewall comes from snapshot files in `dir`.
    fn rendered_from_snapshot_dir(
        dir: &std::path::Path,
        fallback: impl Fn(&str) -> Option<String>,
    ) -> String {
        let (v4, v6) = firewall_rules(dir, std::time::SystemTime::now(), fallback);
        let report = AuditReport::new(
            HostSnapshot {
                ports: Some(vec![port("0.0.0.0", 5432)]),
                firewall: Some(shared::firewall::FirewallSnapshot {
                    v4,
                    v6,
                    public_ifaces_v4: vec!["eth0".into()],
                    public_ifaces_v6: vec!["eth0".into()],
                }),
                ..Default::default()
            },
            now(),
            0.1,
        );
        rendered(Some(&report))
    }

    fn write_snapshot(dir: &std::path::Path, file: &str, age: std::time::Duration) {
        let closed = "*filter\n:INPUT DROP [0:0]\n-A INPUT -s 10.0.0.0/8 -j ACCEPT\nCOMMIT\n";
        let path = dir.join(file);
        std::fs::write(&path, closed).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - age)
            .unwrap();
    }

    const UNVERIFIED: &str =
        "code_monitor_finding{check=\"datastore_exposed\",severity=\"warning\",target=\"0.0.0.0:5432\"} 1\n";
    const FIREWALL_UP: &str = "code_monitor_audit_source_up{source=\"firewall\"} 1\n";
    const FIREWALL_DOWN: &str = "code_monitor_audit_source_up{source=\"firewall\"} 0\n";

    #[test]
    fn fresh_snapshot_files_bring_the_firewall_source_up() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = std::time::Duration::from_secs(30);
        write_snapshot(dir.path(), shared::firewall::SNAPSHOT_V4_FILE, fresh);
        write_snapshot(dir.path(), shared::firewall::SNAPSHOT_V6_FILE, fresh);
        let out = rendered_from_snapshot_dir(dir.path(), |_| None);
        assert!(out.contains(FIREWALL_UP), "{out}");
        assert!(out.contains(
            "code_monitor_finding{check=\"datastore_bound_all_interfaces\",severity=\"info\",target=\"0.0.0.0:5432\"} 1\n"
        ));
        assert!(!out.contains(UNVERIFIED));
    }

    #[test]
    fn stale_snapshot_takes_the_firewall_source_down() {
        let dir = tempfile::tempdir().unwrap();
        write_snapshot(
            dir.path(),
            shared::firewall::SNAPSHOT_V4_FILE,
            std::time::Duration::from_secs(30),
        );
        write_snapshot(
            dir.path(),
            shared::firewall::SNAPSHOT_V6_FILE,
            std::time::Duration::from_secs(6 * 60),
        );
        let out = rendered_from_snapshot_dir(dir.path(), |_| None);
        assert!(out.contains(FIREWALL_DOWN), "{out}");
        // Back to the conservative verdict
        assert!(out.contains(UNVERIFIED), "{out}");
    }

    #[test]
    fn missing_snapshot_takes_the_firewall_source_down() {
        let dir = tempfile::tempdir().unwrap();
        let out = rendered_from_snapshot_dir(&dir.path().join("absent"), |_| None);
        assert!(out.contains(FIREWALL_DOWN), "{out}");
        assert!(out.contains(UNVERIFIED), "{out}");
    }

    #[test]
    fn missing_snapshot_falls_back_only_through_the_given_reader() {
        let dir = tempfile::tempdir().unwrap();
        write_snapshot(
            dir.path(),
            shared::firewall::SNAPSHOT_V4_FILE,
            std::time::Duration::ZERO,
        );
        let asked = std::cell::RefCell::new(Vec::new());
        let out = rendered_from_snapshot_dir(dir.path(), |program| {
            asked.borrow_mut().push(program.to_string());
            Some("*filter\n:INPUT DROP [0:0]\nCOMMIT\n".into())
        });
        // Only the family without a usable file goes to the fallback
        assert_eq!(asked.into_inner(), vec!["ip6tables-save"]);
        assert!(out.contains(FIREWALL_UP), "{out}");
    }

    #[test]
    fn certificate_expiry_series_stay_unique_when_a_name_is_both_orphaned_and_served() {
        let cert = |source: &str, days: i64, orphaned: bool| TlsCertificateInfo {
            name: "site.example".into(),
            domains: vec!["site.example".into()],
            issuer: "R11".into(),
            not_after: None,
            days_until_expiry: days,
            source: source.into(),
            orphaned,
        };
        let report = AuditReport::new(
            HostSnapshot {
                tls: Some(TlsSnapshot {
                    certificates: vec![
                        cert("/etc/letsencrypt/live/site.example/cert.pem", -30, true),
                        cert("127.0.0.1:443", 60, false),
                        cert("127.0.0.1:443", 60, false),
                    ],
                    renewal_unit_name: None,
                    renewal_unit_status: None,
                }),
                ..Default::default()
            },
            now(),
            0.0,
        );
        let out = rendered(Some(&report));
        let series: Vec<&str> = out
            .lines()
            .filter(|l| l.starts_with("code_monitor_tls_certificate_expiry_days{"))
            .collect();
        assert_eq!(
            series,
            vec![
                "code_monitor_tls_certificate_expiry_days{certificate=\"site.example\",source=\"/etc/letsencrypt/live/site.example/cert.pem\",orphaned=\"true\"} -30",
                "code_monitor_tls_certificate_expiry_days{certificate=\"site.example\",source=\"127.0.0.1:443\",orphaned=\"false\"} 60",
            ]
        );
    }

    #[test]
    fn label_values_are_escaped_and_bounded() {
        assert_eq!(escape_label("a\"b\\c\nd\te"), "a\\\"b\\\\c\\nde");
        assert_eq!(escape_label(&"x".repeat(500)).len(), MAX_LABEL_LEN);
    }

    #[test]
    fn output_is_valid_exposition_format() {
        let mut system = SystemInfo {
            hostname: "h".into(),
            os: "linux".into(),
            kernel_version: "6".into(),
            uptime_seconds: 1,
            cpu_count: 2,
            cpu_usage_percent: 12.5,
            memory_total_bytes: 1 << 30,
            memory_used_bytes: 1 << 29,
            memory_available_bytes: 1 << 29,
            disk_info: vec![
                DiskInfo {
                    device: "/dev/sda1".into(),
                    mount_point: "/".into(),
                    filesystem_type: "ext4".into(),
                    total_bytes: 100,
                    used_bytes: 50,
                    available_bytes: 50,
                    usage_percent: 50.0,
                },
                DiskInfo {
                    device: "overlay".into(),
                    mount_point: "/var/lib/docker/overlay2/x/merged".into(),
                    filesystem_type: "overlay".into(),
                    total_bytes: 100,
                    used_bytes: 50,
                    available_bytes: 50,
                    usage_percent: 50.0,
                },
            ],
            timestamp: now(),
            swap: SwapInfo::default(),
            memory_pressure: Some(MemoryPressure {
                some_avg60: 1.0,
                full_avg60: 0.5,
                full_total_seconds: 3.0,
            }),
        };
        system.swap.total_bytes = 1 << 30;
        let report = AuditReport::new(
            HostSnapshot {
                system: Some(system),
                containers: Some(vec![]),
                postgres: Some(vec![]),
                mariadb: Some(vec![]),
                systemd_failed: Some(vec![]),
                tls: Some(TlsSnapshot::default()),
                ports: Some(vec![]),
                processes: vec![],
                firewall: None,
                network: None,
                disk_policy: Default::default(),
                scheduled_jobs: Some(vec![]),
            },
            now(),
            1.25,
        );
        let out = rendered(Some(&report));
        assert!(out.contains("code_monitor_disk_usage_percent{mount=\"/\"} 50\n"));
        assert!(!out.contains("overlay2"));
        assert!(out.contains("code_monitor_memory_commitment_ratio 0\n"));
        // Every check is evaluated, and on this clean host each counts zero
        for check in Check::ALL {
            assert!(
                out.contains(&format!("code_monitor_findings{{check=\"{check}\"}} 0\n")),
                "{check}"
            );
        }
        for line in out.lines() {
            if line.starts_with('#') {
                assert!(line.starts_with("# HELP ") || line.starts_with("# TYPE "));
                continue;
            }
            let (name, value) = line.rsplit_once(' ').expect(line);
            assert!(name.starts_with("code_monitor_"), "{line}");
            value.parse::<f64>().expect(line);
        }
    }
}
