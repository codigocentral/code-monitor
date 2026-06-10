# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
