//! systemd unit metrics collector
//!
//! Collects status information for configured systemd units, plus a host-wide
//! scan for units in the `failed` state.
//! On Linux, uses systemctl to get unit properties.
//! On other platforms, returns empty.

use anyhow::Result;
use chrono::{DateTime, Utc};
use shared::types::{SystemdFailedUnit, SystemdUnitInfo};
use std::collections::HashMap;
#[cfg(target_os = "linux")]
use tracing::warn;

/// Collector for systemd unit status
pub struct SystemdCollector {
    units: Vec<String>,
}

/// Parse the `key=value` lines emitted by `systemctl show` into a map.
///
/// Values may themselves contain `=`, so only the first separator splits.
fn parse_unit_properties(stdout: &str) -> HashMap<String, String> {
    let mut props = HashMap::new();

    for line in stdout.lines() {
        if let Some((key, value)) = line.split_once('=') {
            props.insert(key.to_string(), value.to_string());
        }
    }

    props
}

/// Parse a systemd timestamp such as `Mon 2024-01-01 10:00:00 UTC` or
/// `Thu 2026-07-31 10:39:13 -03`.
///
/// systemd prints timestamps in the host's local zone, so the trailing offset
/// is honoured when present. Named zones other than UTC cannot be resolved from
/// the string alone and are treated as UTC. Returns `None` for `n/a`, empty
/// values and anything unparseable.
fn parse_systemd_timestamp(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    if value.is_empty() || value == "n/a" {
        return None;
    }

    // "Thu 2026-07-31 10:39:13 -03" — weekday, date, time, zone
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }

    let local = chrono::NaiveDateTime::parse_from_str(
        &format!("{} {}", parts[1], parts[2]),
        "%Y-%m-%d %H:%M:%S",
    )
    .ok()?;

    // A numeric zone shifts the reading; a named one (UTC, CEST) cannot be
    // resolved from the string alone and is taken as UTC.
    let offset_seconds = parts
        .get(3)
        .and_then(|zone| parse_utc_offset_seconds(zone))
        .unwrap_or(0);

    Some(DateTime::<Utc>::from_naive_utc_and_offset(
        local - chrono::Duration::seconds(offset_seconds as i64),
        Utc,
    ))
}

/// Parse a numeric UTC offset as printed by systemd (`-03`, `+0530`, `+05:30`)
/// into seconds east of UTC. Named zones such as `UTC` yield `None`.
fn parse_utc_offset_seconds(token: &str) -> Option<i32> {
    let sign = match token.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };

    let digits = &token[1..];
    let (hours, minutes) = match digits.len() {
        2 => (digits.parse::<i32>().ok()?, 0),
        4 => (
            digits[0..2].parse::<i32>().ok()?,
            digits[2..4].parse::<i32>().ok()?,
        ),
        5 if digits.as_bytes()[2] == b':' => (
            digits[0..2].parse::<i32>().ok()?,
            digits[3..5].parse::<i32>().ok()?,
        ),
        _ => return None,
    };

    if hours > 23 || minutes > 59 {
        return None;
    }

    Some(sign * (hours * 3600 + minutes * 60))
}

/// Parse systemd's `MemoryCurrent` property, which is `[not set]` when the unit
/// has no memory accounting.
fn parse_memory_current(value: &str) -> Option<u64> {
    if value == "[not set]" || value.is_empty() {
        None
    } else {
        value.parse::<u64>().ok()
    }
}

/// Map systemd's `ActiveState`/`SubState` pair to a display status and whether
/// the unit counts as active.
fn unit_state_label(active_state: &str, sub_state: &str) -> (String, bool) {
    match active_state {
        "active" => ("active".to_string(), true),
        "inactive" => ("inactive".to_string(), false),
        "failed" => ("failed".to_string(), false),
        "activating" => (format!("activating ({})", sub_state), false),
        "deactivating" => (format!("deactivating ({})", sub_state), false),
        _ => (format!("{} ({})", active_state, sub_state), false),
    }
}

/// Parse `systemctl list-units --state=failed --plain --no-legend` into
/// `(unit name, description)` pairs.
///
/// Columns are `UNIT LOAD ACTIVE SUB DESCRIPTION`, the description being
/// everything past the fourth column. A leading bullet is tolerated so the
/// parser still works if `--plain` is ever dropped.
fn parse_failed_units(stdout: &str) -> Vec<(String, String)> {
    let mut units = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let mut fields = line.split_whitespace();
        let mut name = match fields.next() {
            Some(f) => f,
            None => continue,
        };

        // Drop the status bullet systemd prints without `--plain`
        if name == "\u{25cf}" || name == "*" || name == "x" {
            name = match fields.next() {
                Some(f) => f,
                None => continue,
            };
        }

        // Skip LOAD, ACTIVE and SUB columns; anything left is the description
        let load = fields.next();
        let active = fields.next();
        let sub = fields.next();
        if load.is_none() || active.is_none() || sub.is_none() {
            continue;
        }

        let description = fields.collect::<Vec<_>>().join(" ");
        units.push((name.to_string(), description));
    }

    units
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

    /// Status of a single unit, regardless of whether it is configured for
    /// monitoring.
    ///
    /// Used to answer questions about a specific unit that another collector
    /// cares about — the certificate inventory needs to know whether renewal
    /// still runs, since a healthy certificate with a broken timer is an
    /// outage on a schedule.
    pub async fn unit_status(&self, unit: &str) -> Option<String> {
        #[cfg(target_os = "linux")]
        {
            self.collect_unit(unit).await.ok().map(|info| info.status)
        }

        #[cfg(not(target_os = "linux"))]
        {
            let _ = unit;
            None
        }
    }

    /// Scan the whole host for units in the `failed` state.
    ///
    /// Deliberately independent of the configured unit list: a failed unit
    /// nobody thought to configure is exactly the one worth surfacing. Returns
    /// an empty list — never an error — when systemd is unavailable, so a host
    /// without systemd does not fail the whole RPC.
    pub async fn collect_failed(&self) -> Result<Vec<SystemdFailedUnit>> {
        #[cfg(target_os = "linux")]
        {
            self.collect_failed_linux().await
        }

        #[cfg(not(target_os = "linux"))]
        {
            Ok(Vec::new())
        }
    }

    #[cfg(target_os = "linux")]
    async fn collect_failed_linux(&self) -> Result<Vec<SystemdFailedUnit>> {
        use std::process::Command;

        let output = match Command::new("systemctl")
            .args([
                "list-units",
                "--state=failed",
                "--plain",
                "--no-legend",
                "--no-pager",
            ])
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                warn!("Failed to list failed systemd units: {}", e);
                return Ok(Vec::new());
            }
        };

        if !output.status.success() {
            warn!(
                "systemctl list-units failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let entries = parse_failed_units(&stdout);

        let mut failed = Vec::with_capacity(entries.len());
        for (name, description) in entries {
            let since = self.unit_state_change(&name);
            failed.push(SystemdFailedUnit {
                name,
                description,
                since,
            });
        }

        Ok(failed)
    }

    /// Read when a unit last changed state. Best effort: a missing value only
    /// costs the `since` column.
    #[cfg(target_os = "linux")]
    fn unit_state_change(&self, unit: &str) -> Option<DateTime<Utc>> {
        use std::process::Command;

        let output = Command::new("systemctl")
            .args([
                "show",
                unit,
                "--property=StateChangeTimestamp",
                "--no-pager",
            ])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let props = parse_unit_properties(&stdout);
        props
            .get("StateChangeTimestamp")
            .and_then(|v| parse_systemd_timestamp(v))
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
        let props = parse_unit_properties(&stdout);

        let active_state = props.get("ActiveState").cloned().unwrap_or_default();
        let sub_state = props.get("SubState").cloned().unwrap_or_default();

        let main_pid = props
            .get("MainPID")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);

        let memory_current = props
            .get("MemoryCurrent")
            .and_then(|v| parse_memory_current(v))
            .unwrap_or(0);

        let started_at = props
            .get("ExecMainStartTimestamp")
            .and_then(|v| parse_systemd_timestamp(v));

        let (status, is_active) = unit_state_label(&active_state, &sub_state);

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

    #[tokio::test]
    async fn test_systemd_collect_failed_never_errors() {
        // Scanning for failed units must degrade to an empty list rather than
        // failing the RPC, including on hosts without systemd.
        let collector = SystemdCollector::new(Vec::new());
        let result = collector.collect_failed().await;
        assert!(result.is_ok());
    }

    // --- systemctl show property parsing ---

    #[test]
    fn test_parse_unit_properties_active_unit() {
        let output = "ActiveState=active\nSubState=running\nMainPID=1234\n\
                      ExecMainStartTimestamp=Mon 2024-01-01 10:00:00 UTC\nMemoryCurrent=1048576\n";
        let props = parse_unit_properties(output);

        assert_eq!(props.get("ActiveState").unwrap(), "active");
        assert_eq!(props.get("MainPID").unwrap(), "1234");
        assert_eq!(props.get("MemoryCurrent").unwrap(), "1048576");
    }

    #[test]
    fn test_parse_unit_properties_keeps_equals_in_value() {
        let props = parse_unit_properties("Description=nginx --conf=/etc/nginx.conf\n");
        assert_eq!(
            props.get("Description").unwrap(),
            "nginx --conf=/etc/nginx.conf"
        );
    }

    #[test]
    fn test_parse_unit_properties_ignores_malformed_lines() {
        let props = parse_unit_properties("garbage line\nActiveState=failed\n");
        assert_eq!(props.len(), 1);
        assert_eq!(props.get("ActiveState").unwrap(), "failed");
    }

    // --- state label ---

    #[test]
    fn test_unit_state_label_active() {
        let (status, is_active) = unit_state_label("active", "running");
        assert_eq!(status, "active");
        assert!(is_active);
    }

    #[test]
    fn test_unit_state_label_failed() {
        let (status, is_active) = unit_state_label("failed", "failed");
        assert_eq!(status, "failed");
        assert!(!is_active);
    }

    #[test]
    fn test_unit_state_label_activating_keeps_sub_state() {
        let (status, is_active) = unit_state_label("activating", "auto-restart");
        assert_eq!(status, "activating (auto-restart)");
        assert!(!is_active);
    }

    #[test]
    fn test_unit_state_label_unknown_state() {
        let (status, is_active) = unit_state_label("reloading", "reload");
        assert_eq!(status, "reloading (reload)");
        assert!(!is_active);
    }

    // --- memory ---

    #[test]
    fn test_parse_memory_current_not_set() {
        assert!(parse_memory_current("[not set]").is_none());
        assert!(parse_memory_current("").is_none());
    }

    #[test]
    fn test_parse_memory_current_value() {
        assert_eq!(parse_memory_current("1048576"), Some(1_048_576));
    }

    // --- timestamps ---

    #[test]
    fn test_parse_systemd_timestamp_na_and_empty() {
        assert!(parse_systemd_timestamp("n/a").is_none());
        assert!(parse_systemd_timestamp("").is_none());
        assert!(parse_systemd_timestamp("   ").is_none());
    }

    #[test]
    fn test_parse_systemd_timestamp_utc() {
        let ts = parse_systemd_timestamp("Mon 2024-01-01 10:00:00 UTC").unwrap();
        assert_eq!(ts.to_rfc3339(), "2024-01-01T10:00:00+00:00");
    }

    #[test]
    fn test_parse_systemd_timestamp_honours_numeric_offset() {
        // systemd prints local time; -03 must shift to 13:39 UTC
        let ts = parse_systemd_timestamp("Thu 2026-07-31 10:39:13 -03").unwrap();
        assert_eq!(ts.to_rfc3339(), "2026-07-31T13:39:13+00:00");
    }

    #[test]
    fn test_parse_systemd_timestamp_positive_offset_with_minutes() {
        let ts = parse_systemd_timestamp("Wed 2026-07-01 12:00:00 +0530").unwrap();
        assert_eq!(ts.to_rfc3339(), "2026-07-01T06:30:00+00:00");
    }

    #[test]
    fn test_parse_systemd_timestamp_unknown_named_zone_falls_back_to_utc() {
        // CEST cannot be resolved from the string alone; taken at face value.
        let ts = parse_systemd_timestamp("Wed 2026-07-01 12:00:00 CEST").unwrap();
        assert_eq!(ts.to_rfc3339(), "2026-07-01T12:00:00+00:00");
    }

    #[test]
    fn test_parse_systemd_timestamp_garbage() {
        assert!(parse_systemd_timestamp("not a timestamp").is_none());
    }

    #[test]
    fn test_parse_utc_offset_seconds_forms() {
        assert_eq!(parse_utc_offset_seconds("-03"), Some(-10_800));
        assert_eq!(parse_utc_offset_seconds("+00"), Some(0));
        assert_eq!(parse_utc_offset_seconds("+0530"), Some(19_800));
        assert_eq!(parse_utc_offset_seconds("+05:30"), Some(19_800));
    }

    #[test]
    fn test_parse_utc_offset_seconds_rejects_non_numeric_zones() {
        assert!(parse_utc_offset_seconds("UTC").is_none());
        assert!(parse_utc_offset_seconds("CEST").is_none());
        assert!(parse_utc_offset_seconds("").is_none());
    }

    #[test]
    fn test_parse_utc_offset_seconds_rejects_out_of_range() {
        assert!(parse_utc_offset_seconds("+99").is_none());
        assert!(parse_utc_offset_seconds("+0399").is_none());
    }

    // --- failed unit listing ---

    #[test]
    fn test_parse_failed_units_multiple() {
        let output = "certbot.service loaded failed failed Certbot\n\
                      cloud-init.service loaded failed failed Initial cloud-init job (metadata service crawler)\n";
        let units = parse_failed_units(output);

        assert_eq!(units.len(), 2);
        assert_eq!(units[0].0, "certbot.service");
        assert_eq!(units[0].1, "Certbot");
        assert_eq!(units[1].0, "cloud-init.service");
        assert_eq!(
            units[1].1,
            "Initial cloud-init job (metadata service crawler)"
        );
    }

    #[test]
    fn test_parse_failed_units_empty_output() {
        // A healthy host prints nothing with --no-legend
        assert!(parse_failed_units("").is_empty());
        assert!(parse_failed_units("\n\n").is_empty());
    }

    #[test]
    fn test_parse_failed_units_tolerates_bullet() {
        let output = "● networking.service loaded failed failed Raise network interfaces\n";
        let units = parse_failed_units(output);

        assert_eq!(units.len(), 1);
        assert_eq!(units[0].0, "networking.service");
        assert_eq!(units[0].1, "Raise network interfaces");
    }

    #[test]
    fn test_parse_failed_units_skips_short_lines() {
        // Truncated output must not produce a unit with a bogus description
        let units = parse_failed_units("certbot.service loaded failed\n");
        assert!(units.is_empty());
    }

    #[test]
    fn test_parse_failed_units_allows_missing_description() {
        let units = parse_failed_units("weird.service loaded failed failed\n");
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].0, "weird.service");
        assert_eq!(units[0].1, "");
    }
}
