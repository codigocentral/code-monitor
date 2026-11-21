# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Product page redesigned on the Código Central brand (graphite, lime, Sora,
  Inter and JetBrains Mono), with real screenshots of the terminal dashboard
  taken against demo servers, Open Graph images in English and Portuguese, and
  a mobile layout without horizontal scroll.
- Dependencies: tonic 0.14 and prost 0.14 (hyper 1, h2 0.4), reqwest 0.12,
  rustls 0.23 everywhere with the ring provider, mysql_async 0.37, rusqlite
  0.43, config 0.15, indicatif 0.18, axum 0.8, and `yaml-rust2` in place of
  the unmaintained `yaml-rust` for compose files.

### Security

- `cargo audit` is clean: the update removes the advisories against
  rustls-webpki (certificate name constraints, CRL handling, distrusted
  intermediates), rustls, h2, lru, libsqlite3-sys, webpki-roots,
  tokio-postgres, postgres-protocol, tokio and others. The two remaining
  "unmaintained" notices with no vulnerability, `tui` and `chrono`, are
  ignored with their reasons in `.cargo/audit.toml`.
- The client's `danger_skip_verify` mode still skips the certificate chain
  only: handshake signatures are verified against the presented certificate.

### Fixed

- CI: the lint job installs `protoc`, tests that rely on Unix permission
  bits are Unix-only, and `cargo audit` also runs on every GitLab pipeline.

## [0.2.0] - 2026-11-19

First release under AGPL-3.0-or-later. Version 0.1.0 stays under MIT.

### Added

- Product page in `site/` (English, with a Portuguese version in `site/pt/`),
  deployed to GitHub Pages at codemonitor.codigocentral.com.br by
  `.github/workflows/pages.yml`.

### Changed

- License: from MIT to AGPL-3.0-or-later, starting with this version. Code
  published before (the GitHub snapshot of 2026-11-13) stays under MIT.
  Contributions now require the CLA (`CLA.md`).

### Fixed

- Compose drift: a variable the project's env files do not define makes its
  field unknown even when the reference has a default (`${PORT:-8080}`,
  `${IMAGE:-app:local}`). The default is only what compose uses when the
  deploying shell does not set the variable; deployments driven from CI set
  it in the job's environment, and comparing the default reported drift on
  every such service.
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

- Compose drift: `CODE_MONITOR_COMPOSE_SILENCE` (comma-separated
  `project/service`, `project/*` or `service`) appends to
  `[compose_drift] silence`, so a systemd drop-in can silence projects
  whose compose files never reach the host (deployed from CI through a
  remote `DOCKER_HOST`) without editing `config.toml`.
- Versions collector: newer version on a pinned tag (#13). Digest drift never
  fires for a pinned tag such as `26.1` or `1.27.3-alpine`, so the collector
  now also lists the repository's tags (`GET /v2/<repo>/tags/list`, same
  anonymous token, `Link` pagination) and reports
  `VersionStatus::NewerAvailable` with `latest_tag` when a newer tag exists in
  the same release line: the tag minus its last component (`26.1` → `26.x`,
  `2.4.10` → `2.4.x`, so an LTS line is never left), same shape, same `v`
  prefix and same variant suffix (`-alpine`, `-community`...). A new major is
  never suggested and pre-releases (`-rc`, `-beta`, `-alpha`, `-dev`,
  `nightly`...) are ignored unless `include_prereleases` is set. Floating and
  non-semver tags (`latest`, `stable`, `16`, dates, hashes) keep the digest
  drift behaviour and trigger no tag listing. A failed or rate-limited listing
  turns an up-to-date pinned image into `Unknown` (drift still wins). The tag
  list has its own per-repository cache, 7 days by default and never shorter
  than the digest cache. New `[image_versions]` config section
  (`digest_cache_ttl_hours`, `tag_cache_ttl_hours`, `include_prereleases`,
  `skip_patterns`), so the digest TTL and the skip patterns are configurable
  for the first time. New audit check `image_newer_available` (info, target
  `repository:tag → latest`, with guidance); gRPC carries
  `IMAGE_VERSION_STATUS_NEWER_AVAILABLE` and `latest_tag`; the Containers tab
  shows `⇡ <tag>` next to the image, distinct from the `↑` of drift, and
  counts such images in the risk summary.
- Compose drift: an `.env` the agent may not read is treated like a missing
  one. Interpolated fields stay uncompared instead of the whole project being
  reported as `compose_unreadable`, so the agent can be given the compose
  files without the secrets kept next to them.
- Audit: compose drift (#15). Each running container is compared with the
  docker-compose files named in its labels
  (`com.docker.compose.project.config_files`, `project.working_dir`,
  `service`), merged in label order with `${VAR}` / `${VAR:-default}`
  resolved from the project's `.env` (or the `environment_file` label).
  Compared: image tag, published ports including the bind address, mounts
  (source, target, `:ro`), restart policy, networks / `network_mode`, and
  environment by key presence only — values are never read from the
  container, compared, logged or exported. New checks: `compose_drift`
  (warning), `compose_downgrade_risk` (warning, the file declares an older
  tag than the one running) and `compose_unreadable` (info, file missing,
  unreadable or unsupported: indeterminate, not clean). A divergence is
  reported only when two consecutive audit passes see the same one, so a
  deploy caught between `stop` and `up` is not a finding. Containers without
  compose labels are `undeclared`, not drift. `code_monitor_compose_services{state}`
  counts containers per verdict. Intended divergences are silenced per
  service with `[compose_drift] silence`. YAML is parsed with `yaml-rust`,
  already in the dependency tree through `config`.
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
- GitLab CI: fmt, clippy (native and unit-doubles builds), tests with coverage,
  static `x86_64-unknown-linux-musl` binaries, and upload to the generic
  package registry from `main`

- Fleet audit collectors and predictive alerts (#1–#11):
  - Postgres: `pg_settings` inspection revealing actual source file (`auto.conf` vs `.conf`) (#1)
  - Docker: true `mem_limit` detection distinguishing unlimited containers from host-RAM allocations (#2)
  - Docker: inspect-based real `restart_count` tracking crash-looping containers (#3)
  - Docker: healthcheck failing streak count and last error output classification (#4)
  - Memory: active swap throughput (`si`/`so` page rates) and Linux PSI pressure gauges instead of raw swap percentage (#5)
  - Memory: per-process `VmSwap` tracking and pre-flight `would_exceed_limit_on_swapoff` prediction (#6)
  - systemd: automatic detection and alerting for units in `failed` state (#7)
  - TLS: automated certificate expiration tracking (21d warning / 7d critical) and certbot renewal service health (#8)
  - Network: socket listening scan flagging sensitive datastores bound to `0.0.0.0` (#9)
  - Predictive alerts: machine overcommitment detector comparing promised memory vs actual physical RAM (#10)
  - Database lifecycle: orphan database detection for PostgreSQL and MariaDB tracking idle instances and lifetime transactions (#11)
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
- gRPC API documentation (`docsx/05-TECNICO/API-GRPC.md`)
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
