//! Alert system for monitoring thresholds
//!
//! This module provides alert detection and notification for system metrics

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Type of alert
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AlertType {
    CpuHigh,
    MemoryHigh,
    DiskHigh,
    ServerDown,
    ProcessDown,
    SystemdUnitFailed,
    ContainerCrashLoop,
    ContainerHealthcheckBroken,
    TlsCertificateExpiring,
    TlsRenewalBroken,
    ExposedDatastore,
}

impl std::fmt::Display for AlertType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertType::CpuHigh => write!(f, "CPU_HIGH"),
            AlertType::MemoryHigh => write!(f, "MEMORY_HIGH"),
            AlertType::DiskHigh => write!(f, "DISK_HIGH"),
            AlertType::ServerDown => write!(f, "SERVER_DOWN"),
            AlertType::ProcessDown => write!(f, "PROCESS_DOWN"),
            AlertType::SystemdUnitFailed => write!(f, "SYSTEMD_UNIT_FAILED"),
            AlertType::ContainerCrashLoop => write!(f, "CONTAINER_CRASH_LOOP"),
            AlertType::ContainerHealthcheckBroken => write!(f, "CONTAINER_HEALTHCHECK_BROKEN"),
            AlertType::TlsCertificateExpiring => write!(f, "TLS_CERTIFICATE_EXPIRING"),
            AlertType::TlsRenewalBroken => write!(f, "TLS_RENEWAL_BROKEN"),
            AlertType::ExposedDatastore => write!(f, "EXPOSED_DATASTORE"),
        }
    }
}

/// Severity level of an alert
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

impl std::fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertSeverity::Info => write!(f, "info"),
            AlertSeverity::Warning => write!(f, "warning"),
            AlertSeverity::Critical => write!(f, "critical"),
        }
    }
}

/// An active or historical alert
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: uuid::Uuid,
    pub alert_type: AlertType,
    pub severity: AlertSeverity,
    pub server_id: String,
    pub server_name: String,
    pub message: String,
    pub value: Option<f64>,
    pub threshold: Option<f64>,
    pub triggered_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub acknowledged: bool,
    pub acknowledged_by: Option<String>,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

impl Alert {
    pub fn new(
        alert_type: AlertType,
        severity: AlertSeverity,
        server_id: String,
        server_name: String,
        message: String,
        value: Option<f64>,
        threshold: Option<f64>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            alert_type,
            severity,
            server_id,
            server_name,
            message,
            value,
            threshold,
            triggered_at: Utc::now(),
            resolved_at: None,
            acknowledged: false,
            acknowledged_by: None,
            acknowledged_at: None,
        }
    }

    pub fn resolve(&mut self) {
        self.resolved_at = Some(Utc::now());
    }

    pub fn acknowledge(&mut self, by: String) {
        self.acknowledged = true;
        self.acknowledged_by = Some(by);
        self.acknowledged_at = Some(Utc::now());
    }

    pub fn is_resolved(&self) -> bool {
        self.resolved_at.is_some()
    }

    pub fn duration(&self) -> Duration {
        let end = self.resolved_at.unwrap_or_else(Utc::now);
        end - self.triggered_at
    }
}

/// Configuration for an alert rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: String,
    pub name: String,
    pub alert_type: AlertType,
    pub severity: AlertSeverity,
    pub enabled: bool,
    pub threshold: f64,
    pub duration_seconds: u64, // Must exceed threshold for this duration
    pub servers: Vec<String>,  // Empty = all servers
    #[serde(skip)]
    pub channels: Vec<AlertChannel>,
}

/// Notification channel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertChannel {
    Webhook { url: String },
    Slack { webhook_url: String },
    Discord { webhook_url: String },
    Email { to: Vec<String> },
}

/// Alert state for tracking thresholds over time
#[derive(Debug)]
pub struct AlertState {
    samples: VecDeque<(DateTime<Utc>, f64)>,
    triggered: bool,
    last_triggered: Option<DateTime<Utc>>,
    silenced_until: Option<DateTime<Utc>>,
}

impl AlertState {
    pub fn new() -> Self {
        Self {
            samples: VecDeque::new(),
            triggered: false,
            last_triggered: None,
            silenced_until: None,
        }
    }

    pub fn add_sample(&mut self, value: f64, max_age: Duration) {
        let now = Utc::now();

        // Remove old samples
        while let Some((time, _)) = self.samples.front() {
            if now - *time > max_age {
                self.samples.pop_front();
            } else {
                break;
            }
        }

        self.samples.push_back((now, value));
    }

    pub fn check_threshold(&self, threshold: f64, min_samples: usize) -> bool {
        if self.samples.len() < min_samples {
            return false;
        }

        // Check if the most recent `min_samples` samples all exceed threshold
        self.samples
            .iter()
            .rev()
            .take(min_samples)
            .all(|(_, value)| *value > threshold)
    }

    pub fn mark_triggered(&mut self) {
        self.triggered = true;
        self.last_triggered = Some(Utc::now());
    }

    pub fn mark_resolved(&mut self) {
        self.triggered = false;
    }

    pub fn is_triggered(&self) -> bool {
        self.triggered
    }

    pub fn can_trigger_again(&self, cooldown: Duration) -> bool {
        match self.last_triggered {
            None => true,
            Some(last) => Utc::now() - last > cooldown,
        }
    }

    /// Silence this alert for the given duration: no new alerts are generated until it expires
    pub fn silence(&mut self, duration: Duration) {
        self.silenced_until = Some(Utc::now() + duration);
    }

    /// Remove an active silence
    pub fn unsilence(&mut self) {
        self.silenced_until = None;
    }

    pub fn is_silenced(&self) -> bool {
        match self.silenced_until {
            None => false,
            Some(until) => Utc::now() < until,
        }
    }
}

impl Default for AlertState {
    fn default() -> Self {
        Self::new()
    }
}

/// Restart count last seen for a container, and when
#[derive(Debug, Clone)]
struct RestartBaseline {
    count: u32,
    observed_at: DateTime<Utc>,
}

/// Alert manager that tracks state and generates alerts
#[derive(Debug)]
pub struct AlertManager {
    rules: Vec<AlertRule>,
    states: HashMap<(String, AlertType), AlertState>,
    active_alerts: Vec<Alert>,
    alert_history: Vec<Alert>,
    max_history: usize,
    /// Restart counts per (server, container), so restarts can be judged as a
    /// rate rather than a running total
    restart_baselines: HashMap<(String, String), RestartBaseline>,
}

impl AlertManager {
    /// Restarts within [`Self::RESTART_WINDOW_MINUTES`] that constitute a
    /// crash loop.
    pub const RESTART_RATE_THRESHOLD: u32 = 3;

    /// Window over which restarts are counted.
    pub const RESTART_WINDOW_MINUTES: i64 = 10;

    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            states: HashMap::new(),
            active_alerts: Vec::new(),
            alert_history: Vec::new(),
            max_history: 1000,
            restart_baselines: HashMap::new(),
        }
    }

    pub fn add_rule(&mut self, rule: AlertRule) {
        self.rules.push(rule);
    }

    pub fn remove_rule(&mut self, rule_id: &str) {
        self.rules.retain(|r| r.id != rule_id);
    }

    pub fn get_rules(&self) -> &[AlertRule] {
        &self.rules
    }

    /// Process metrics and generate alerts
    pub fn process_metrics(
        &mut self,
        server_id: &str,
        server_name: &str,
        cpu_usage: f64,
        memory_usage: f64,
        disk_usage: f64,
    ) -> Vec<Alert> {
        let mut new_alerts = Vec::new();
        let rules = self.rules.to_vec();

        for rule in rules {
            if !rule.enabled {
                continue;
            }

            // Check if rule applies to this server
            if !rule.servers.is_empty() && !rule.servers.contains(&server_id.to_string()) {
                continue;
            }

            let state_key = (server_id.to_string(), rule.alert_type);
            let state = self.states.entry(state_key.clone()).or_default();

            let (value, exceeded) = match rule.alert_type {
                AlertType::CpuHigh => {
                    state.add_sample(
                        cpu_usage,
                        Duration::seconds(rule.duration_seconds as i64 + 10),
                    );
                    (cpu_usage, cpu_usage > rule.threshold)
                }
                AlertType::MemoryHigh => {
                    state.add_sample(
                        memory_usage,
                        Duration::seconds(rule.duration_seconds as i64 + 10),
                    );
                    (memory_usage, memory_usage > rule.threshold)
                }
                AlertType::DiskHigh => {
                    state.add_sample(
                        disk_usage,
                        Duration::seconds(rule.duration_seconds as i64 + 10),
                    );
                    (disk_usage, disk_usage > rule.threshold)
                }
                _ => continue,
            };

            let min_samples = (rule.duration_seconds / 5).max(1) as usize; // Assuming 5s sample interval

            if exceeded && state.check_threshold(rule.threshold, min_samples) {
                if !state.is_triggered()
                    && state.can_trigger_again(Duration::minutes(5))
                    && !state.is_silenced()
                {
                    state.mark_triggered();

                    let alert = Alert::new(
                        rule.alert_type,
                        rule.severity,
                        server_id.to_string(),
                        server_name.to_string(),
                        format!(
                            "{} exceeded threshold: {:.1}% (threshold: {:.1}%)",
                            rule.alert_type, value, rule.threshold
                        ),
                        Some(value),
                        Some(rule.threshold),
                    );

                    self.active_alerts.push(alert.clone());
                    self.add_to_history(alert.clone());
                    new_alerts.push(alert);
                }
            } else if !exceeded && state.is_triggered() {
                state.mark_resolved();

                // Resolve any active alert of this type for this server
                if let Some(active) = self.active_alerts.iter_mut().find(|a| {
                    a.server_id == server_id && a.alert_type == rule.alert_type && !a.is_resolved()
                }) {
                    active.resolve();
                }
            }
        }

        new_alerts
    }

    /// Raise an alert for systemd units sitting in the `failed` state.
    ///
    /// Deliberately separate from [`Self::process_metrics`]: this is not a
    /// threshold over a sampling window. There is no legitimate steady state in
    /// which a unit stays failed, so a single observation is enough to alert,
    /// and the alert clears as soon as the count returns to zero.
    ///
    /// Returns the alert only when one is newly raised, so callers can dispatch
    /// notifications without re-notifying on every poll.
    pub fn process_systemd_failed_units(
        &mut self,
        server_id: &str,
        server_name: &str,
        failed_units: &[String],
    ) -> Option<Alert> {
        let state_key = (server_id.to_string(), AlertType::SystemdUnitFailed);
        let state = self.states.entry(state_key).or_default();

        if failed_units.is_empty() {
            if state.is_triggered() {
                state.mark_resolved();

                if let Some(active) = self.active_alerts.iter_mut().find(|a| {
                    a.server_id == server_id
                        && a.alert_type == AlertType::SystemdUnitFailed
                        && !a.is_resolved()
                }) {
                    active.resolve();
                }
            }
            return None;
        }

        if state.is_triggered()
            || state.is_silenced()
            || !state.can_trigger_again(Duration::minutes(5))
        {
            return None;
        }

        state.mark_triggered();

        let alert = Alert::new(
            AlertType::SystemdUnitFailed,
            AlertSeverity::Warning,
            server_id.to_string(),
            server_name.to_string(),
            format!(
                "{} systemd unit(s) in failed state: {}",
                failed_units.len(),
                failed_units.join(", ")
            ),
            Some(failed_units.len() as f64),
            Some(0.0),
        );

        self.active_alerts.push(alert.clone());
        self.add_to_history(alert.clone());

        Some(alert)
    }

    /// Days of remaining validity below which a certificate is a warning.
    ///
    /// Let's Encrypt renews at 30 days, so 21 means renewal has already had a
    /// week to work and has not.
    pub const TLS_WARNING_DAYS: i64 = 21;

    /// Days below which expiry becomes critical.
    pub const TLS_CRITICAL_DAYS: i64 = 7;

    /// Alert on certificates approaching expiry and on broken renewal.
    ///
    /// Takes `(name, days_until_expiry)` pairs plus the renewal unit's status.
    /// Both matter, and they fail independently: renewal can be dead for weeks
    /// while every certificate still looks fine, which is exactly the state
    /// three servers in the fleet were found in.
    pub fn process_tls_certificates(
        &mut self,
        server_id: &str,
        server_name: &str,
        certificates: &[(String, i64)],
        renewal_unit: Option<(&str, &str)>,
    ) -> Vec<Alert> {
        let mut new_alerts = Vec::new();

        // Worst certificate decides the severity, and its name leads the message
        let soonest = certificates.iter().min_by_key(|(_, days)| *days);

        if let Some((name, days)) = soonest {
            if *days <= Self::TLS_WARNING_DAYS {
                let severity = if *days <= Self::TLS_CRITICAL_DAYS {
                    AlertSeverity::Critical
                } else {
                    AlertSeverity::Warning
                };

                let expiring: Vec<&str> = certificates
                    .iter()
                    .filter(|(_, d)| *d <= Self::TLS_WARNING_DAYS)
                    .map(|(n, _)| n.as_str())
                    .collect();

                let message = if *days < 0 {
                    format!(
                        "certificate '{}' expired {} day(s) ago ({} affected)",
                        name,
                        -days,
                        expiring.len()
                    )
                } else {
                    format!(
                        "certificate '{}' expires in {} day(s) ({} of {} within {} days)",
                        name,
                        days,
                        expiring.len(),
                        certificates.len(),
                        Self::TLS_WARNING_DAYS
                    )
                };

                if let Some(alert) = self.raise_once(
                    server_id,
                    server_name,
                    AlertType::TlsCertificateExpiring,
                    severity,
                    message,
                    Some(*days as f64),
                    Some(Self::TLS_WARNING_DAYS as f64),
                    Duration::hours(12),
                ) {
                    new_alerts.push(alert);
                }
            } else {
                self.clear(server_id, AlertType::TlsCertificateExpiring);
            }
        }

        // A failed renewal unit is the leading indicator: certificates still
        // look healthy right up to the day a whole batch expires at once.
        match renewal_unit {
            Some((unit, status)) if status.contains("failed") => {
                if let Some(alert) = self.raise_once(
                    server_id,
                    server_name,
                    AlertType::TlsRenewalBroken,
                    AlertSeverity::Warning,
                    format!(
                        "{} is {}: {} certificate(s) will stop renewing",
                        unit,
                        status,
                        certificates.len()
                    ),
                    Some(certificates.len() as f64),
                    None,
                    Duration::hours(12),
                ) {
                    new_alerts.push(alert);
                }
            }
            _ => self.clear(server_id, AlertType::TlsRenewalBroken),
        }

        new_alerts
    }

    /// Raise an alert unless one of this type is already standing for the
    /// server, it is silenced, or the cooldown has not elapsed.
    #[allow(clippy::too_many_arguments)]
    fn raise_once(
        &mut self,
        server_id: &str,
        server_name: &str,
        alert_type: AlertType,
        severity: AlertSeverity,
        message: String,
        value: Option<f64>,
        threshold: Option<f64>,
        cooldown: Duration,
    ) -> Option<Alert> {
        let state = self
            .states
            .entry((server_id.to_string(), alert_type))
            .or_default();

        if state.is_triggered() || state.is_silenced() || !state.can_trigger_again(cooldown) {
            return None;
        }
        state.mark_triggered();

        let alert = Alert::new(
            alert_type,
            severity,
            server_id.to_string(),
            server_name.to_string(),
            message,
            value,
            threshold,
        );

        self.active_alerts.push(alert.clone());
        self.add_to_history(alert.clone());

        Some(alert)
    }

    /// Resolve a standing alert of this type, if there is one.
    fn clear(&mut self, server_id: &str, alert_type: AlertType) {
        let state = self
            .states
            .entry((server_id.to_string(), alert_type))
            .or_default();

        if !state.is_triggered() {
            return;
        }
        state.mark_resolved();

        if let Some(active) = self
            .active_alerts
            .iter_mut()
            .find(|a| a.server_id == server_id && a.alert_type == alert_type && !a.is_resolved())
        {
            active.resolve();
        }
    }

    /// Alert on data stores reachable from outside the host.
    ///
    /// A database bound to every interface works exactly as well as one bound
    /// to loopback, which is why nobody notices: the service is fine, the
    /// exposure is silent. Raised as a warning rather than critical because
    /// nothing is broken — the risk is what could happen, not what is
    /// happening.
    pub fn process_exposed_datastores(
        &mut self,
        server_id: &str,
        server_name: &str,
        exposed: &[String],
    ) -> Option<Alert> {
        if exposed.is_empty() {
            self.clear(server_id, AlertType::ExposedDatastore);
            return None;
        }

        self.raise_once(
            server_id,
            server_name,
            AlertType::ExposedDatastore,
            AlertSeverity::Warning,
            format!(
                "{} data store port(s) reachable beyond this host: {}",
                exposed.len(),
                exposed.join(", ")
            ),
            Some(exposed.len() as f64),
            Some(0.0),
            Duration::hours(6),
        )
    }

    /// Alert on containers that are restarting repeatedly.
    ///
    /// Judged as a rate, never as a total: a container that has been up for a
    /// year legitimately accumulates restarts across reboots and deploys, and
    /// alerting on the total would fire once for history nobody can act on.
    /// What matters is restarting *now*.
    ///
    /// The first observation of a container only records a baseline, so
    /// connecting to a server does not produce a burst of alerts for restarts
    /// that happened months ago.
    pub fn process_container_restarts(
        &mut self,
        server_id: &str,
        server_name: &str,
        containers: &[(String, u32)],
    ) -> Vec<Alert> {
        let now = Utc::now();
        let window = Duration::minutes(Self::RESTART_WINDOW_MINUTES);
        let mut new_alerts = Vec::new();

        for (name, count) in containers {
            let key = (server_id.to_string(), name.clone());

            let baseline = match self.restart_baselines.get(&key) {
                Some(baseline) => baseline.clone(),
                None => {
                    // First sighting: record where it stands, judge from here on
                    self.restart_baselines.insert(
                        key,
                        RestartBaseline {
                            count: *count,
                            observed_at: now,
                        },
                    );
                    continue;
                }
            };

            // A lower count means the container was recreated; start over
            if *count < baseline.count || now - baseline.observed_at > window {
                self.restart_baselines.insert(
                    key,
                    RestartBaseline {
                        count: *count,
                        observed_at: now,
                    },
                );
                continue;
            }

            let restarts_in_window = count - baseline.count;
            if restarts_in_window < Self::RESTART_RATE_THRESHOLD {
                continue;
            }

            // Reset regardless of whether the alert is emitted, so a silenced
            // or cooled-down container does not accumulate a stale baseline
            self.restart_baselines.insert(
                key,
                RestartBaseline {
                    count: *count,
                    observed_at: now,
                },
            );

            let state_key = (
                format!("{}::{}", server_id, name),
                AlertType::ContainerCrashLoop,
            );
            let state = self.states.entry(state_key).or_default();

            if state.is_silenced() || !state.can_trigger_again(Duration::minutes(15)) {
                continue;
            }
            state.mark_triggered();

            let alert = Alert::new(
                AlertType::ContainerCrashLoop,
                AlertSeverity::Warning,
                server_id.to_string(),
                server_name.to_string(),
                format!(
                    "container '{}' restarted {} times in {} minutes (total {})",
                    name,
                    restarts_in_window,
                    Self::RESTART_WINDOW_MINUTES,
                    count
                ),
                Some(restarts_in_window as f64),
                Some(Self::RESTART_RATE_THRESHOLD as f64),
            );

            self.active_alerts.push(alert.clone());
            self.add_to_history(alert.clone());
            new_alerts.push(alert);
        }

        new_alerts
    }

    /// Report healthchecks that have never worked.
    ///
    /// Raised at info severity on purpose: a probe that cannot run is a
    /// configuration defect to schedule, not an outage to wake someone for. It
    /// still deserves reporting, because every permanently red container makes
    /// a genuine failure harder to see.
    pub fn process_broken_healthchecks(
        &mut self,
        server_id: &str,
        server_name: &str,
        broken: &[String],
    ) -> Option<Alert> {
        let state_key = (server_id.to_string(), AlertType::ContainerHealthcheckBroken);
        let state = self.states.entry(state_key).or_default();

        if broken.is_empty() {
            if state.is_triggered() {
                state.mark_resolved();

                if let Some(active) = self.active_alerts.iter_mut().find(|a| {
                    a.server_id == server_id
                        && a.alert_type == AlertType::ContainerHealthcheckBroken
                        && !a.is_resolved()
                }) {
                    active.resolve();
                }
            }
            return None;
        }

        if state.is_triggered()
            || state.is_silenced()
            || !state.can_trigger_again(Duration::hours(6))
        {
            return None;
        }
        state.mark_triggered();

        let alert = Alert::new(
            AlertType::ContainerHealthcheckBroken,
            AlertSeverity::Info,
            server_id.to_string(),
            server_name.to_string(),
            format!(
                "{} container(s) with a healthcheck that never ran: {}",
                broken.len(),
                broken.join(", ")
            ),
            Some(broken.len() as f64),
            Some(0.0),
        );

        self.active_alerts.push(alert.clone());
        self.add_to_history(alert.clone());

        Some(alert)
    }

    pub fn acknowledge_alert(&mut self, alert_id: uuid::Uuid, by: String) -> Option<&Alert> {
        if let Some(alert) = self.active_alerts.iter_mut().find(|a| a.id == alert_id) {
            alert.acknowledge(by);
            return Some(alert);
        }
        if let Some(alert) = self.alert_history.iter_mut().find(|a| a.id == alert_id) {
            alert.acknowledge(by);
            return Some(alert);
        }
        None
    }

    /// Silence alerts of a given type for a server for the given duration
    pub fn silence_alert(&mut self, server_id: &str, alert_type: AlertType, duration: Duration) {
        let state = self
            .states
            .entry((server_id.to_string(), alert_type))
            .or_default();
        state.silence(duration);
    }

    /// Remove an active silence for a server/alert type pair
    pub fn unsilence_alert(&mut self, server_id: &str, alert_type: AlertType) {
        if let Some(state) = self.states.get_mut(&(server_id.to_string(), alert_type)) {
            state.unsilence();
        }
    }

    /// Check whether alerts of a given type are currently silenced for a server
    pub fn is_alert_silenced(&self, server_id: &str, alert_type: AlertType) -> bool {
        self.states
            .get(&(server_id.to_string(), alert_type))
            .map(|s| s.is_silenced())
            .unwrap_or(false)
    }

    pub fn get_active_alerts(&self) -> &[Alert] {
        &self.active_alerts
    }

    pub fn get_alert_history(&self) -> &[Alert] {
        &self.alert_history
    }

    fn add_to_history(&mut self, alert: Alert) {
        self.alert_history.push(alert);

        // Trim history if needed
        if self.alert_history.len() > self.max_history {
            self.alert_history.remove(0);
        }
    }

    /// Clean up resolved alerts older than retention period
    pub fn cleanup(&mut self, retention: Duration) {
        let cutoff = Utc::now() - retention;

        self.active_alerts
            .retain(|a| !a.is_resolved() || a.triggered_at > cutoff);
        self.alert_history.retain(|a| a.triggered_at > cutoff);
    }
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_state_tracking() {
        let mut state = AlertState::new();

        // Add samples below threshold
        for _ in 0..5 {
            state.add_sample(50.0, Duration::seconds(60));
        }

        assert!(!state.check_threshold(80.0, 3));

        // Add samples above threshold
        for _ in 0..5 {
            state.add_sample(90.0, Duration::seconds(60));
        }

        assert!(state.check_threshold(80.0, 3));
    }

    #[test]
    fn test_alert_creation() {
        let alert = Alert::new(
            AlertType::CpuHigh,
            AlertSeverity::Warning,
            "server-1".to_string(),
            "Test Server".to_string(),
            "CPU high".to_string(),
            Some(95.0),
            Some(80.0),
        );

        assert_eq!(alert.alert_type, AlertType::CpuHigh);
        assert!(!alert.is_resolved());

        let mut alert = alert;
        alert.resolve();
        assert!(alert.is_resolved());
    }

    #[test]
    fn test_alert_acknowledge() {
        let mut alert = Alert::new(
            AlertType::MemoryHigh,
            AlertSeverity::Critical,
            "server-1".to_string(),
            "Test Server".to_string(),
            "Memory high".to_string(),
            Some(95.0),
            Some(90.0),
        );

        assert!(!alert.acknowledged);
        alert.acknowledge("admin".to_string());
        assert!(alert.acknowledged);
        assert_eq!(alert.acknowledged_by.as_ref().unwrap(), "admin");
        assert!(alert.acknowledged_at.is_some());
    }

    #[test]
    fn test_alert_duration() {
        let alert = Alert::new(
            AlertType::CpuHigh,
            AlertSeverity::Warning,
            "server-1".to_string(),
            "Test Server".to_string(),
            "CPU high".to_string(),
            None,
            None,
        );

        let duration = alert.duration();
        assert!(duration.num_seconds() >= 0);
    }

    #[test]
    fn test_alert_state_triggered_and_resolved() {
        let mut state = AlertState::new();
        assert!(!state.is_triggered());

        state.mark_triggered();
        assert!(state.is_triggered());

        state.mark_resolved();
        assert!(!state.is_triggered());
    }

    #[test]
    fn test_alert_state_can_trigger_again() {
        let mut state = AlertState::new();
        assert!(state.can_trigger_again(Duration::minutes(5)));

        state.mark_triggered();
        assert!(!state.can_trigger_again(Duration::minutes(5)));
        // But with zero cooldown it should allow
        assert!(state.can_trigger_again(Duration::seconds(0)));
    }

    #[test]
    fn test_alert_state_silence() {
        let mut state = AlertState::new();
        assert!(!state.is_silenced());

        state.silence(Duration::minutes(10));
        assert!(state.is_silenced());

        state.unsilence();
        assert!(!state.is_silenced());
    }

    #[test]
    fn test_alert_state_silence_expires() {
        let mut state = AlertState::new();
        // A silence in the past must not be active
        state.silenced_until = Some(Utc::now() - Duration::seconds(1));
        assert!(!state.is_silenced());
    }

    #[test]
    fn test_alert_manager_silence_alert() {
        let mut manager = AlertManager::new();
        assert!(!manager.is_alert_silenced("srv-1", AlertType::CpuHigh));

        manager.silence_alert("srv-1", AlertType::CpuHigh, Duration::minutes(30));
        assert!(manager.is_alert_silenced("srv-1", AlertType::CpuHigh));
        // Other servers/types are unaffected
        assert!(!manager.is_alert_silenced("srv-2", AlertType::CpuHigh));
        assert!(!manager.is_alert_silenced("srv-1", AlertType::MemoryHigh));

        manager.unsilence_alert("srv-1", AlertType::CpuHigh);
        assert!(!manager.is_alert_silenced("srv-1", AlertType::CpuHigh));
    }

    #[test]
    fn test_alert_manager_add_remove_rule() {
        let mut manager = AlertManager::new();
        assert_eq!(manager.get_rules().len(), 0);

        manager.add_rule(AlertRule {
            id: "cpu-high".to_string(),
            name: "CPU High".to_string(),
            alert_type: AlertType::CpuHigh,
            severity: AlertSeverity::Warning,
            enabled: true,
            threshold: 80.0,
            duration_seconds: 15,
            servers: vec![],
            channels: vec![],
        });
        assert_eq!(manager.get_rules().len(), 1);

        manager.remove_rule("cpu-high");
        assert_eq!(manager.get_rules().len(), 0);
    }

    #[test]
    fn test_alert_manager_process_metrics_disabled_rule() {
        let mut manager = AlertManager::new();
        manager.add_rule(AlertRule {
            id: "cpu-high".to_string(),
            name: "CPU High".to_string(),
            alert_type: AlertType::CpuHigh,
            severity: AlertSeverity::Warning,
            enabled: false,
            threshold: 80.0,
            duration_seconds: 15,
            servers: vec![],
            channels: vec![],
        });

        let alerts = manager.process_metrics("srv1", "Server 1", 90.0, 50.0, 30.0);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_alert_manager_process_metrics_server_filter() {
        let mut manager = AlertManager::new();
        manager.add_rule(AlertRule {
            id: "cpu-high".to_string(),
            name: "CPU High".to_string(),
            alert_type: AlertType::CpuHigh,
            severity: AlertSeverity::Warning,
            enabled: true,
            threshold: 80.0,
            duration_seconds: 0,
            servers: vec!["other-server".to_string()],
            channels: vec![],
        });

        let alerts = manager.process_metrics("srv1", "Server 1", 90.0, 50.0, 30.0);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_alert_manager_acknowledge() {
        let mut manager = AlertManager::new();
        let alert = Alert::new(
            AlertType::CpuHigh,
            AlertSeverity::Warning,
            "srv1".to_string(),
            "Server 1".to_string(),
            "CPU high".to_string(),
            Some(95.0),
            Some(80.0),
        );
        let alert_id = alert.id;
        manager.active_alerts.push(alert);

        let result = manager.acknowledge_alert(alert_id, "operator".to_string());
        assert!(result.is_some());
        assert!(result.unwrap().acknowledged);
        assert_eq!(
            manager.active_alerts[0].acknowledged_by.as_ref().unwrap(),
            "operator"
        );
    }

    #[test]
    fn test_alert_manager_acknowledge_not_found() {
        let mut manager = AlertManager::new();
        let result = manager.acknowledge_alert(uuid::Uuid::new_v4(), "operator".to_string());
        assert!(result.is_none());
    }

    #[test]
    fn test_alert_manager_cleanup() {
        let mut manager = AlertManager::new();
        let mut old_alert = Alert::new(
            AlertType::CpuHigh,
            AlertSeverity::Warning,
            "srv1".to_string(),
            "Server 1".to_string(),
            "Old CPU high".to_string(),
            Some(95.0),
            Some(80.0),
        );
        old_alert.resolve();
        // Manually set triggered_at to old time
        // We can't easily do this since triggered_at is set in constructor
        // So we just verify cleanup doesn't panic
        manager.active_alerts.push(old_alert);
        manager.cleanup(Duration::seconds(0));
        // All old alerts should be removed
    }

    #[test]
    fn test_alert_manager_history_limit() {
        let mut manager = AlertManager::new();
        for i in 0..1005 {
            let alert = Alert::new(
                AlertType::CpuHigh,
                AlertSeverity::Warning,
                "srv1".to_string(),
                "Server 1".to_string(),
                format!("Alert {}", i),
                Some(95.0),
                Some(80.0),
            );
            manager.add_to_history(alert);
        }
        assert_eq!(manager.get_alert_history().len(), 1000);
    }

    #[test]
    fn test_alert_type_display() {
        assert_eq!(format!("{}", AlertType::CpuHigh), "CPU_HIGH");
        assert_eq!(format!("{}", AlertType::MemoryHigh), "MEMORY_HIGH");
        assert_eq!(format!("{}", AlertType::DiskHigh), "DISK_HIGH");
        assert_eq!(format!("{}", AlertType::ServerDown), "SERVER_DOWN");
        assert_eq!(format!("{}", AlertType::ProcessDown), "PROCESS_DOWN");
        assert_eq!(
            format!("{}", AlertType::SystemdUnitFailed),
            "SYSTEMD_UNIT_FAILED"
        );
    }

    // ─────────────────────────────────────────
    // systemd failed units
    // ─────────────────────────────────────────

    // ─────────────────────────────────────────
    // Exposed data stores
    // ─────────────────────────────────────────

    #[test]
    fn test_exposed_datastore_alerts() {
        let mut manager = AlertManager::new();
        let alert = manager
            .process_exposed_datastores(
                "srv-1",
                "alemanha6",
                &["0.0.0.0:5432 (postgres)".to_string()],
            )
            .expect("an exposed database must alert");

        assert_eq!(alert.alert_type, AlertType::ExposedDatastore);
        assert_eq!(alert.severity, AlertSeverity::Warning);
        assert!(alert.message.contains("5432"));
    }

    #[test]
    fn test_no_alert_without_exposure() {
        let mut manager = AlertManager::new();
        assert!(manager
            .process_exposed_datastores("srv-1", "srv", &[])
            .is_none());
    }

    #[test]
    fn test_exposed_datastore_alert_does_not_repeat() {
        let mut manager = AlertManager::new();
        let exposed = vec!["0.0.0.0:5432".to_string()];

        assert!(manager
            .process_exposed_datastores("srv-1", "srv", &exposed)
            .is_some());
        assert!(manager
            .process_exposed_datastores("srv-1", "srv", &exposed)
            .is_none());
    }

    #[test]
    fn test_exposed_datastore_alert_resolves_after_rebinding() {
        let mut manager = AlertManager::new();
        manager.process_exposed_datastores("srv-1", "srv", &["0.0.0.0:5432".to_string()]);
        manager.process_exposed_datastores("srv-1", "srv", &[]);

        assert!(manager.get_active_alerts()[0].is_resolved());
    }

    #[test]
    fn test_exposed_datastore_message_lists_every_port() {
        let mut manager = AlertManager::new();
        let alert = manager
            .process_exposed_datastores(
                "srv-1",
                "alemanha6",
                &["0.0.0.0:5432".to_string(), "0.0.0.0:5433".to_string()],
            )
            .unwrap();

        assert!(alert.message.contains("5432"));
        assert!(alert.message.contains("5433"));
    }

    // ─────────────────────────────────────────
    // TLS certificates
    // ─────────────────────────────────────────

    #[test]
    fn test_tls_healthy_certificates_do_not_alert() {
        let mut manager = AlertManager::new();
        let alerts = manager.process_tls_certificates(
            "srv-1",
            "alemanha9",
            &[("example.com".to_string(), 60)],
            Some(("certbot.timer", "active")),
        );
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_tls_warning_at_three_weeks() {
        let mut manager = AlertManager::new();
        let alerts = manager.process_tls_certificates(
            "srv-1",
            "alemanha9",
            &[("example.com".to_string(), 21)],
            None,
        );

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].alert_type, AlertType::TlsCertificateExpiring);
        assert_eq!(alerts[0].severity, AlertSeverity::Warning);
    }

    #[test]
    fn test_tls_critical_at_one_week() {
        let mut manager = AlertManager::new();
        let alerts = manager.process_tls_certificates(
            "srv-1",
            "alemanha9",
            &[("example.com".to_string(), 7)],
            None,
        );

        assert_eq!(alerts[0].severity, AlertSeverity::Critical);
    }

    #[test]
    fn test_tls_expired_certificate_says_so() {
        let mut manager = AlertManager::new();
        let alerts = manager.process_tls_certificates(
            "srv-1",
            "alemanha9",
            &[("example.com".to_string(), -3)],
            None,
        );

        assert_eq!(alerts[0].severity, AlertSeverity::Critical);
        assert!(alerts[0].message.contains("expired 3 day(s) ago"));
    }

    #[test]
    fn test_tls_severity_follows_the_worst_certificate() {
        let mut manager = AlertManager::new();
        let alerts = manager.process_tls_certificates(
            "srv-1",
            "alemanha8",
            &[
                ("healthy.example".to_string(), 80),
                ("urgent.example".to_string(), 2),
                ("soon.example".to_string(), 15),
            ],
            None,
        );

        assert_eq!(alerts[0].severity, AlertSeverity::Critical);
        assert!(alerts[0].message.contains("urgent.example"));
        assert!(
            alerts[0].message.contains("2 of 3"),
            "the message should say how many are within the window: {}",
            alerts[0].message
        );
    }

    #[test]
    fn test_tls_alert_does_not_repeat() {
        let mut manager = AlertManager::new();
        let certs = [("example.com".to_string(), 10)];

        assert_eq!(
            manager
                .process_tls_certificates("srv-1", "srv", &certs, None)
                .len(),
            1
        );
        assert!(manager
            .process_tls_certificates("srv-1", "srv", &certs, None)
            .is_empty());
    }

    #[test]
    fn test_tls_alert_resolves_after_renewal() {
        let mut manager = AlertManager::new();

        manager.process_tls_certificates("srv-1", "srv", &[("example.com".to_string(), 5)], None);
        // Renewed: back to a full 90 day certificate
        manager.process_tls_certificates("srv-1", "srv", &[("example.com".to_string(), 89)], None);

        assert!(manager.get_active_alerts()[0].is_resolved());
    }

    #[test]
    fn test_broken_renewal_alerts_even_with_healthy_certificates() {
        // The fleet's actual state: 97 certificates all perfectly valid, and
        // certbot.service dead on three hosts. This is the leading indicator.
        let mut manager = AlertManager::new();
        let alerts = manager.process_tls_certificates(
            "srv-1",
            "alemanha8",
            &[("a.example".to_string(), 75), ("b.example".to_string(), 80)],
            Some(("certbot.service", "failed")),
        );

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].alert_type, AlertType::TlsRenewalBroken);
        assert!(alerts[0].message.contains("certbot.service"));
        assert!(alerts[0].message.contains('2'));
    }

    #[test]
    fn test_healthy_renewal_does_not_alert() {
        let mut manager = AlertManager::new();
        let alerts = manager.process_tls_certificates(
            "srv-1",
            "srv",
            &[("a.example".to_string(), 75)],
            Some(("certbot.timer", "active")),
        );
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_renewal_alert_resolves_when_fixed() {
        let mut manager = AlertManager::new();
        let certs = [("a.example".to_string(), 75)];

        manager.process_tls_certificates(
            "srv-1",
            "srv",
            &certs,
            Some(("certbot.service", "failed")),
        );
        manager.process_tls_certificates(
            "srv-1",
            "srv",
            &certs,
            Some(("certbot.service", "active")),
        );

        assert!(manager.get_active_alerts()[0].is_resolved());
    }

    #[test]
    fn test_expiry_and_renewal_alert_independently() {
        // Both broken at once must produce both alerts, not one
        let mut manager = AlertManager::new();
        let alerts = manager.process_tls_certificates(
            "srv-1",
            "srv",
            &[("a.example".to_string(), 3)],
            Some(("certbot.service", "failed")),
        );

        assert_eq!(alerts.len(), 2);
        let types: Vec<AlertType> = alerts.iter().map(|a| a.alert_type).collect();
        assert!(types.contains(&AlertType::TlsCertificateExpiring));
        assert!(types.contains(&AlertType::TlsRenewalBroken));
    }

    #[test]
    fn test_tls_with_no_certificates_still_checks_renewal() {
        let mut manager = AlertManager::new();
        let alerts = manager.process_tls_certificates(
            "srv-1",
            "srv",
            &[],
            Some(("certbot.service", "failed")),
        );
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].alert_type, AlertType::TlsRenewalBroken);
    }

    // ─────────────────────────────────────────
    // Container restarts
    // ─────────────────────────────────────────

    #[test]
    fn test_first_sighting_of_a_container_never_alerts() {
        // netfilter-mailcow sits at 412,813 restarts accumulated over months.
        // Connecting to that host must not fire an alert for old history.
        let mut manager = AlertManager::new();
        let alerts = manager.process_container_restarts(
            "srv-1",
            "alemanha3",
            &[("netfilter-mailcow".to_string(), 412_813)],
        );
        assert!(alerts.is_empty());
        assert!(manager.get_active_alerts().is_empty());
    }

    #[test]
    fn test_restarts_alert_on_rate_not_total() {
        let mut manager = AlertManager::new();
        let name = "netfilter-mailcow".to_string();

        // Baseline at a huge total
        manager.process_container_restarts("srv-1", "alemanha3", &[(name.clone(), 412_813)]);
        // Three more restarts since: that is the news
        let alerts =
            manager.process_container_restarts("srv-1", "alemanha3", &[(name.clone(), 412_816)]);

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].alert_type, AlertType::ContainerCrashLoop);
        assert_eq!(alerts[0].value, Some(3.0));
        assert!(alerts[0].message.contains("netfilter-mailcow"));
    }

    #[test]
    fn test_restarts_below_threshold_do_not_alert() {
        let mut manager = AlertManager::new();
        let name = "app".to_string();

        manager.process_container_restarts("srv-1", "srv", &[(name.clone(), 10)]);
        // Two restarts is a deploy, not a crash loop
        let alerts = manager.process_container_restarts("srv-1", "srv", &[(name.clone(), 12)]);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_restart_baseline_accumulates_within_the_window() {
        let mut manager = AlertManager::new();
        let name = "app".to_string();

        manager.process_container_restarts("srv-1", "srv", &[(name.clone(), 0)]);
        assert!(manager
            .process_container_restarts("srv-1", "srv", &[(name.clone(), 1)])
            .is_empty());
        assert!(manager
            .process_container_restarts("srv-1", "srv", &[(name.clone(), 2)])
            .is_empty());
        // Third restart crosses the threshold against the same baseline
        assert_eq!(
            manager
                .process_container_restarts("srv-1", "srv", &[(name.clone(), 3)])
                .len(),
            1
        );
    }

    #[test]
    fn test_recreated_container_resets_the_baseline() {
        let mut manager = AlertManager::new();
        let name = "app".to_string();

        manager.process_container_restarts("srv-1", "srv", &[(name.clone(), 50)]);
        // docker compose up recreates it: the counter starts from zero again
        let alerts = manager.process_container_restarts("srv-1", "srv", &[(name.clone(), 0)]);
        assert!(
            alerts.is_empty(),
            "a counter going backwards is a new container, not a fix"
        );

        // And the new baseline is the one used from here on
        assert!(
            manager
                .process_container_restarts("srv-1", "srv", &[(name.clone(), 3)])
                .len()
                == 1
        );
    }

    #[test]
    fn test_restarts_tracked_per_container() {
        let mut manager = AlertManager::new();

        manager.process_container_restarts(
            "srv-1",
            "srv",
            &[("a".to_string(), 0), ("b".to_string(), 0)],
        );
        let alerts = manager.process_container_restarts(
            "srv-1",
            "srv",
            &[("a".to_string(), 5), ("b".to_string(), 0)],
        );

        assert_eq!(alerts.len(), 1);
        assert!(alerts[0].message.contains("'a'"));
    }

    #[test]
    fn test_restarts_tracked_per_server() {
        let mut manager = AlertManager::new();
        let name = "app".to_string();

        manager.process_container_restarts("srv-1", "one", &[(name.clone(), 0)]);
        // Same container name on another host must not inherit the baseline
        let alerts = manager.process_container_restarts("srv-2", "two", &[(name.clone(), 99)]);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_crash_loop_alert_respects_cooldown() {
        let mut manager = AlertManager::new();
        let name = "app".to_string();

        manager.process_container_restarts("srv-1", "srv", &[(name.clone(), 0)]);
        assert_eq!(
            manager
                .process_container_restarts("srv-1", "srv", &[(name.clone(), 5)])
                .len(),
            1
        );
        // Still looping, but no second alert inside the cooldown
        assert!(manager
            .process_container_restarts("srv-1", "srv", &[(name.clone(), 10)])
            .is_empty());
    }

    // ─────────────────────────────────────────
    // Broken healthchecks
    // ─────────────────────────────────────────

    #[test]
    fn test_broken_healthchecks_reported_as_info() {
        let mut manager = AlertManager::new();
        let alert = manager
            .process_broken_healthchecks(
                "srv-1",
                "alemanha8",
                &["app-1".to_string(), "app-2".to_string()],
            )
            .expect("broken checks should be reported");

        assert_eq!(alert.alert_type, AlertType::ContainerHealthcheckBroken);
        assert_eq!(
            alert.severity,
            AlertSeverity::Info,
            "a probe that cannot run is maintenance, not an outage"
        );
        assert!(alert.message.contains("app-1"));
        assert!(alert.message.contains("app-2"));
    }

    #[test]
    fn test_broken_healthchecks_no_alert_when_none() {
        let mut manager = AlertManager::new();
        assert!(manager
            .process_broken_healthchecks("srv-1", "srv", &[])
            .is_none());
    }

    #[test]
    fn test_broken_healthchecks_do_not_repeat() {
        let mut manager = AlertManager::new();
        let broken = vec!["app-1".to_string()];

        assert!(manager
            .process_broken_healthchecks("srv-1", "srv", &broken)
            .is_some());
        assert!(
            manager
                .process_broken_healthchecks("srv-1", "srv", &broken)
                .is_none(),
            "a standing configuration defect must not be re-reported every poll"
        );
    }

    #[test]
    fn test_broken_healthchecks_resolve_when_fixed() {
        let mut manager = AlertManager::new();
        manager.process_broken_healthchecks("srv-1", "srv", &["app-1".to_string()]);
        manager.process_broken_healthchecks("srv-1", "srv", &[]);

        assert!(manager.get_active_alerts()[0].is_resolved());
    }

    #[test]
    fn test_systemd_failed_units_raises_warning() {
        let mut manager = AlertManager::new();
        let failed = vec!["certbot.service".to_string()];

        let alert = manager
            .process_systemd_failed_units("srv-1", "alemanha6", &failed)
            .expect("a failed unit must alert on the first observation");

        assert_eq!(alert.alert_type, AlertType::SystemdUnitFailed);
        assert_eq!(alert.severity, AlertSeverity::Warning);
        assert!(alert.message.contains("certbot.service"));
        assert_eq!(alert.value, Some(1.0));
        assert_eq!(manager.get_active_alerts().len(), 1);
    }

    #[test]
    fn test_systemd_failed_units_needs_no_sampling_window() {
        // Unlike threshold rules, one observation is enough: there is no
        // legitimate steady state where a unit stays failed.
        let mut manager = AlertManager::new();
        assert!(manager
            .process_systemd_failed_units("srv-1", "alemanha6", &["a.service".to_string()])
            .is_some());
    }

    #[test]
    fn test_systemd_failed_units_does_not_realert_while_failing() {
        let mut manager = AlertManager::new();
        let failed = vec!["certbot.service".to_string()];

        assert!(manager
            .process_systemd_failed_units("srv-1", "alemanha6", &failed)
            .is_some());
        // Polling again must not spam the notification channels
        assert!(manager
            .process_systemd_failed_units("srv-1", "alemanha6", &failed)
            .is_none());
        assert_eq!(manager.get_active_alerts().len(), 1);
    }

    #[test]
    fn test_systemd_failed_units_no_alert_when_healthy() {
        let mut manager = AlertManager::new();
        assert!(manager
            .process_systemd_failed_units("srv-1", "alemanha6", &[])
            .is_none());
        assert!(manager.get_active_alerts().is_empty());
    }

    #[test]
    fn test_systemd_failed_units_resolves_when_cleared() {
        let mut manager = AlertManager::new();
        let failed = vec!["certbot.service".to_string()];

        manager.process_systemd_failed_units("srv-1", "alemanha6", &failed);
        assert!(!manager.get_active_alerts()[0].is_resolved());

        manager.process_systemd_failed_units("srv-1", "alemanha6", &[]);
        assert!(
            manager.get_active_alerts()[0].is_resolved(),
            "fixing the unit must clear the alert"
        );
    }

    #[test]
    fn test_systemd_failed_units_cooldown_suppresses_flapping() {
        // A unit in a restart loop flips between failed and active on every
        // poll. The same five minute cooldown that guards the threshold rules
        // applies here, so flapping yields one alert, not one per poll.
        let mut manager = AlertManager::new();
        let failed = vec!["certbot.service".to_string()];

        assert!(manager
            .process_systemd_failed_units("srv-1", "alemanha6", &failed)
            .is_some());
        manager.process_systemd_failed_units("srv-1", "alemanha6", &[]);

        assert!(
            manager
                .process_systemd_failed_units("srv-1", "alemanha6", &failed)
                .is_none(),
            "re-failing inside the cooldown must not raise a second alert"
        );
        assert_eq!(
            manager.get_alert_history().len(),
            1,
            "the flap must leave a single entry in history"
        );
    }

    #[test]
    fn test_systemd_failed_units_tracked_per_server() {
        let mut manager = AlertManager::new();
        let failed = vec!["certbot.service".to_string()];

        assert!(manager
            .process_systemd_failed_units("srv-1", "alemanha6", &failed)
            .is_some());
        assert!(
            manager
                .process_systemd_failed_units("srv-2", "alemanha7", &failed)
                .is_some(),
            "a second host failing is a separate alert"
        );
        assert_eq!(manager.get_active_alerts().len(), 2);
    }

    #[test]
    fn test_systemd_failed_units_respects_silence() {
        let mut manager = AlertManager::new();
        manager.silence_alert("srv-1", AlertType::SystemdUnitFailed, Duration::minutes(30));

        assert!(manager
            .process_systemd_failed_units("srv-1", "alemanha6", &["certbot.service".to_string()])
            .is_none());
        assert!(manager.get_active_alerts().is_empty());
    }

    #[test]
    fn test_systemd_failed_units_message_lists_every_unit() {
        let mut manager = AlertManager::new();
        let failed = vec![
            "certbot.service".to_string(),
            "cloud-init.service".to_string(),
        ];

        let alert = manager
            .process_systemd_failed_units("srv-1", "alemanha6", &failed)
            .unwrap();

        assert!(alert.message.contains("certbot.service"));
        assert!(alert.message.contains("cloud-init.service"));
        assert!(alert.message.contains('2'));
    }

    #[test]
    fn test_alert_severity_display() {
        assert_eq!(format!("{}", AlertSeverity::Info), "info");
        assert_eq!(format!("{}", AlertSeverity::Warning), "warning");
        assert_eq!(format!("{}", AlertSeverity::Critical), "critical");
    }

    #[test]
    fn test_alert_state_add_sample_removes_old() {
        let mut state = AlertState::new();
        state.add_sample(50.0, Duration::seconds(60));
        assert_eq!(state.samples.len(), 1);
        // Use negative max_age to force removal of existing samples
        state.add_sample(60.0, Duration::seconds(-1));
        assert_eq!(state.samples.len(), 1);
    }

    #[test]
    fn test_alert_manager_process_metrics_server_down_continue() {
        let mut manager = AlertManager::new();
        manager.add_rule(AlertRule {
            id: "server-down".to_string(),
            name: "Server Down".to_string(),
            alert_type: AlertType::ServerDown,
            severity: AlertSeverity::Critical,
            enabled: true,
            threshold: 1.0,
            duration_seconds: 0,
            servers: vec![],
            channels: vec![],
        });

        let alerts = manager.process_metrics("srv1", "Server 1", 50.0, 50.0, 50.0);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_alert_manager_process_metrics_resolve_active_alert() {
        let mut manager = AlertManager::new();
        manager.add_rule(AlertRule {
            id: "cpu-high".to_string(),
            name: "CPU High".to_string(),
            alert_type: AlertType::CpuHigh,
            severity: AlertSeverity::Warning,
            enabled: true,
            threshold: 80.0,
            duration_seconds: 0,
            servers: vec![],
            channels: vec![],
        });

        // Trigger the alert
        let alerts = manager.process_metrics("srv1", "Server 1", 90.0, 50.0, 30.0);
        assert_eq!(alerts.len(), 1);
        assert!(!manager.active_alerts[0].is_resolved());

        // Resolve the alert by sending low CPU
        let alerts = manager.process_metrics("srv1", "Server 1", 50.0, 50.0, 30.0);
        assert!(alerts.is_empty());
        assert!(manager.active_alerts[0].is_resolved());
    }

    #[test]
    fn test_alert_manager_acknowledge_in_history() {
        let mut manager = AlertManager::new();
        let alert = Alert::new(
            AlertType::CpuHigh,
            AlertSeverity::Warning,
            "srv1".to_string(),
            "Server 1".to_string(),
            "CPU high".to_string(),
            Some(95.0),
            Some(80.0),
        );
        let alert_id = alert.id;
        manager.alert_history.push(alert);

        let result = manager.acknowledge_alert(alert_id, "admin".to_string());
        assert!(result.is_some());
        assert!(result.unwrap().acknowledged);
        assert_eq!(
            manager.alert_history[0].acknowledged_by.as_ref().unwrap(),
            "admin"
        );
    }

    #[test]
    fn test_alert_manager_default() {
        let manager = AlertManager::default();
        assert!(manager.rules.is_empty());
        assert!(manager.states.is_empty());
        assert!(manager.active_alerts.is_empty());
        assert!(manager.alert_history.is_empty());
        assert_eq!(manager.max_history, 1000);
    }
}
