# Prometheus metrics and audit findings

`monitor-server` runs every collector on a timer (`--audit-interval`, default
60 s) and exports the latest pass on the health server's `/metrics`, in the
Prometheus text format. Nothing is stored on the agent: Prometheus scrapes,
keeps history and decides, with its own rules, when a finding deserves a page.

```bash
monitor-server --address <grpc-ip> --health-address 127.0.0.1 --health-port 9188 \
               --config /etc/code-monitor/config.toml
monitor-server --version        # 0.1.0 (<git sha>)
```

`/metrics` reveals the host's weak spots (exposed ports, failed units, image
drift), so bind it to loopback or a private interface with `--health-address`
and let a local Prometheus agent scrape it. `--audit-interval 0` turns the audit
off; `--no-health` turns off the whole HTTP server.

## Findings

One series per finding that holds right now:

```text
code_monitor_finding{check="datastore_exposed",severity="warning",target="0.0.0.0:5432"} 1
code_monitor_finding_value{check="datastore_exposed",target="0.0.0.0:5432"} 5432
code_monitor_findings{check="datastore_exposed"} 1
```

- `code_monitor_finding` is always `1`; the series disappears when the finding
  clears. `target` is a stable name (unit, container, certificate,
  `cluster/database`, `address:port`, or `host`), never a PID or a measured value.
- `code_monitor_finding_value` carries the figure behind it (days left, ratio...).
- `code_monitor_findings{check}` counts findings per check, including `0`. It is
  **absent** when the check's source could not be read
  (`code_monitor_audit_source_up{source}` is `0`), so "clean" and "blind" never
  look the same.
- At most 25 targets per check are exported, worst severity first; the count
  is always complete. Label values are capped at 128 characters.

| `check` | Severity | Target | Value |
|---|---|---|---|
| `postgres_alter_system_override` | info | cluster | settings coming from `postgresql.auto.conf` |
| `container_no_memory_limit` | info | container | memory in use (bytes) |
| `container_healthcheck_broken` | info | container | failing streak |
| `container_unhealthy` | warning | running container | failing streak |
| `containers_stopped` | info | `host` | number of exited/dead containers |
| `swap_thrashing` | warning | `host` | pages/s (in + out) |
| `memory_stalled` | warning | `host` | PSI full avg60 (%) |
| `swapoff_would_exceed_limit` | info / warning (container at ≥ 95% of its limit in RAM) | container | bytes over the limit |
| `systemd_unit_failed` | warning / info (`systemd-networkd-wait-online` while the network is online) | unit | 1 |
| `tls_certificate_expiring` | warning (≤ 21 d) / critical (≤ 7 d) | certificate | days left |
| `tls_renewal_broken` | warning | renewal unit | certificates affected |
| `tls_certificate_orphaned` | info | certificate | days left (lineage on disk that nothing serves; never also `tls_certificate_expiring`) |
| `datastore_exposed` | critical (firewall accepts public sources) / warning (firewall unreadable) | `address:port` | port |
| `datastore_bound_all_interfaces` | info | `address:port` | port (bound to all interfaces, firewall drops public sources) |
| `memory_overcommitted` | info / warning (≥ 1.5×) / critical (≥ 3×) only while memory is short | `host` | promised / physical |
| `postgres_database_idle` | info | `cluster/database` | size (bytes) |
| `mariadb_instance_empty` | info | instance | 0 |
| `mariadb_schema_empty` | info | `instance/schema` | 0 |
| `image_drift` | info | `repository:tag` | 1 |

TLS findings are about the certificate in use. For each name the collector
takes what `127.0.0.1:443` serves (local handshake with SNI), then the
`ssl_certificate` paths of a running nginx, then the newest lineage for the
name; every other lineage under the certificate directories is orphaned. A
served certificate with no lineage on disk (Caddy, another ACME client) shows
up with `127.0.0.1:443` as its source and the SNI name as its target.

### Severity criteria

- **Data store ports** (`datastore_exposed`, `datastore_bound_all_interfaces`):
  a database, cache or broker port bound to `0.0.0.0`/`::` is judged against
  the packet filter read with `iptables-save` and `ip6tables-save` (ufw is a
  layer on top of it; both the legacy and nf_tables backends print this
  format). The question is whether *any* public source address would get a new
  TCP connection accepted: private sources (RFC 1918, CGNAT, link-local,
  ULA), loopback and internal interfaces (docker, bridges, wg) do not count;
  the interfaces holding a default route are where the internet arrives. A
  socket on `::` is dual-stack and needs both families. Ports caught by a
  `DNAT` rule (Docker `-p 5432:5432`) are judged on the `FORWARD` path, where
  ufw does not apply and `DOCKER-USER` does. Anything the analysis cannot
  model (rate limits, ipsets, unknown targets) counts as *may accept*, so a
  port can be over-reported but never reported closed when it is open.
  Native nftables tables (not visible to `iptables-save`) are ignored, which
  only ever errs towards "exposed". Reading the rules needs `CAP_NET_ADMIN`,
  which would also allow changing them, so the agent does not hold it: the
  root oneshot `code-monitor-firewall-snapshot.service`, triggered every
  minute by `code-monitor-firewall-snapshot.timer`, writes the output of both
  commands atomically to `/var/lib/code-monitor/firewall/iptables.rules` and
  `ip6tables.rules` (`root:code-monitor`, `0640`, directory `0750`), and the
  agent reads those files (`--firewall-snapshot-dir` to move them). A missing
  file, or one whose mtime is more than 5 minutes old, sets
  `code_monitor_audit_source_up{source="firewall"}` to `0` and exposed ports
  keep the previous, unverified `warning`. An agent running as root falls
  back to running `iptables-save` itself.
- **`systemd_unit_failed`**: every failed unit is a warning, except
  `systemd-networkd-wait-online` (and `@iface` variants) while `networkctl`
  reports the host online now (`Online state: online/partial`, or a routable
  uplink on older systemd). That failure is a boot-time timeout — typically
  the uplink stuck in `routable (configuring)` because the provider keeps
  sending IPv6 router advertisements — and is reported as info with the link
  state in the detail. `networkctl` runs only when such a unit has failed.
- **`container_unhealthy`**: only running containers. A stopped container
  keeps its last health state (blue/green leftovers); stopped and dead
  containers are counted once per host in `containers_stopped` (info).
- **`memory_overcommitted`**: the ratio of promised memory (postgres
  `shared_buffers`, `work_mem × max_connections`, `maintenance_work_mem ×
  autovacuum workers`, container limits, JVM `-Xmx`) to physical memory. It is
  a ceiling that is rarely reached all at once, so on its own it is info. It
  becomes warning (≥ 1.5×) / critical (≥ 3×) only while the host is also short
  of memory: under 10% of RAM available, PSI `some` avg60 ≥ 10%, or paging
  above the thrashing rate (200 pages/s).
- **`swapoff_would_exceed_limit`**: a pre-flight answer for `swapoff` (RAM +
  swap of the container above its limit). Info by default; warning when the
  container already uses ≥ 95% of its limit in RAM, i.e. it lives on swap now.

The `check`, `severity` and `target` labels are unchanged, so rules that
group by `check`/`target` keep working; a severity change only moves the
series between `severity` values.

## Detail and guidance (`/findings`)

`GET /findings` on the health server returns the latest pass as JSON: every
finding with `check`, `severity`, `target`, `value`, a `detail` sentence
explaining the verdict (e.g. "bound to every interface, but the firewall drops
public sources") and `guidance` — why it matters and how to fix it, in pt-BR
(`why_pt_br`, `fix_pt_br`) and English (`why_en`, `fix_en`). Guidance is
static per check and stays out of Prometheus labels; checks without text yet
return `null`.

```bash
curl -s 127.0.0.1:9188/findings | jq '.findings[] | {check, severity, target, detail}'
```

Findings are instantaneous: thrashing or memory stall seen in one pass shows up
in that pass. Use `for:` in the alerting rule to require it to persist.

Crash loops are a rate, so they are not a finding: alert on
`increase(code_monitor_container_restarts_total[15m]) >= 3`.

## Collector metrics

| Metric | Labels |
|---|---|
| `code_monitor_build_info` | `version`, `git_sha` |
| `code_monitor_audit_up`, `code_monitor_audit_last_run_timestamp_seconds`, `code_monitor_audit_duration_seconds` | |
| `code_monitor_audit_source_up` | `source` (`firewall` is `1` only when both iptables families were read) |
| `code_monitor_cpu_usage_percent` | |
| `code_monitor_memory_bytes` | `kind` = total, used, available |
| `code_monitor_swap_bytes` | `kind` = total, used |
| `code_monitor_swap_pages_per_second` | `direction` = in, out |
| `code_monitor_memory_pressure_avg60_percent` | `kind` = some, full |
| `code_monitor_disk_usage_percent`, `code_monitor_disk_available_bytes` | `mount` (pseudo filesystems skipped) |
| `code_monitor_memory_commitment_ratio` | |
| `code_monitor_containers` | `state` |
| `code_monitor_container_restarts_total` | `container` |
| `code_monitor_systemd_failed_units` | |
| `code_monitor_tls_certificate_expiry_days` | `certificate`, `source` (file path or `127.0.0.1:443`), `orphaned` = true, false |
| `code_monitor_listening_ports` | `scope` |
| `code_monitor_postgres_connections`, `code_monitor_mariadb_connections` | `cluster` |
| `code_monitor_grpc_*` | gRPC service counters |

## Example alerting rules

```yaml
groups:
  - name: code-monitor
    rules:
      - alert: CodeMonitorFinding
        expr: code_monitor_finding{severity=~"warning|critical"} == 1
        for: 15m  # the series already carries check, severity and target
        annotations:
          summary: '{{ $labels.check }} on {{ $labels.instance }}: {{ $labels.target }}'
      - alert: CodeMonitorAuditStale
        expr: time() - code_monitor_audit_last_run_timestamp_seconds > 600 or code_monitor_audit_up == 0
        for: 10m
      - alert: CodeMonitorContainerCrashLoop
        expr: increase(code_monitor_container_restarts_total[15m]) >= 3
```
