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
| `image_newer_available` | info | `repository:tag → latest` (e.g. `keycloak/keycloak:26.1 → 26.4`) | 1 |
| `compose_drift` | warning | `project/service` | number of differences |
| `compose_downgrade_risk` | warning | `project/service` | 1 |
| `compose_unreadable` | info | `project/service` | 1 |

Image findings come from the versions collector. `image_drift` means the tag
itself now points to another digest (pull it again). `image_newer_available`
is for pinned version tags, whose digest never moves: the registry's tag list
has a newer tag in the same release line, i.e. the tag minus its last
component (`26.1` looks at `26.x`, `2.4.10` at `2.4.x`), with the same shape
and variant suffix (`-alpine` only against `-alpine`). New majors and
pre-releases are never suggested by default (`[image_versions]` in the server
config). The target names the suggested tag, so a newer release opens a new
series.

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
- **Compose drift** (`compose_drift`, `compose_downgrade_risk`,
  `compose_unreadable`): each running container is compared with the
  docker-compose files its labels name (`com.docker.compose.project.config_files`,
  `com.docker.compose.project.working_dir`, `com.docker.compose.service`).
  The files are merged in label order (single values replaced, `ports`
  appended, `volumes` replaced by target, `networks` and `environment` by
  key; YAML `<<:` merge keys are honoured) and `${VAR}`, `${VAR:-default}`
  and the other compose forms are resolved from the project's `.env`, or the
  files of the `com.docker.compose.project.environment_file` label. A field
  whose variable has no value and no default is not compared, since the
  shell of whoever ran `compose up` is not knowable. Compared:
  - image: `nginx`, `nginx:latest` and `docker.io/library/nginx:latest` are
    the same; when the file declares an older version of the same image
    (`1.4.0` against `1.5.2`, same suffix; `1.25` against `1.25.3` is a
    floating tag, not older), the next `compose up` is a downgrade:
    `compose_downgrade_risk`. Any other image difference is `compose_drift`.
  - ports, by container port, including the bind address (`127.0.0.1:8080`
    against `0.0.0.0:8080` is drift);
  - mounts, by target: bind source, volume name (`project_volume`, `name:`,
    `external:`), `:ro`. Anonymous volumes that only the container has come
    from the image's `VOLUME` and are ignored; tmpfs is not compared;
  - restart policy; networks, or `network_mode`;
  - environment by **key presence only**: a key the file sets that the
    container lacks, or a key only the container has that its image does not
    define (skipped when the service uses `env_file`, whose keys are not
    read). Values are never read from the container, compared, logged or
    exported;
  - a service no longer in the file while its container runs.

  `${VAR}` is resolved from the project's `.env` (or the `environment_file`
  label's files). A field that uses a variable those files do not define is
  not compared, even with a default (`${PORT:-8080}`): the deploying shell,
  often a CI job, may have set it, and the default says nothing about what
  the container got.

  A divergence (and an unreadable file) is reported only when two
  consecutive passes see exactly the same one, so a container caught between
  `stop` and `up` during a deploy is not a finding. A file the agent cannot
  read (permissions, removed), invalid YAML, `include:` or `extends:` give
  `compose_unreadable`: the service is indeterminate, not clean — give the
  agent's user read access to the project directory. Containers without
  compose labels (`docker run`) are `undeclared`, and `compose run`
  one-offs and stopped containers are not compared. Intended divergences
  are silenced in `config.toml`:

  ```toml
  [compose_drift]
  enabled = true                         # default
  silence = ["shop/worker", "legacy/*"]  # project/service, project/*, or service
  ```

  `CODE_MONITOR_COMPOSE_SILENCE` (comma-separated, same forms) appends to
  that list, so a systemd drop-in can silence projects without editing
  `config.toml`:

  ```ini
  [Service]
  Environment=CODE_MONITOR_COMPOSE_SILENCE=ci-lab/*,ci-release/*
  ```

  The `detail` in `/findings` lists the differences (`ports 80/tcp: declared
  127.0.0.1:8080, running 0.0.0.0:8080`).

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
| `code_monitor_compose_services` | `state` = in_sync, drifted, pending (seen once, awaiting confirmation), unreadable, undeclared (no compose labels), silenced |
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

### Outdated images: one low-priority notice per host

Image findings (`image_drift`, and `image_newer_available` for pinned semver
tags) are `info`: maintenance, not incidents. `CodeMonitorFinding` above
ignores them on purpose, so on their own they only live on `/metrics` and in
the TUI. To have them reach someone without flooding the channel, alert on the
**count per host** rather than on each image, and send it on a slow route:

```yaml
groups:
  - name: code-monitor-maintenance
    rules:
      - alert: CodeMonitorImagesOutdated
        # One alert per host, whatever the number of images behind
        expr: |
          count by (instance) (
            code_monitor_finding{check=~"image_drift|image_newer_available"} == 1
          ) > 0
        # Ride out deploys and registry hiccups; images stay behind for weeks
        for: 6h
        labels:
          severity: info
          category: maintenance
        annotations:
          summary: '{{ $value }} outdated image(s) on {{ $labels.instance }}'
          description: 'See /findings on the host or the Images panel of the TUI.'
```

Alertmanager deduplicates by labels, so the same host does not notify again
every evaluation cycle. Give the maintenance category its own receiver and a
long repeat interval, so it reads as a daily digest and never competes with
service alerts:

```yaml
route:
  routes:
    - matchers: ['category="maintenance"']
      receiver: maintenance-digest
      group_by: ['alertname']   # all hosts in one message
      group_wait: 10m
      group_interval: 24h
      repeat_interval: 7d
```

Silencing:

- images pinned on purpose: add them to `skip_patterns` in the server config
  and they produce no finding at all;
- one host or one image for a while: an Alertmanager silence on
  `alertname="CodeMonitorImagesOutdated"` (plus `instance`);
- switch it off entirely: drop the `code-monitor-maintenance` rule group.
  The findings keep being exported; nothing else depends on the rule.
