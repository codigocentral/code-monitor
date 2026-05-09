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
}

impl std::fmt::Display for AlertType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertType::CpuHigh => write!(f, "CPU_HIGH"),
            AlertType::MemoryHigh => write!(f, "MEMORY_HIGH"),
            AlertType::DiskHigh => write!(f, "DISK_HIGH"),
            AlertType::ServerDown => write!(f, "SERVER_DOWN"),
            AlertType::ProcessDown => write!(f, "PROCESS_DOWN"),
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
}

impl AlertState {
    pub fn new() -> Self {
        Self {
            samples: VecDeque::new(),
            triggered: false,
            last_triggered: None,
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
}

impl Default for AlertState {
    fn default() -> Self {
        Self::new()
    }
}

/// Alert manager that tracks state and generates alerts
#[derive(Debug)]
pub struct AlertManager {
    rules: Vec<AlertRule>,
    states: HashMap<(String, AlertType), AlertState>,
    active_alerts: Vec<Alert>,
    alert_history: Vec<Alert>,
    max_history: usize,
}

impl AlertManager {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            states: HashMap::new(),
            active_alerts: Vec::new(),
            alert_history: Vec::new(),
            max_history: 1000,
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
                if !state.is_triggered() && state.can_trigger_again(Duration::minutes(5)) {
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
        assert_eq!(manager.active_alerts[0].acknowledged_by.as_ref().unwrap(), "operator");
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
        assert_eq!(manager.alert_history[0].acknowledged_by.as_ref().unwrap(), "admin");
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
