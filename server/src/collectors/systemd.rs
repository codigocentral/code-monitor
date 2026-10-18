//! systemd unit metrics collector
//!
//! Collects status information for configured systemd units, plus a host-wide
//! scan for units in the `failed` state.
//! On Linux, uses systemctl to get unit properties.
//! On other platforms, returns empty.

use anyhow::Result;
#[cfg(test)]
#[cfg(feature = "unit-doubles")]
use campaign_systemd_units::Command;
use chrono::{DateTime, Utc};
use shared::types::{SystemdFailedUnit, SystemdUnitInfo};
use std::collections::HashMap;
#[cfg(all(
    any(target_os = "linux", test),
    not(all(test, feature = "unit-doubles"))
))]
use std::process::Command;
#[cfg(any(target_os = "linux", test))]
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
        // A trimmed, nonempty line necessarily has a first whitespace token.
        let mut name = fields.next().unwrap();

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

    /// Collect status for all configured units.
    pub async fn collect(&self) -> Result<Vec<SystemdUnitInfo>> {
        self.collect_for_platform(cfg!(target_os = "linux")).await
    }

    async fn collect_for_platform(&self, linux: bool) -> Result<Vec<SystemdUnitInfo>> {
        if self.units.is_empty() {
            return Ok(Vec::new());
        }
        #[cfg(any(target_os = "linux", test))]
        if linux {
            return self.collect_linux().await;
        }
        let _ = linux;
        Ok(Vec::new())
    }

    /// Status of a single unit, whether or not it is configured for monitoring.
    pub async fn unit_status(&self, unit: &str) -> Option<String> {
        self.unit_status_for_platform(unit, cfg!(target_os = "linux"))
            .await
    }

    async fn unit_status_for_platform(&self, unit: &str, linux: bool) -> Option<String> {
        #[cfg(any(target_os = "linux", test))]
        if linux {
            return self.collect_unit(unit).await.ok().map(|info| info.status);
        }
        let _ = (unit, linux);
        None
    }

    /// Scan host-wide failed units; unavailable systemd yields an empty list.
    pub async fn collect_failed(&self) -> Result<Vec<SystemdFailedUnit>> {
        self.collect_failed_for_platform(cfg!(target_os = "linux"))
            .await
    }

    async fn collect_failed_for_platform(&self, linux: bool) -> Result<Vec<SystemdFailedUnit>> {
        #[cfg(any(target_os = "linux", test))]
        if linux {
            return self.collect_failed_linux().await;
        }
        let _ = linux;
        Ok(Vec::new())
    }

    #[cfg(any(target_os = "linux", test))]
    async fn collect_failed_linux(&self) -> Result<Vec<SystemdFailedUnit>> {
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
    #[cfg(any(target_os = "linux", test))]
    fn unit_state_change(&self, unit: &str) -> Option<DateTime<Utc>> {
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

    #[cfg(any(target_os = "linux", test))]
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

    #[cfg(any(target_os = "linux", test))]
    async fn collect_unit(&self, unit: &str) -> Result<SystemdUnitInfo> {
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

#[cfg(test)]
#[cfg(feature = "unit-doubles")]
pub(crate) mod campaign_systemd_units {
    use super::*;
    use std::{
        cell::RefCell,
        collections::VecDeque,
        io,
        process::{ExitStatus, Output},
    };
    thread_local! { static RESPONSES: RefCell<VecDeque<(Vec<String>, io::Result<Output>)>> = const { RefCell::new(VecDeque::new()) }; }
    pub struct Command {
        args: Vec<String>,
    }
    impl Command {
        pub fn new(program: &str) -> Self {
            assert_eq!(program, "systemctl");
            Self { args: vec![] }
        }
        pub fn args<const N: usize>(&mut self, args: [&str; N]) -> &mut Self {
            self.args.extend(args.map(str::to_owned));
            self
        }
        pub fn output(&mut self) -> io::Result<Output> {
            RESPONSES.with(|queue| {
                let (expected, result) =
                    queue.borrow_mut().pop_front().expect("unexpected command");
                assert_eq!(self.args, expected);
                result
            })
        }
    }
    fn output(success: bool, stdout: &str) -> io::Result<Output> {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt;
        Ok(Output {
            status: ExitStatus::from_raw(if success { 0 } else { 256 }),
            stdout: stdout.as_bytes().into(),
            stderr: b"unit systemctl refused".into(),
        })
    }
    fn queue(args: &[&str], result: io::Result<Output>) {
        RESPONSES.with(|queue| {
            queue
                .borrow_mut()
                .push_back((args.iter().map(|s| (*s).into()).collect(), result))
        });
    }
    fn show(unit: &str, result: io::Result<Output>) {
        queue(
            &[
                "show",
                unit,
                "--property=ActiveState",
                "--property=SubState",
                "--property=MainPID",
                "--property=ExecMainStartTimestamp",
                "--property=MemoryCurrent",
                "--no-pager",
            ],
            result,
        );
    }
    fn timestamp(unit: &str, result: io::Result<Output>) {
        queue(
            &[
                "show",
                unit,
                "--property=StateChangeTimestamp",
                "--no-pager",
            ],
            result,
        );
    }
    fn list(result: io::Result<Output>) {
        queue(
            &[
                "list-units",
                "--state=failed",
                "--plain",
                "--no-legend",
                "--no-pager",
            ],
            result,
        );
    }
    fn unavailable() -> io::Result<Output> {
        Err(io::Error::new(io::ErrorKind::NotFound, "unit unavailable"))
    }
    fn drained() {
        RESPONSES.with(|queue| assert!(queue.borrow().is_empty()));
    }

    #[tokio::test]
    async fn unit_status_decodes_properties_and_preserves_failure_policy() {
        let (_guard, logs) = crate::unit_logs::capture();
        let collector = SystemdCollector::new(vec!["ok.service".into(), "bad.service".into()]);
        show("ok.service", output(true, "ActiveState=active\nSubState=running\nMainPID=42\nMemoryCurrent=8192\nExecMainStartTimestamp=Mon 2024-01-01 10:00:00 UTC\n"));
        show("bad.service", unavailable());
        let units = collector.collect_for_platform(true).await.unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "ok.service");
        assert!(units[0].is_active);
        assert_eq!(units[0].pid, Some(42));
        assert_eq!(units[0].memory_current_bytes, 8192);
        assert_eq!(
            units[0].started_at.unwrap().to_rfc3339(),
            "2024-01-01T10:00:00+00:00"
        );
        show("missing", output(true, ""));
        let unit = collector.collect_unit("missing").await.unwrap();
        assert_eq!(unit.pid, None);
        assert_eq!(unit.memory_current_bytes, 0);
        assert_eq!(unit.started_at, None);
        show(
            "invalid",
            output(
                true,
                "MainPID=bad\nMemoryCurrent=bad\nExecMainStartTimestamp=n/a\n",
            ),
        );
        assert_eq!(collector.collect_unit("invalid").await.unwrap().pid, None);
        show("bad-exit", output(false, ""));
        assert!(collector
            .collect_unit("bad-exit")
            .await
            .unwrap_err()
            .to_string()
            .contains("unit systemctl refused"));
        show("status", output(true, "ActiveState=inactive\n"));
        assert_eq!(
            collector
                .unit_status_for_platform("status", true)
                .await
                .as_deref(),
            Some("inactive")
        );
        show("status", unavailable());
        assert_eq!(
            collector.unit_status_for_platform("status", true).await,
            None
        );
        assert!(collector
            .collect_for_platform(false)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            collector.unit_status_for_platform("unused", false).await,
            None
        );
        assert!(collector
            .collect_failed_for_platform(false)
            .await
            .unwrap()
            .is_empty());
        // Public methods preserve the native OS dispatch without native commands.
        if cfg!(target_os = "linux") {
            show("public", output(true, "ActiveState=active\n"));
            assert_eq!(
                collector.unit_status("public").await.as_deref(),
                Some("active")
            );
        } else {
            assert_eq!(collector.unit_status("public").await, None);
        }
        let empty = SystemdCollector::new(vec![]);
        assert!(empty.collect().await.unwrap().is_empty());
        if cfg!(target_os = "linux") {
            show("ok.service", output(true, "ActiveState=active\n"));
            show("bad.service", output(false, ""));
            assert_eq!(collector.collect().await.unwrap().len(), 1);
            list(output(true, ""));
        } else {
            assert!(collector.collect().await.unwrap().is_empty());
        }
        assert!(collector.collect_failed().await.unwrap().is_empty());
        assert!(!logs.lock().unwrap().is_empty());
        drained();
    }

    #[tokio::test]
    async fn failed_units_scan_is_best_effort_and_keeps_timestamps() {
        let (_guard, logs) = crate::unit_logs::capture();
        let collector = SystemdCollector::new(vec![]);
        list(unavailable());
        assert!(collector
            .collect_failed_for_platform(true)
            .await
            .unwrap()
            .is_empty());
        list(output(false, ""));
        assert!(collector
            .collect_failed_for_platform(true)
            .await
            .unwrap()
            .is_empty());
        list(output(true, "broken.service loaded failed failed Unit failed\nmissing.service loaded failed failed No time\n"));
        timestamp(
            "broken.service",
            output(true, "StateChangeTimestamp=Mon 2024-01-01 10:00:00 UTC\n"),
        );
        timestamp("missing.service", unavailable());
        let units = collector.collect_failed_for_platform(true).await.unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].name, "broken.service");
        assert_eq!(units[0].description, "Unit failed");
        assert!(units[0].since.is_some());
        assert!(units[1].since.is_none());
        timestamp("bad", output(false, ""));
        assert!(collector.unit_state_change("bad").is_none());
        timestamp("absent", output(true, ""));
        assert!(collector.unit_state_change("absent").is_none());
        timestamp("invalid", output(true, "StateChangeTimestamp=invalid\n"));
        assert!(collector.unit_state_change("invalid").is_none());
        assert!(logs
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains("systemctl list-units failed")));
        drained();
    }

    pub(crate) async fn exercise_log_fallback() {
        let collector = SystemdCollector::new(vec![]);
        list(output(false, ""));
        assert!(collector
            .collect_failed_for_platform(true)
            .await
            .unwrap()
            .is_empty());
        drained();
    }

    #[test]
    fn offset_and_failed_row_edge_cases() {
        assert_eq!(
            unit_state_label("deactivating", "stop-sigterm"),
            ("deactivating (stop-sigterm)".into(), false)
        );
        for invalid in [
            "", "+xy", "+xy12", "+12xy", "+xy:12", "+12:xy", "+12x34", "+123", "+24", "+1260",
        ] {
            assert_eq!(parse_utc_offset_seconds(invalid), None, "{invalid}");
        }
        assert!(parse_systemd_timestamp("Mon invalid 10:00:00 UTC").is_none());
        assert_eq!(parse_failed_units("\n●\n*\nx\nshort loaded\n"), vec![]);
    }
}
