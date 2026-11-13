//! Read-only disk and scheduled-job monitoring contracts.
use crate::types::DiskInfo;
use serde::{Deserialize, Serialize};

/// Per-filesystem thresholds. Byte thresholds are opt-in, since small boot
/// filesystems must not inherit a data disk's free-space budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DiskThresholds {
    pub warning_percent: f64,
    pub critical_percent: f64,
    pub warning_available_bytes: u64,
    pub critical_available_bytes: u64,
}
impl Default for DiskThresholds {
    fn default() -> Self {
        Self {
            warning_percent: 80.0,
            critical_percent: 90.0,
            warning_available_bytes: 0,
            critical_available_bytes: 0,
        }
    }
}
impl DiskThresholds {
    pub fn validate(&self) -> bool {
        self.warning_percent.is_finite()
            && self.critical_percent.is_finite()
            && self.warning_percent > 0.0
            && self.warning_percent <= self.critical_percent
            && self.critical_percent <= 100.0
            && self.critical_available_bytes <= self.warning_available_bytes
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskMountPolicy {
    pub mount: String,
    #[serde(flatten)]
    pub thresholds: DiskThresholds,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DiskPolicy {
    pub defaults: DiskThresholds,
    pub mounts: Vec<DiskMountPolicy>,
    pub ignored_mounts: Vec<String>,
}
/// Pseudo filesystems are views or memory, not independently reclaimable disks.
pub fn real_disk(disk: &DiskInfo) -> bool {
    disk.total_bytes > 0
        && ![
            "overlay", "tmpfs", "devtmpfs", "squashfs", "nsfs", "proc", "sysfs", "cgroup",
            "cgroup2", "autofs", "ramfs", "tracefs", "debugfs", "efivarfs",
        ]
        .contains(&disk.filesystem_type.as_str())
}
impl DiskPolicy {
    /// At most one severity per mount. Critical pressure suppresses warning.
    pub fn severity(&self, disk: &DiskInfo) -> Option<crate::alerts::AlertSeverity> {
        use crate::alerts::AlertSeverity::{Critical, Warning};
        if !real_disk(disk) || self.ignored_mounts.contains(&disk.mount_point) {
            return None;
        }
        let t = self
            .mounts
            .iter()
            .find(|m| m.mount == disk.mount_point)
            .map(|m| &m.thresholds)
            .unwrap_or(&self.defaults);
        if disk.usage_percent >= t.critical_percent
            || disk.available_bytes <= t.critical_available_bytes
        {
            Some(Critical)
        } else if disk.usage_percent >= t.warning_percent
            || disk.available_bytes <= t.warning_available_bytes
        {
            Some(Warning)
        } else {
            None
        }
    }
}

/// One explicitly monitored systemd timer and its service. The collector never
/// starts, stops, enables or modifies either unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJobConfig {
    pub name: String,
    pub service: String,
    pub timer: String,
    pub max_interval_seconds: u64,
    /// Optional durable Prometheus textfile with <prefix>_last_run_timestamp_seconds,
    /// <prefix>_last_success_timestamp_seconds and <prefix>_last_exit_code.
    pub status_file: Option<String>,
    pub metric_prefix: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub name: String,
    pub service: String,
    pub timer: String,
    pub max_interval_seconds: u64,
    pub collection_up: bool,
    pub timer_active: bool,
    pub running: bool,
    /// Zero is unknown, never a fabricated successful run.
    pub last_run: i64,
    pub last_success: i64,
    pub next_run: i64,
    /// systemd result or a bounded collector error category; never command output.
    pub result: String,
    pub exit_code: i32,
}
impl ScheduledJob {
    pub fn overdue_seconds(&self, now: i64) -> i64 {
        now.saturating_sub(self.last_run)
            .saturating_sub(self.max_interval_seconds as i64)
    }
    pub fn failed(&self) -> bool {
        self.last_run > 0
            && (self.exit_code != 0 || !matches!(self.result.as_str(), "success" | "running"))
    }
}

impl From<ScheduledJob> for crate::proto::monitoring::ScheduledJobInfo {
    fn from(j: ScheduledJob) -> Self {
        Self {
            name: j.name,
            service: j.service,
            timer: j.timer,
            max_interval_seconds: j.max_interval_seconds,
            collection_up: j.collection_up,
            timer_active: j.timer_active,
            running: j.running,
            last_run: j.last_run,
            last_success: j.last_success,
            next_run: j.next_run,
            result: j.result,
            exit_code: j.exit_code,
        }
    }
}
impl From<crate::proto::monitoring::ScheduledJobInfo> for ScheduledJob {
    fn from(j: crate::proto::monitoring::ScheduledJobInfo) -> Self {
        Self {
            name: j.name,
            service: j.service,
            timer: j.timer,
            max_interval_seconds: j.max_interval_seconds,
            collection_up: j.collection_up,
            timer_active: j.timer_active,
            running: j.running,
            last_run: j.last_run,
            last_success: j.last_success,
            next_run: j.next_run,
            result: j.result,
            exit_code: j.exit_code,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn disk(mount: &str, fs: &str, percent: f64, available: u64) -> DiskInfo {
        DiskInfo {
            device: "test".into(),
            mount_point: mount.into(),
            filesystem_type: fs.into(),
            total_bytes: 1000,
            used_bytes: 1000 - available,
            available_bytes: available,
            usage_percent: percent,
        }
    }
    #[test]
    fn disks_include_later_mounts_and_exclude_pseudo() {
        let p = DiskPolicy::default();
        assert!(p.severity(&disk("/boot", "ext4", 20.0, 800)).is_none());
        assert_eq!(
            p.severity(&disk("/data", "ext4", 95.0, 50)),
            Some(crate::alerts::AlertSeverity::Critical)
        );
        assert!(p.severity(&disk("/view", "overlay", 99.0, 1)).is_none());
    }
    #[test]
    fn absolute_budget_only_applies_to_configured_mount() {
        let p = DiskPolicy {
            mounts: vec![DiskMountPolicy {
                mount: "/".into(),
                thresholds: DiskThresholds {
                    warning_available_bytes: 200,
                    critical_available_bytes: 100,
                    ..Default::default()
                },
            }],
            ..Default::default()
        };
        assert_eq!(
            p.severity(&disk("/", "ext4", 50.0, 90)),
            Some(crate::alerts::AlertSeverity::Critical)
        );
        assert!(p.severity(&disk("/boot", "ext4", 50.0, 90)).is_none());
        assert!(!DiskThresholds {
            warning_percent: 95.0,
            ..Default::default()
        }
        .validate());
    }
    #[test]
    fn oneshot_inactive_is_not_failure_and_missing_history_is_unknown() {
        let j = ScheduledJob {
            result: "success".into(),
            last_run: 1000,
            max_interval_seconds: 3600,
            ..Default::default()
        };
        assert!(!j.failed());
        assert!(j.overdue_seconds(4600) <= 0);
        assert!(j.overdue_seconds(4601) > 0);
        assert!(!ScheduledJob::default().failed());
        assert!(ScheduledJob {
            result: "timeout".into(),
            ..j
        }
        .failed());
    }
}
