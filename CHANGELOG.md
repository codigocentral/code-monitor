# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Audit: `datastore_exposed` now reads the packet filter (`iptables-save` /
  `ip6tables-save`, which covers ufw) and is critical only when a public
  source would be accepted, including ports published by Docker through
  `DNAT`, which bypass ufw. A data store bound to every interface behind a
  firewall that drops public sources becomes `datastore_bound_all_interfaces`
  (info). The agent gets no capability for this: a root oneshot unit,
  `code-monitor-firewall-snapshot.service`, run every minute by its `.timer`,
  saves both rule sets atomically to `/var/lib/code-monitor/firewall/`
  (`iptables.rules`, `ip6tables.rules`; `root:code-monitor`, `0640`) and the
  agent reads them (`--firewall-snapshot-dir`). A missing snapshot, or one
  older than 5 minutes, sets `code_monitor_audit_source_up{source="firewall"}`
  to 0 and keeps the previous unverified warning. `iptables-save` is run
  directly only when the agent itself runs as root. `service/linux/`,
  `install.sh` and `scripts/install-remote.sh` install and enable the timer;
  the units, the timer and the agent's units grant no `CAP_NET_ADMIN`. The
  CI package ships both units next to the binaries.
- Audit: a failed `systemd-networkd-wait-online` on a host that `networkctl`
  reports online is info, with the link state in the detail; every other
  failed unit stays a warning.
- Audit: `container_unhealthy` ignores stopped containers (their last health
  state is history); stopped/dead containers are counted once per host in the
  new `containers_stopped` (info).
- Audit: `memory_overcommitted` is info unless the host is also short of
  memory (< 10% available, PSI some ≥ 10% or thrashing);
  `swapoff_would_exceed_limit` is info unless the container already sits at
  ≥ 95% of its limit in RAM.

- TLS: expiry is judged on the certificate in use, not on every file under
  `/etc/letsencrypt/live`. The collector prefers what `127.0.0.1:443` actually
  serves for each name (local handshake with SNI, 2 s timeout), then the
  `ssl_certificate` paths of a running nginx (host or container), then the
  newest lineage per name. Superseded or unused lineages (an old `example.com`
  next to `example.com-0001`, files left behind after moving to Caddy) are
  reported only as `tls_certificate_orphaned` (info), never as
  `tls_certificate_expiring`. Served certificates with no lineage on disk are
  reported with source `127.0.0.1:443`.
- `/metrics`: `code_monitor_tls_certificate_expiry_days` gains `source` and
  `orphaned` labels, so an orphaned lineage and the certificate served for the
  same name no longer produce duplicate series.
- Audit: an unreachable Docker daemon (missing socket, no permission) now
  reports `code_monitor_audit_source_up{source="docker"} 0` instead of a host
  with zero containers and zero container findings.

### Added

- Findings carry a `detail` sentence and `guidance` (why it matters, how to
  fix it, pt-BR and English) for the refined checks; served as JSON on the
  health server's `/findings`. `/metrics` labels are unchanged.
- `code_monitor_audit_source_up{source="firewall"}`.

- Server `/metrics` exports collector figures and every audit finding as
  Prometheus series (`code_monitor_finding{check,severity,target}`), refreshed by
  a periodic audit pass (`--audit-interval`, default 60 s). Findings no longer
  depend on an open dashboard. See `docs/prometheus-metrics.md`.
- `--health-address` to bind the health/metrics server to a specific interface
- `--version` on server and client, with the git revision embedded at build time
- Audit collectors and predictive alerts:
  - Postgres: `pg_settings` inspection revealing actual source file (`auto.conf` vs `.conf`)
  - Docker: true `mem_limit` detection distinguishing unlimited containers from host-RAM allocations
  - Docker: inspect-based real `restart_count` tracking crash-looping containers
  - Docker: healthcheck failing streak count and last error output classification
  - Memory: active swap throughput (`si`/`so` page rates) and Linux PSI pressure gauges instead of raw swap percentage
  - Memory: per-process `VmSwap` tracking and pre-flight `would_exceed_limit_on_swapoff` prediction
  - systemd: automatic detection and alerting for units in `failed` state
  - TLS: automated certificate expiration tracking (21d warning / 7d critical) and certbot renewal service health
  - Network: socket listening scan flagging sensitive datastores bound to `0.0.0.0`
  - Predictive alerts: machine overcommitment detector comparing promised memory vs actual physical RAM
  - Database lifecycle: orphan database detection for PostgreSQL and MariaDB tracking idle instances and lifetime transactions
- Versions collector: lightweight detection of container image drift (local digest vs remote registry digest) for floating tags (`latest`), with 24h TTL caching and TUI indicators (`·` up to date, `↑` drifted)
- Static binary compilation using `rustls` throughout to remove glibc dynamic dependencies
- Remote installation script (`scripts/install-remote.sh`) and database collector provisioning scripts
- Config validation on load: interval, max clients, log level, cluster ports and TLS paths are checked with descriptive errors
- Graceful shutdown on SIGINT/SIGTERM for the gRPC server
- gzip compression for gRPC traffic (server and client)
- Optional structured JSON logging on the server (`--log-json` or `CODE_MONITOR_LOG_JSON=1`)
- `max_clients` is now enforced: streaming connections beyond the limit are rejected with `RESOURCE_EXHAUSTED`
- Alert silencing: alerts can be muted per server/type for a given duration (`AlertManager::silence_alert`)
- Headless monitoring mode: `monitor-client monitor --format json` emits one JSON document per sample
- `.dockerignore` to keep images lean and free of local secrets
- `CONTRIBUTING.md` with development and PR guidelines
- Third-party GitHub Actions inventory (`.github/THIRD_PARTY_ACTIONS.md`)

### Changed

- Access tokens are now generated with the OS cryptographic RNG (`OsRng`, 32 bytes)
- Server metrics state uses `RwLock` instead of `Mutex` for concurrent reads
- gRPC server requests have a 30s timeout and HTTP/2 keepalive
- Client config file is written with `0600` permissions on Unix (it stores access tokens)
- `generate-certs.sh` now runs with `set -euo pipefail`
- `Cargo.lock` is committed for reproducible binary builds
- Dockerfiles no longer reference the non-existent `shared/Cargo.lock` and stub
  out unused workspace members so images build again

### Fixed

- `shared/build.rs` no longer panics with `.expect()`; proto compilation errors are propagated with context
- Process filter parameter is validated (max 256 chars) and rejected with `INVALID_ARGUMENT`

## [0.1.0] - 2026-05-21

### Added

- TUI dashboard client (`monitor-client`) with multi-server support
- gRPC monitoring server (`monitor-server`) with token authentication
- TLS and mTLS support for client–server communication
- Collectors: system (CPU/memory/disk/network), processes, Docker containers,
  PostgreSQL clusters, MariaDB clusters, systemd units
- Alerts engine with Slack, Discord and generic webhook notifications
- SQLite metrics history with CSV/JSON export, purge and storage stats
- Health check HTTP endpoints (`/health`, `/ready`, `/metrics`)
- Interactive onboarding (`init` commands) for server and client
- Cross-platform installers (`install.sh`, `install.ps1`) and Docker images
- CI (fmt, clippy, tests on Linux/Windows, coverage, cargo-audit) and
  automated multi-target releases via GitHub Actions
