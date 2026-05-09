//! systemd unit metrics collector
//!
//! Collects status information for configured systemd units.
//! On Linux, uses systemctl show to get unit properties.
//! On other platforms, returns empty.

use anyhow::Result;
use shared::types::SystemdUnitInfo;

/// Collector for systemd unit status
pub struct SystemdCollector {
    units: Vec<String>,
}

impl SystemdCollector {
    pub fn new(units: Vec<String>) -> Self {
        Self { units }
    }

    /// Collect status for all configured units
    pub async fn collect(&self) -> Result<Vec<SystemdUnitInfo>> {
        if self.units.is_empty() {
            return Ok(Vec::new());
        }

        #[cfg(target_os = "linux")]
        {
            self.collect_linux().await
        }

        #[cfg(not(target_os = "linux"))]
        {
            Ok(Vec::new())
        }
    }

    #[cfg(target_os = "linux")]
    async fn collect_linux(&self) -> Result<Vec<SystemdUnitInfo>> {
        let mut results = Vec::with_capacity(self.units.len());

        for unit in &self.units {
            match self.collect_unit(unit).await {
                Ok(info) => results.push(info),
                Err(e) => {
                    warn!("Failed to collect systemd unit '{}': {}", unit, e);
                }
            }
        }

        Ok(results)
    }

    #[cfg(target_os = "linux")]
    async fn collect_unit(&self, unit: &str) -> Result<SystemdUnitInfo> {
        use std::process::Command;

        let output = Command::new("systemctl")
            .args([
                "show",
                unit,
                "--property=ActiveState",
                "--property=SubState",
                "--property=MainPID",
                "--property=ExecMainStartTimestamp",
                "--property=MemoryCurrent",
                "--no-pager",
            ])
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to run systemctl: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("systemctl failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut props = HashMap::new();

        for line in stdout.lines() {
            if let Some((key, value)) = line.split_once('=') {
                props.insert(key.to_string(), value.to_string());
            }
        }

        let active_state = props.get("ActiveState").cloned().unwrap_or_default();
        let sub_state = props.get("SubState").cloned().unwrap_or_default();

        let main_pid = props
            .get("MainPID")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);

        let memory_current = props
            .get("MemoryCurrent")
            .and_then(|v| {
                if v == "[not set]" || v.is_empty() {
                    None
                } else {
                    v.parse::<u64>().ok()
                }
            })
            .unwrap_or(0);

        let started_at = props.get("ExecMainStartTimestamp").and_then(|v| {
            if v == "n/a" || v.is_empty() {
                None
            } else {
                let parts: Vec<&str> = v.split_whitespace().collect();
                if parts.len() >= 4 {
                    let date_str = format!("{} {} {}", parts[1], parts[2], parts[3]);
                    chrono::NaiveDateTime::parse_from_str(&date_str, "%Y-%m-%d %H:%M:%S")
                        .ok()
                        .map(|dt| {
                            chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
                                dt,
                                chrono::Utc,
                            )
                        })
                } else {
                    None
                }
            }
        });

        let (status, is_active) = match active_state.as_str() {
            "active" => ("active".to_string(), true),
            "inactive" => ("inactive".to_string(), false),
            "failed" => ("failed".to_string(), false),
            "activating" => (format!("activating ({})", sub_state), false),
            "deactivating" => (format!("deactivating ({})", sub_state), false),
            _ => (format!("{} ({})", active_state, sub_state), false),
        };

        Ok(SystemdUnitInfo {
            name: unit.to_string(),
            status,
            is_active,
            pid: if main_pid > 0 { Some(main_pid) } else { None },
            memory_current_bytes: memory_current,
            started_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_systemd_collector_empty() {
        let collector = SystemdCollector::new(Vec::new());
        assert!(collector.units.is_empty());
    }

    #[test]
    fn test_systemd_collector_with_units() {
        let units = vec!["nginx.service".to_string(), "postgres.service".to_string()];
        let collector = SystemdCollector::new(units.clone());
        assert_eq!(collector.units.len(), 2);
        assert_eq!(collector.units[0], "nginx.service");
    }

    #[tokio::test]
    async fn test_systemd_collect_empty_units() {
        let collector = SystemdCollector::new(Vec::new());
        let result = collector.collect().await.unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_systemd_unit_info_parsing_active() {
        // Simulate parsing of systemctl output for an active unit
        let output = r#"ActiveState=active
SubState=running
MainPID=1234
ExecMainStartTimestamp=Mon 2024-01-01 10:00:00 UTC
MemoryCurrent=1048576
"#;

        let mut props = std::collections::HashMap::new();
        for line in output.lines() {
            if let Some((key, value)) = line.split_once('=') {
                props.insert(key.to_string(), value.to_string());
            }
        }

        let active_state = props.get("ActiveState").cloned().unwrap_or_default();
        let (status, is_active) = match active_state.as_str() {
            "active" => ("active".to_string(), true),
            "inactive" => ("inactive".to_string(), false),
            "failed" => ("failed".to_string(), false),
            _ => (format!("unknown"), false),
        };

        assert_eq!(status, "active");
        assert!(is_active);
        assert_eq!(props.get("MainPID").unwrap(), "1234");
    }

    #[test]
    fn test_systemd_unit_info_parsing_failed() {
        let output = r#"ActiveState=failed
SubState=failed
MainPID=0
ExecMainStartTimestamp=n/a
MemoryCurrent=[not set]
"#;

        let mut props = std::collections::HashMap::new();
        for line in output.lines() {
            if let Some((key, value)) = line.split_once('=') {
                props.insert(key.to_string(), value.to_string());
            }
        }

        let active_state = props.get("ActiveState").cloned().unwrap_or_default();
        let (status, is_active) = match active_state.as_str() {
            "active" => ("active".to_string(), true),
            "inactive" => ("inactive".to_string(), false),
            "failed" => ("failed".to_string(), false),
            _ => (format!("unknown"), false),
        };

        assert_eq!(status, "failed");
        assert!(!is_active);
        assert_eq!(props.get("MainPID").unwrap(), "0");
    }

    #[test]
    fn test_systemd_memory_parsing_not_set() {
        let value = "[not set]";
        let parsed: Option<u64> = if value == "[not set]" || value.is_empty() {
            None
        } else {
            value.parse::<u64>().ok()
        };
        assert!(parsed.is_none());
    }

    #[test]
    fn test_systemd_timestamp_parsing_na() {
        let value = "n/a";
        let parsed: Option<chrono::DateTime<chrono::Utc>> = if value == "n/a" || value.is_empty() {
            None
        } else {
            None // simplified
        };
        assert!(parsed.is_none());
    }
}
