# Disk pressure and scheduled jobs

The server's periodic audit runs independently of the dashboard. `/metrics`
exports findings; `/findings` includes their detail and the latest job evidence.
Both endpoints use the HTTP listener's existing network boundary: keep it on
loopback or a trusted interface. Authenticated `GetSystemdInfo` adds scheduled
jobs; the Systemd dashboard tab shows state, last attempt, last success and next
run. An older agent is shown as requiring an upgrade, not as healthy.

Disk pressure evaluates every real filesystem, excluding memory and pseudo
filesystems. Defaults are warning at 80% and critical at 90%. Configure
`disk_policy.defaults`, exact `disk_policy.mounts` overrides and
`disk_policy.ignored_mounts` in TOML. Free-byte thresholds default to zero and
should be set only for mounts that need a space budget. Critical suppresses
warning for the same mount. `disk_pressure` identifies its mount as `target`.
The client percentage alert uses the fullest real disk, even when it is not
first in the response. Server findings also account for configured free bytes.

Each `[[scheduled_jobs]]` entry names a systemd service and timer, plus the
maximum interval between attempts including scheduler jitter and acceptable
runtime. Collection only reads `systemctl show`, with bounded timeouts and no
privileged execution. Up to 25 explicit jobs may be configured. This monitors
systemd timers; migrate cron jobs to timers or export their status separately.

A successful oneshot may be inactive. That is normal. The following checks are
critical: `scheduled_job_failed`, `scheduled_job_overdue`,
`scheduled_job_disabled`, and `scheduled_job_unavailable`. Missing or unreadable
units/status files are unknown, never success. A never-run job is overdue.
An execution that outlasts its allowed interval also becomes overdue. Next-run
zero means unknown (for example, an inactive timer or a monotonic timer without
a wall-clock deadline). Timestamps are UTC Unix seconds; zero means unknown.

Systemd retains limited execution history across failures/reboots. For durable
last-success and last-completion evidence configure `status_file` and
`metric_prefix` together. The file must contain scalar numeric metrics named
`<prefix>_last_run_timestamp_seconds`, `<prefix>_last_success_timestamp_seconds`
and `<prefix>_last_exit_code`. Last-run is the completion/attempt timestamp;
last-success must not exceed it; zero means no known success. Failure codes
range 1–255. Write files atomically, retain last-success on failure, and publish
from an independent systemd ExecStopPost hook to catch crashes and timeouts.
The collector combines this with live timer/service state. It does not execute
the job, erase data, clear failure state or persist secrets. Files must be
readable by the agent and fit within 64 KiB.

`code_monitor_scheduled_jobs_configured` gives the count. Per-job gauges use a
stable `job_name` label: `collection_up`, `timer_active`, `running`,
`last_run_timestamp_seconds`, `last_success_timestamp_seconds`,
`next_run_timestamp_seconds`, `last_exit_code`, and `max_interval_seconds`, each
prefixed with `code_monitor_scheduled_job_`. When evidence is unavailable only
`collection_up=0` is emitted; other gauges are absent, not fake zeroes.

See `examples/disk-job-alerts.yml` for Prometheus rules. The agent reports
conditions; Prometheus owns duration, notification routing and deduplication.
If a generic rule already consumes `code_monitor_finding`, exclude the new
checks there before installing specific rules, to avoid duplicate pages. Keep
an external expected-host/job inventory: absence of all agent metrics must
still trigger a monitoring-unavailable alert. Successful command exit does not
prove a backup can be restored; job-specific outcome checks remain necessary.
