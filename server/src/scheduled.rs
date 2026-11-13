//! Bounded, read-only systemd timer collection, with optional durable status.
use shared::operations::{ScheduledJob, ScheduledJobConfig};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::process::Command;

const PROPERTIES: &str = "LoadState,ActiveState,UnitFileState,Result,ExecMainStatus,ExecMainCode,ExecMainStartTimestamp,ExecMainExitTimestamp,LastTriggerUSec,NextElapseUSecRealtime";

fn properties(text: &str) -> BTreeMap<&str, &str> {
    text.lines().filter_map(|l| l.split_once('=')).collect()
}
fn timestamp(value: Option<&&str>) -> i64 {
    let Some(value) = value else { return 0 };
    // systemd's textual timestamps have second precision on Ubuntu 22.04 too.
    chrono::DateTime::parse_from_str(value, "%a %Y-%m-%d %H:%M:%S %Z")
        .map(|t| t.timestamp())
        .unwrap_or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(value, "%a %Y-%m-%d %H:%M:%S UTC")
                .map(|t| t.and_utc().timestamp())
                .unwrap_or(0)
        })
}
async fn show(unit: &str) -> anyhow::Result<String> {
    let mut command = Command::new("systemctl");
    command
        .args(["show", "--no-pager", "--property", PROPERTIES, "--", unit])
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(3), command.output()).await??;
    anyhow::ensure!(output.status.success(), "systemctl unavailable");
    anyhow::ensure!(output.stdout.len() <= 65536, "oversized systemctl response");
    Ok(String::from_utf8(output.stdout)?)
}
fn parse(config: &ScheduledJobConfig, service: &str, timer: &str) -> ScheduledJob {
    let s = properties(service);
    let t = properties(timer);
    let loaded = s.get("LoadState") == Some(&"loaded") && t.get("LoadState") == Some(&"loaded");
    let start = timestamp(s.get("ExecMainStartTimestamp"));
    let end = timestamp(s.get("ExecMainExitTimestamp"));
    let running = matches!(s.get("ActiveState"), Some(&"active" | &"activating")) && start > end;
    let code = s
        .get("ExecMainStatus")
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1);
    let result = if running {
        "running"
    } else {
        s.get("Result").copied().unwrap_or("unknown")
    };
    ScheduledJob {
        name: config.name.clone(),
        service: config.service.clone(),
        timer: config.timer.clone(),
        max_interval_seconds: config.max_interval_seconds,
        collection_up: loaded,
        timer_active: t.get("ActiveState") == Some(&"active")
            && !matches!(
                t.get("UnitFileState"),
                Some(&"disabled" | &"masked" | &"masked-runtime")
            ),
        running,
        last_run: start,
        last_success: if !running && result == "success" && code == 0 {
            end
        } else {
            0
        },
        next_run: timestamp(t.get("NextElapseUSecRealtime")),
        result: result.into(),
        exit_code: if running { 0 } else { code },
    }
}
/// Accept only exact scalar metric names, never arbitrary label/output text.
fn apply_status(job: &mut ScheduledJob, text: &str, prefix: &str, now: i64) -> anyhow::Result<()> {
    let metrics: BTreeMap<_, _> = text
        .lines()
        .filter_map(|l| {
            let mut fields = l.split_whitespace();
            let name = fields.next()?;
            let value = fields.next()?;
            if fields.next().is_some() || name.starts_with('#') {
                return None;
            }
            Some((name, value))
        })
        .collect();
    let read = |suffix: &str| -> anyhow::Result<i64> {
        let key = format!("{prefix}_{suffix}");
        let value: f64 = metrics
            .get(key.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing status field"))?
            .parse()?;
        anyhow::ensure!(
            value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= now as f64 + 60.0,
            "invalid status field"
        );
        Ok(value as i64)
    };
    let run = read("last_run_timestamp_seconds")?;
    let success = read("last_success_timestamp_seconds")?;
    let code = read("last_exit_code")?;
    anyhow::ensure!(success <= run && code <= 255, "inconsistent status");
    job.last_success = success;
    // A persistent completion survives reboot/reset-failed. A currently newer
    // invocation must not be mistaken for the previous failed attempt.
    if run >= job.last_run && !job.running {
        job.last_run = run;
        job.exit_code = code as i32;
        job.result = if code == 0 { "success" } else { "exit-code" }.into();
    }
    Ok(())
}
pub async fn collect(configs: &[ScheduledJobConfig]) -> Vec<ScheduledJob> {
    let mut jobs = Vec::with_capacity(configs.len());
    for config in configs {
        let collected = async {
            let (service, timer) = tokio::try_join!(show(&config.service), show(&config.timer))?;
            let mut job = parse(config, &service, &timer);
            if let (Some(path), Some(prefix)) = (&config.status_file, &config.metric_prefix) {
                use tokio::io::AsyncReadExt;
                // A configured file is required evidence, not an optional success.
                let metadata = tokio::fs::metadata(path).await?;
                anyhow::ensure!(
                    metadata.is_file() && metadata.len() <= 65536,
                    "invalid status file"
                );
                let file = tokio::fs::File::open(path).await?;
                let mut text = String::new();
                file.take(65537).read_to_string(&mut text).await?;
                anyhow::ensure!(text.len() <= 65536, "oversized status file");
                apply_status(&mut job, &text, prefix, chrono::Utc::now().timestamp())?;
            }
            anyhow::Ok(job)
        };
        let job = match tokio::time::timeout(Duration::from_secs(5), collected).await {
            Ok(Ok(job)) => job,
            _ => ScheduledJob {
                name: config.name.clone(),
                service: config.service.clone(),
                timer: config.timer.clone(),
                max_interval_seconds: config.max_interval_seconds,
                result: "collection-unavailable".into(),
                ..Default::default()
            },
        };
        jobs.push(job);
    }
    jobs
}
#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> ScheduledJobConfig {
        ScheduledJobConfig {
            name: "backup".into(),
            service: "backup.service".into(),
            timer: "backup.timer".into(),
            max_interval_seconds: 3600,
            status_file: None,
            metric_prefix: None,
        }
    }
    #[test]
    fn ubuntu_timestamps_and_successful_inactive_oneshot() {
        let job=parse(&config(),"LoadState=loaded\nActiveState=inactive\nResult=success\nExecMainStatus=0\nExecMainStartTimestamp=Mon 2026-11-09 10:00:00 UTC\nExecMainExitTimestamp=Mon 2026-11-09 10:00:05 UTC", "LoadState=loaded\nActiveState=active\nUnitFileState=enabled\nNextElapseUSecRealtime=Mon 2026-11-09 11:00:00 UTC");
        assert!(job.collection_up && job.timer_active && !job.running && !job.failed());
        assert_eq!(job.last_run, 1794218400);
        assert_eq!(job.last_success, job.last_run + 5);
        assert_eq!(job.next_run, job.last_run + 3600);
    }
    #[test]
    fn disabled_timer_is_reported_even_if_still_active_until_reboot() {
        let job = parse(
            &config(),
            "LoadState=loaded\nActiveState=inactive\nResult=success\nExecMainStatus=0",
            "LoadState=loaded\nActiveState=active\nUnitFileState=disabled",
        );
        assert!(job.collection_up);
        assert!(!job.timer_active);
    }

    #[test]
    fn absent_unit_is_unavailable_not_healthy() {
        assert!(!parse(&config(), "LoadState=not-found", "LoadState=not-found").collection_up);
    }
    #[test]
    fn durable_failure_survives_systemd_reset_and_bad_files_fail_closed() {
        let mut job = ScheduledJob::default();
        apply_status(&mut job,"task_last_run_timestamp_seconds 100\ntask_last_success_timestamp_seconds 50\ntask_last_exit_code 1", "task", 200).unwrap();
        assert!(job.failed());
        assert_eq!(job.last_success, 50);
        for text in [
            "",
            "task_last_run_timestamp_seconds NaN",
            "task_last_run_timestamp_seconds 999999",
        ] {
            assert!(apply_status(&mut job, text, "task", 200).is_err());
        }
    }
    #[test]
    fn running_does_not_inherit_previous_completion() {
        let mut job = ScheduledJob {
            running: true,
            last_run: 150,
            result: "running".into(),
            ..Default::default()
        };
        apply_status(&mut job,"task_last_run_timestamp_seconds 100\ntask_last_success_timestamp_seconds 50\ntask_last_exit_code 1", "task",200).unwrap();
        assert_eq!(job.last_run, 150);
        assert!(!job.failed());
    }
}
