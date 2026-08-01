//! Memory pressure collector
//!
//! Swap occupancy is a historical high-water mark, not a measure of pressure:
//! pages parked on disk during an old spike cost nothing until something
//! touches them. A host can sit at 65% swap and be perfectly healthy while
//! another at 55% is thrashing. What distinguishes them is the paging *rate*
//! and the time tasks spend stalled waiting for memory.
//!
//! Reads `/proc/vmstat` and `/proc/pressure/memory`, plus `VmSwap` per process
//! so the question "what comes back if I clear swap?" can be answered before
//! running `swapoff` rather than after.

use chrono::{DateTime, Utc};
use shared::types::MemoryPressure;
use std::collections::HashMap;
use std::path::Path;
use tracing::debug;

/// Raw paging counters, monotonic since boot
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagingCounters {
    pub pages_in: u64,
    pub pages_out: u64,
}

/// Paging counters plus when they were read, so a rate can be derived
#[derive(Debug, Clone, Copy)]
struct CounterSample {
    counters: PagingCounters,
    observed_at: DateTime<Utc>,
}

/// Tracks paging counters between polls to turn them into a rate
#[derive(Debug, Default)]
pub struct SwapActivityTracker {
    previous: Option<CounterSample>,
}

/// Parse `pswpin` and `pswpout` out of `/proc/vmstat`.
pub fn parse_vmstat(contents: &str) -> Option<PagingCounters> {
    let mut pages_in = None;
    let mut pages_out = None;

    for line in contents.lines() {
        let mut parts = line.split_whitespace();
        match (parts.next(), parts.next()) {
            (Some("pswpin"), Some(value)) => pages_in = value.parse().ok(),
            (Some("pswpout"), Some(value)) => pages_out = value.parse().ok(),
            _ => {}
        }
    }

    Some(PagingCounters {
        pages_in: pages_in?,
        pages_out: pages_out?,
    })
}

/// Parse `/proc/pressure/memory`.
///
/// `total` is microseconds of cumulative stall; it is converted to seconds
/// because nobody reasons in microseconds about a host that has been up for
/// months.
pub fn parse_pressure(contents: &str) -> Option<MemoryPressure> {
    let mut some_avg60 = None;
    let mut full_avg60 = None;
    let mut full_total_micros = None;

    for line in contents.lines() {
        let mut parts = line.split_whitespace();
        let kind = parts.next()?;

        for field in parts {
            let (key, value) = match field.split_once('=') {
                Some(pair) => pair,
                None => continue,
            };

            match (kind, key) {
                ("some", "avg60") => some_avg60 = value.parse::<f64>().ok(),
                ("full", "avg60") => full_avg60 = value.parse::<f64>().ok(),
                ("full", "total") => full_total_micros = value.parse::<f64>().ok(),
                _ => {}
            }
        }
    }

    Some(MemoryPressure {
        some_avg60: some_avg60?,
        // A kernel reporting `some` but not `full` still says something useful
        full_avg60: full_avg60.unwrap_or(0.0),
        full_total_seconds: full_total_micros.unwrap_or(0.0) / 1_000_000.0,
    })
}

/// Parse `VmSwap` from `/proc/[pid]/status`, in bytes.
///
/// Absent for kernel threads and on kernels without swap accounting, which is
/// not the same as zero.
pub fn parse_vm_swap(status: &str) -> Option<u64> {
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmSwap:") {
            let mut parts = rest.split_whitespace();
            let value: u64 = parts.next()?.parse().ok()?;
            // The kernel always prints kB here
            return Some(value * 1024);
        }
    }
    None
}

/// Extract a container id from a `/proc/[pid]/cgroup` line.
///
/// Handles the shapes Docker produces under both cgroup versions:
/// `/docker/<id>`, `/system.slice/docker-<id>.scope`, and the kubepods
/// variants that embed the same 64-character hex id.
pub fn parse_container_id(cgroup: &str) -> Option<String> {
    for line in cgroup.lines() {
        let path = line.rsplit(':').next()?;

        for segment in path.split('/') {
            let candidate = segment
                .strip_prefix("docker-")
                .and_then(|s| s.strip_suffix(".scope"))
                .unwrap_or(segment);

            if candidate.len() == 64 && candidate.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

impl SwapActivityTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a counter reading and return the rate since the previous one.
    ///
    /// The first reading has nothing to compare against and yields zero: a
    /// counter that has been climbing since boot says nothing about now.
    /// Counters going backwards mean a reboot, and also yield zero rather than
    /// a nonsensical negative.
    pub fn observe(&mut self, counters: PagingCounters, now: DateTime<Utc>) -> (f64, f64) {
        let previous = match self.previous.replace(CounterSample {
            counters,
            observed_at: now,
        }) {
            Some(previous) => previous,
            None => return (0.0, 0.0),
        };

        let elapsed = (now - previous.observed_at).num_milliseconds() as f64 / 1000.0;
        if elapsed <= 0.0 {
            return (0.0, 0.0);
        }

        let rate = |current: u64, before: u64| {
            if current < before {
                0.0
            } else {
                (current - before) as f64 / elapsed
            }
        };

        (
            rate(counters.pages_in, previous.counters.pages_in),
            rate(counters.pages_out, previous.counters.pages_out),
        )
    }
}

/// Collector for swap activity and memory pressure
pub struct MemoryCollector {
    proc_root: String,
}

impl MemoryCollector {
    pub fn new() -> Self {
        Self {
            proc_root: "/proc".to_string(),
        }
    }

    #[allow(dead_code)]
    pub fn with_proc_root(proc_root: &str) -> Self {
        Self {
            proc_root: proc_root.to_string(),
        }
    }

    /// Read the paging counters, if the platform exposes them.
    pub fn paging_counters(&self) -> Option<PagingCounters> {
        let path = Path::new(&self.proc_root).join("vmstat");
        match std::fs::read_to_string(&path) {
            Ok(contents) => parse_vmstat(&contents),
            Err(e) => {
                debug!("Cannot read {}: {}", path.display(), e);
                None
            }
        }
    }

    /// Read memory pressure, if the kernel was built with `CONFIG_PSI`.
    pub fn pressure(&self) -> Option<MemoryPressure> {
        let path = Path::new(&self.proc_root).join("pressure/memory");
        match std::fs::read_to_string(&path) {
            Ok(contents) => parse_pressure(&contents),
            Err(e) => {
                debug!("Cannot read {}: {}", path.display(), e);
                None
            }
        }
    }

    /// Swap held by a single process.
    pub fn process_swap(&self, pid: u32) -> Option<u64> {
        let path = Path::new(&self.proc_root)
            .join(pid.to_string())
            .join("status");
        std::fs::read_to_string(path)
            .ok()
            .and_then(|status| parse_vm_swap(&status))
    }

    /// Swap held by each container, summed over its processes.
    ///
    /// This is what makes a `swapoff` pre-flight possible: compared against a
    /// container's memory limit, it says which one the kernel would kill.
    pub fn swap_by_container(&self) -> HashMap<String, u64> {
        let mut totals: HashMap<String, u64> = HashMap::new();

        let entries = match std::fs::read_dir(&self.proc_root) {
            Ok(entries) => entries,
            Err(e) => {
                debug!("Cannot enumerate {}: {}", self.proc_root, e);
                return totals;
            }
        };

        for entry in entries.filter_map(|e| e.ok()) {
            if entry.file_name().to_string_lossy().parse::<u32>().is_err() {
                continue; // Not a process directory
            }

            let swap = match std::fs::read_to_string(entry.path().join("status"))
                .ok()
                .and_then(|status| parse_vm_swap(&status))
            {
                Some(swap) if swap > 0 => swap,
                _ => continue, // Nothing paged out, or the process just exited
            };

            if let Some(container) = std::fs::read_to_string(entry.path().join("cgroup"))
                .ok()
                .and_then(|cgroup| parse_container_id(&cgroup))
            {
                *totals.entry(container).or_insert(0) += swap;
            }
        }

        totals
    }
}

impl Default for MemoryCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VMSTAT: &str = "nr_free_pages 1234567
pgpgin 98765
pswpin 39496
pswpout 497978
pgfault 12345678
";

    const PRESSURE: &str = "some avg10=0.00 avg60=1.23 avg300=0.41 total=10966867
full avg10=0.00 avg60=0.75 avg300=0.20 total=10507701
";

    // ─────────────────────────────────────────
    // vmstat
    // ─────────────────────────────────────────

    #[test]
    fn test_parse_vmstat() {
        let counters = parse_vmstat(VMSTAT).unwrap();
        assert_eq!(counters.pages_in, 39_496);
        assert_eq!(counters.pages_out, 497_978);
    }

    #[test]
    fn test_parse_vmstat_without_swap_counters() {
        assert!(parse_vmstat("nr_free_pages 100\npgfault 200\n").is_none());
    }

    #[test]
    fn test_parse_vmstat_empty() {
        assert!(parse_vmstat("").is_none());
    }

    #[test]
    fn test_parse_vmstat_ignores_similar_keys() {
        // pgpgin must not be mistaken for pswpin
        let counters = parse_vmstat("pgpgin 1\npswpin 2\npswpout 3\n").unwrap();
        assert_eq!(counters.pages_in, 2);
    }

    // ─────────────────────────────────────────
    // pressure
    // ─────────────────────────────────────────

    #[test]
    fn test_parse_pressure() {
        let pressure = parse_pressure(PRESSURE).unwrap();
        assert_eq!(pressure.some_avg60, 1.23);
        assert_eq!(pressure.full_avg60, 0.75);
    }

    #[test]
    fn test_parse_pressure_converts_total_to_seconds() {
        // 10,507,701 microseconds is about 10.5 seconds, not 10 million of
        // anything a human would read
        let pressure = parse_pressure(PRESSURE).unwrap();
        assert!((pressure.full_total_seconds - 10.507701).abs() < 0.000_001);
    }

    #[test]
    fn test_parse_pressure_without_full_line() {
        // Some kernels report only `some`; that is still worth having
        let pressure =
            parse_pressure("some avg10=0.00 avg60=2.50 avg300=0.00 total=100\n").unwrap();
        assert_eq!(pressure.some_avg60, 2.5);
        assert_eq!(pressure.full_avg60, 0.0);
    }

    #[test]
    fn test_parse_pressure_garbage() {
        assert!(parse_pressure("not pressure data").is_none());
        assert!(parse_pressure("").is_none());
    }

    // ─────────────────────────────────────────
    // VmSwap
    // ─────────────────────────────────────────

    #[test]
    fn test_parse_vm_swap_converts_kb_to_bytes() {
        let status = "Name:\tredis-server\nVmRSS:\t  102400 kB\nVmSwap:\t 1135616 kB\n";
        assert_eq!(parse_vm_swap(status), Some(1_135_616 * 1024));
    }

    #[test]
    fn test_parse_vm_swap_zero() {
        assert_eq!(parse_vm_swap("VmSwap:\t       0 kB\n"), Some(0));
    }

    #[test]
    fn test_parse_vm_swap_absent_is_none() {
        // Kernel threads have no VmSwap line at all — unknown, not zero
        assert_eq!(parse_vm_swap("Name:\tkthreadd\nState:\tS\n"), None);
    }

    #[test]
    fn test_parse_vm_swap_does_not_match_vmrss() {
        assert_eq!(parse_vm_swap("VmRSS:\t  102400 kB\n"), None);
    }

    // ─────────────────────────────────────────
    // Container ids
    // ─────────────────────────────────────────

    #[test]
    fn test_parse_container_id_cgroup_v2_scope() {
        let cgroup = "0::/system.slice/docker-3f4e5a6b7c8d9e0f1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7081.scope\n";
        assert_eq!(
            parse_container_id(cgroup).as_deref(),
            Some("3f4e5a6b7c8d9e0f1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7081")
        );
    }

    #[test]
    fn test_parse_container_id_cgroup_v1() {
        let cgroup =
            "12:memory:/docker/3f4e5a6b7c8d9e0f1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7081\n";
        assert_eq!(
            parse_container_id(cgroup).as_deref(),
            Some("3f4e5a6b7c8d9e0f1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7081")
        );
    }

    #[test]
    fn test_parse_container_id_matches_a_real_docker_cgroup() {
        // Copied verbatim from a running container on a cgroup v2 host, so the
        // parser is pinned to the shape docker actually produces rather than
        // the one documented.
        let cgroup = "0::/system.slice/docker-059507be3465cc63cd56d5808c42c573793559ca8f0adeb1e453a5d66ab43e5a.scope\n";
        assert_eq!(
            parse_container_id(cgroup).as_deref(),
            Some("059507be3465cc63cd56d5808c42c573793559ca8f0adeb1e453a5d66ab43e5a")
        );
    }

    #[test]
    fn test_parse_container_id_host_process() {
        let cgroup = "0::/user.slice/user-1000.slice/session-2.scope\n";
        assert!(parse_container_id(cgroup).is_none());
    }

    #[test]
    fn test_parse_container_id_rejects_short_hex() {
        assert!(parse_container_id("0::/docker/abc123\n").is_none());
    }

    // ─────────────────────────────────────────
    // Rate derivation
    // ─────────────────────────────────────────

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_785_600_000 + seconds, 0).unwrap()
    }

    #[test]
    fn test_first_observation_has_no_rate() {
        // Counters climbing since boot say nothing about right now
        let mut tracker = SwapActivityTracker::new();
        let rate = tracker.observe(
            PagingCounters {
                pages_in: 4_600_000,
                pages_out: 10_600_000,
            },
            at(0),
        );
        assert_eq!(rate, (0.0, 0.0));
    }

    #[test]
    fn test_rate_between_observations() {
        let mut tracker = SwapActivityTracker::new();
        tracker.observe(
            PagingCounters {
                pages_in: 1_000,
                pages_out: 2_000,
            },
            at(0),
        );

        // 500 in and 1000 out over 5 seconds
        let (rate_in, rate_out) = tracker.observe(
            PagingCounters {
                pages_in: 1_500,
                pages_out: 3_000,
            },
            at(5),
        );

        assert_eq!(rate_in, 100.0);
        assert_eq!(rate_out, 200.0);
    }

    #[test]
    fn test_idle_host_reports_zero_rate() {
        // alemanha8: 65% swap occupancy and no paging at all
        let mut tracker = SwapActivityTracker::new();
        let counters = PagingCounters {
            pages_in: 4_600_000,
            pages_out: 10_600_000,
        };
        tracker.observe(counters, at(0));
        assert_eq!(tracker.observe(counters, at(5)), (0.0, 0.0));
    }

    #[test]
    fn test_counters_going_backwards_yield_zero() {
        // A reboot resets them; a negative rate would be nonsense
        let mut tracker = SwapActivityTracker::new();
        tracker.observe(
            PagingCounters {
                pages_in: 1_000_000,
                pages_out: 2_000_000,
            },
            at(0),
        );
        assert_eq!(
            tracker.observe(
                PagingCounters {
                    pages_in: 10,
                    pages_out: 20
                },
                at(5)
            ),
            (0.0, 0.0)
        );
    }

    #[test]
    fn test_same_instant_yields_zero() {
        let mut tracker = SwapActivityTracker::new();
        let counters = PagingCounters {
            pages_in: 100,
            pages_out: 200,
        };
        tracker.observe(counters, at(0));
        assert_eq!(
            tracker.observe(
                PagingCounters {
                    pages_in: 500,
                    pages_out: 900
                },
                at(0)
            ),
            (0.0, 0.0),
            "dividing by a zero interval must not produce infinity"
        );
    }

    // ─────────────────────────────────────────
    // Collection
    // ─────────────────────────────────────────

    #[test]
    fn test_collector_on_missing_proc() {
        let collector = MemoryCollector::with_proc_root("/nonexistent/proc");
        assert!(collector.paging_counters().is_none());
        assert!(collector.pressure().is_none());
        assert!(collector.process_swap(1).is_none());
        assert!(collector.swap_by_container().is_empty());
    }

    #[test]
    fn test_collector_reads_a_synthetic_proc() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("vmstat"), VMSTAT).unwrap();
        std::fs::create_dir(root.path().join("pressure")).unwrap();
        std::fs::write(root.path().join("pressure/memory"), PRESSURE).unwrap();

        let collector = MemoryCollector::with_proc_root(&root.path().to_string_lossy());
        assert_eq!(collector.paging_counters().unwrap().pages_in, 39_496);
        assert_eq!(collector.pressure().unwrap().some_avg60, 1.23);
    }

    #[test]
    fn test_swap_by_container_sums_processes() {
        let root = tempfile::tempdir().unwrap();
        let container =
            "3f4e5a6b7c8d9e0f1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7081".to_string();

        for (pid, swap_kb) in [("100", 1024), ("101", 2048)] {
            let dir = root.path().join(pid);
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(dir.join("status"), format!("VmSwap:\t{} kB\n", swap_kb)).unwrap();
            std::fs::write(
                dir.join("cgroup"),
                format!("0::/system.slice/docker-{}.scope\n", container),
            )
            .unwrap();
        }

        // A host process holding swap must not be attributed to the container
        let host = root.path().join("200");
        std::fs::create_dir(&host).unwrap();
        std::fs::write(host.join("status"), "VmSwap:\t9999 kB\n").unwrap();
        std::fs::write(host.join("cgroup"), "0::/user.slice/session-2.scope\n").unwrap();

        let totals =
            MemoryCollector::with_proc_root(&root.path().to_string_lossy()).swap_by_container();

        assert_eq!(totals.len(), 1);
        assert_eq!(totals.get(&container), Some(&((1024 + 2048) * 1024)));
    }

    #[test]
    fn test_swap_by_container_skips_processes_without_swap() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("100");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("status"), "VmSwap:\t0 kB\n").unwrap();
        std::fs::write(
            dir.join("cgroup"),
            "0::/system.slice/docker-3f4e5a6b7c8d9e0f1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7081.scope\n",
        )
        .unwrap();

        assert!(
            MemoryCollector::with_proc_root(&root.path().to_string_lossy())
                .swap_by_container()
                .is_empty()
        );
    }
}
