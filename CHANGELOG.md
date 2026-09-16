# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
