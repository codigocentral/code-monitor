# Code Monitor - Agent Guide

## Project Overview

Code Monitor is a **high-performance, cross-platform system monitoring application** built in Rust. It uses a client-server architecture where `monitor-server` collects system metrics on target machines and `monitor-client` displays them via a rich Terminal User Interface (TUI) dashboard. Communication happens over gRPC (optionally with TLS/mTLS).

The project originated from a Portuguese requirements document (`necessidade.md`), but **all code, comments, and documentation are in English**.

### Architecture

```
┌─────────────────┐    gRPC/HTTP2    ┌─────────────────┐
│   monitor-      │◄────────────────►│   monitor-      │
│   client        │   Port 50051     │   server        │
│   (TUI CLI)     │                  │   (Data Collector│
│                 │                  │   & Auth)        │
└─────────────────┘                  └─────────────────┘
         │                                  │
         │ SQLite storage                   │ HTTP 8080
         ▼                                  ▼
    code-monitor.db                    /health /ready /metrics
```

## Technology Stack

- **Language**: Rust (Edition 2021)
- **Workspace**: Cargo workspace with 3 crates (`shared`, `server`, `client`)
- **Async Runtime**: Tokio
- **RPC Framework**: gRPC via `tonic` (protobuf compiled with `prost` / `tonic-build`)
- **TUI Framework**: `tui-rs` + `crossterm`
- **System Info**: `sysinfo` crate
- **HTTP Health Server**: `axum` (server health checks on port 8080)
- **Docker API**: `bollard`
- **Postgres**: `tokio-postgres`
- **MariaDB**: `mysql_async`
- **systemd**: `systemctl` parsing (Linux only)
- **Authentication**: Simple random base64 access tokens passed in gRPC metadata (`x-access-token` or `Authorization: Bearer`)
- **TLS**: `rustls` 0.21/0.22, `tokio-rustls` 0.24 — optional server-side TLS and mTLS
- **Client Storage**: `rusqlite` (bundled) for historical metrics
- **Serialization**: `serde`, `toml`, `bincode`
- **Error Handling**: `anyhow` for application errors, `thiserror` for structured errors in `shared`
- **Logging**: `tracing`, `tracing-subscriber`
- **CLI Parsing**: `clap` (derive macros)

## Build Commands

### Prerequisites

- Rust 1.75+ with Cargo
- Protocol Buffers compiler (`protoc`)
- Linux, macOS, or Windows (cross-platform supported)

### Build Everything

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Or use the Makefile
make build          # debug
make build-release  # release
```

### Build Individual Components

```bash
cd shared && cargo build --release
cd server && cargo build --release
cd client && cargo build --release
```

### Output Binaries

After building:
- Server: `target/release/monitor-server`
- Client: `target/release/monitor-client`

## Running the Application

### Server (runs on the system to be monitored)

```bash
# Start the monitoring server
./target/release/monitor-server --address 0.0.0.0 --port 50051

# Subcommands
./target/release/monitor-server init                # Interactive config setup
./target/release/monitor-server show-token          # Display access token
./target/release/monitor-server new-token           # Generate new token
./target/release/monitor-server list-clients        # List authorized clients
./target/release/monitor-server remove-client --name <client>
./target/release/monitor-server disable-auth
./target/release/monitor-server enable-auth
```

The server also starts an HTTP health check server on port `8080` (unless `--no-health` is passed) with endpoints:
- `GET /health` — basic health status
- `GET /ready` — readiness probe
- `GET /metrics` — Prometheus-compatible metrics

### Client (runs on the monitoring workstation)

```bash
# Open interactive dashboard (default when no command given)
./target/release/monitor-client

# Quick connect to a new server
./target/release/monitor-client connect 192.168.0.31 --port 50051

# Add a server manually
./target/release/monitor-client add \
    --name "Linux Server" \
    --address 192.168.0.31 \
    --port 50051 \
    --token "<access-token-from-server>"

# List configured servers
./target/release/monitor-client list

# Update token for a server
./target/release/monitor-client set-token --id <uuid> --token "new-token"

# List services / processes
./target/release/monitor-client services --server <uuid>
./target/release/monitor-client processes --server <uuid> --filter <pattern>

# Export stored metrics
./target/release/monitor-client export --format csv --output metrics.csv --hours 24

# Storage management
./target/release/monitor-client storage-stats
./target/release/monitor-client purge --days 30
```

## Configuration Files

### Server Configuration (`config.toml`)

```toml
update_interval_seconds = 5
max_clients = 100
enable_authentication = true
log_level = "info"
# access_token = "auto-generated-token"

# [tls]
# cert_path = "/etc/code-monitor/server.crt"
# key_path = "/etc/code-monitor/server.key"
# ca_path = "/etc/code-monitor/ca.crt"   # enables mTLS
```

Interactive setup: `monitor-server --config config.toml init`

### Client Configuration (`client-config.toml`)

```toml
update_interval_seconds = 5
auto_reconnect = true
reconnect_delay_seconds = 5

[[servers]]
id = "uuid-here"
name = "Linux Server"
address = "192.168.0.31"
port = 50051
description = "Main production server"
access_token = "token-from-server"

# [servers.tls]
# ca_cert_path = "/path/to/ca.crt"
# client_cert_path = "/path/to/client.crt"
# client_key_path = "/path/to/client.key"
```

Interactive setup: `monitor-client --config client-config.toml init`

## Code Organization

### Shared Library (`shared/`)

| File | Purpose |
|------|---------|
| `src/lib.rs` | Error types (`MonitorError`), data structures (`SystemInfo`, `ProcessInfo`, `ServiceInfo`, `NetworkInfo`, `ContainerInfo`, `PostgresClusterInfo`, `MariaDBClusterInfo`, `SystemdUnitInfo`, etc.) |
| `src/proto/monitoring.proto` | gRPC service definition and Protocol Buffer messages |
| `src/alerts.rs` | Alert system: `AlertManager`, `AlertRule`, `AlertType`, `AlertSeverity` — threshold-based detection with cooldown and history |
| `src/notifications.rs` | Notification channels: `WebhookChannel`, `SlackChannel`, `DiscordChannel`, `EmailChannel` — async trait-based delivery |
| `build.rs` | Compiles `.proto` files using `tonic-build` |

### Server (`server/`)

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point, CLI argument parsing (`clap`), subcommand handling, server startup banner |
| `src/config.rs` | Configuration management (TOML), access token generation, authorized clients list, Postgres/MariaDB cluster configs, TLS config |
| `src/monitor.rs` | System information collection using `sysinfo` (CPU, memory, disk, processes, services, network); background update task; collector orchestration |
| `src/service.rs` | gRPC service implementation (`MonitorServiceImpl`) — validates auth tokens and serves all RPC methods |
| `src/health.rs` | HTTP health check server (`axum`) — `/health`, `/ready`, `/metrics` endpoints |
| `src/tls.rs` | TLS identity loader and availability checker for gRPC server |
| `src/collectors/mod.rs` | Collector trait and orchestration |
| `src/collectors/docker.rs` | Docker container metrics via `bollard` |
| `src/collectors/postgres.rs` | Postgres cluster metrics via `tokio-postgres` |
| `src/collectors/mariadb.rs` | MariaDB cluster metrics via `mysql_async` |
| `src/collectors/systemd.rs` | systemd unit status parsing via `systemctl` |

### Client (`client/`)

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point, command dispatch, interactive prompts, monitoring loop, export logic |
| `src/cli.rs` | CLI argument definitions using `clap` (`CliArgs`, `Commands`) |
| `src/client.rs` | gRPC client implementation (`MonitorClient`) with connection management, token injection, and all RPC wrappers |
| `src/config.rs` | Client configuration management — add/remove/list servers, TOML persistence |
| `src/dashboard.rs` | Main TUI dashboard state machine (`DashboardApp`) — event loop, tab navigation, server list, keyboard handling |
| `src/dashboard/data.rs` | Data fetching and connection logic for the dashboard |
| `src/dashboard/render.rs` | Top-level UI rendering coordinator |
| `src/dashboard/render/tabs.rs` | Tab content rendering (Overview, Services, Processes, Network, Containers, Postgres, MariaDB, Systemd) |
| `src/dashboard/render/processes.rs` | Process table and filter rendering |
| `src/dashboard/render/databases.rs` | Postgres and MariaDB table rendering |
| `src/dashboard/render/popups.rs` | Popups: help, add-server, settings, alerts, server details |
| `src/storage.rs` | SQLite-based metrics storage (`MetricsStorage`) — history, stats, purge, CSV/JSON export |
| `src/ui.rs` | Legacy non-dashboard UI components (progress bars, simple tables) |
| `src/tls.rs` | Client TLS configuration for gRPC connections, optional mTLS |

## Key Features

1. **Authentication**: Random base64 access tokens. Clients pass them via `x-access-token` metadata or `Authorization: Bearer` header. Tokens are stored in the server `config.toml` and client `client-config.toml`.

2. **Monitoring Data**:
   - System info: hostname, OS, kernel, uptime, CPU count/usage
   - Memory: total, used, available with percentage
   - Disk: per-mount-point usage with visual bars
   - Processes: CPU/memory usage with filtering capability
   - Services: long-running processes with status indicators
   - Network: interfaces, IPs, MAC addresses, traffic stats
   - Containers: Docker container list with CPU, memory, network, health
   - Postgres: cluster-level metrics (databases, connections, cache hit, top queries)
   - MariaDB: schema sizes, active processes, InnoDB status, connections
   - systemd: unit status (active/inactive/failed), PID, memory, start time

3. **Dashboard UI**:
   - 8 tabs: Overview, Services, Processes, Network, Containers, Postgres, MariaDB, Systemd
   - Server list sidebar with connection status
   - Sparkline charts for CPU/memory history (last 60 samples)
   - Process filtering with `/` key
   - Sortable tables (services/processes)
   - Settings menu (icon style, update interval, auto-connect)
   - Alert popup with active/historical alerts

4. **Keyboard Shortcuts** (in dashboard):
   - `Tab` / `1-8`: Navigate tabs
   - `↑/↓` or `j/k`: Navigate items
   - `Enter`: Connect/disconnect server
   - `a`: Add new server wizard
   - `t`: Edit server token
   - `Delete`: Remove server
   - `/`: Filter processes
   - `x`: Clear filter
   - `d`: Toggle details panel
   - `s`: Settings menu
   - `C` / `D`: Connect/disconnect all servers
   - `r` / `R`: Refresh selected/all servers
   - `?`: Show help
   - `q` / `Esc`: Quit

5. **Alerts & Notifications**: The `shared` crate provides an `AlertManager` with configurable `AlertRule`s (CPU high, memory high, disk high, server down, process down). Notifications can be sent via webhook, Slack, Discord, or email channels configured in `client-config.toml`.

## Testing

Run tests with:

```bash
# Run all tests
cargo test --all

# Or via Makefile
make test
make test-verbose

# Test specific crate
cargo test --package shared
cargo test --package monitor-server
cargo test --package monitor-client
```

### Test Coverage

The codebase has extensive unit tests across nearly every module (~300 test cases total). Key test locations:

- `shared/src/lib.rs`: Serialization tests for all data structures (`SystemInfo`, `DiskInfo`, `ProcessInfo`, `ContainerInfo`, `PostgresClusterInfo`, `MariaDBClusterInfo`, `SystemdUnitInfo`, `ClientTlsConfig`), error-to-tonic-Status mapping tests
- `server/src/config.rs`: Config save/load, token generation, access token validation
- `server/src/main.rs`: Command handlers (`show-token`, `new-token`, `disable-auth`, `enable-auth`, `list-clients`, `remove-client`)
- `server/src/service.rs`: Auth validation (`validate_request`) — disabled auth, missing token, valid `x-access-token`, valid Bearer token, invalid token
- `server/src/health.rs`: HTTP health/ready/metrics endpoint tests
- `server/src/collectors/*.rs`: Tests for Docker, Postgres, MariaDB, systemd collectors
- `client/src/config.rs`: Config manager CRUD tests
- `client/src/cli.rs`: CLI argument parsing tests
- `client/src/client.rs`: Request creation with/without token
- `client/src/tls.rs`: TLS config builder tests
- `client/src/storage.rs`: SQLite storage CRUD, history, stats, purge tests
- `client/src/dashboard.rs` and submodules: UI state machine, data handling, rendering tests

### Release Readiness Check

```bash
make release-check   # runs fmt --check, clippy -D warnings, and test --all
```

## Code Style Guidelines

1. **Documentation**: All modules have doc comments (`//!`). Functions use `///` where appropriate.
2. **Error Handling**:
   - Use `anyhow::Result` for application-level errors.
   - Use `thiserror` for structured error types (in `shared/src/lib.rs`).
   - Convert internal errors to tonic `Status` for gRPC responses.
3. **Naming Conventions**:
   - `snake_case` for functions and variables
   - `PascalCase` for types and structs
   - `UPPER_SNAKE_CASE` for constants
4. **Imports**: Grouped by `std` → external crates → internal modules.
5. **Comments**:
   - Use `//` for inline comments
   - Use `/* */` for multi-line only when necessary
   - Explain "why" not "what" when possible
6. **Formatting**: Enforced via `cargo fmt`. CI fails on formatting violations.
7. **Linting**: Enforced via `cargo clippy --all-targets --all-features -- -D warnings`.

## CI / CD

### GitHub Actions Workflows

- **`.github/workflows/ci.yml`** (runs on push/PR to `main`/`master`):
  - **Lint job**: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`
  - **Test job**: Builds and tests on `ubuntu-latest` and `windows-latest` with `protoc` installed
  - **Coverage job**: Uses `cargo-tarpaulin` to generate XML coverage and uploads to Codecov
  - **Security audit job**: Runs `cargo audit` via `rustsec/audit-check`

- **`.github/workflows/release.yml`** (triggers on `v*` tags):
  - Cross-platform release builds for:
    - `linux-x86_64`
    - `linux-aarch64`
    - `windows-x86_64`
    - `macos-x86_64`
    - `macos-aarch64`
  - Packages binaries with `install.sh`, `generate-certs.sh`, `README.md`
  - Creates a GitHub Release with auto-generated notes

## Docker

Multi-stage Dockerfiles are provided:
- `Dockerfile.server` — builds `monitor-server` binary, runs as non-root `codemonitor` user
- `Dockerfile.client` — builds `monitor-client` binary
- `docker-compose.yml` — orchestrates server (ports 50051, 8080) and optional client profile

```bash
make docker-build
make docker-up
make docker-down
make docker-logs
make docker-clean
```

## Security Considerations

1. **Authentication**: Simple access tokens (random base64 strings) are the primary auth mechanism. Tokens are passed in gRPC metadata. There is no Ed25519 or public-key auth in the current codebase despite earlier experiments.

2. **Network**: TLS is supported via `rustls`. The server auto-enables TLS when `cert_path` + `key_path` are configured. The client uses a CA cert for server validation and optional client cert/key for mTLS. Plaintext gRPC is the fallback when TLS is not configured.

3. **Token Storage**: Client stores access tokens in plain text TOML (`client-config.toml`). This is intentional for ease of use but not suitable for high-security environments.

4. **Dependencies**: CI runs `cargo audit` on every push/PR to catch known vulnerabilities in dependencies.

## Common Development Tasks

### Adding a New Metric

1. Add the field to the protobuf message in `shared/src/proto/monitoring.proto`
2. Regenerate code: `cargo build` (runs `build.rs`)
3. Update `shared/src/lib.rs` types if needed
4. Update collection logic in `server/src/monitor.rs` or the relevant `server/src/collectors/*.rs`
5. Update service response in `server/src/service.rs`
6. Update client display in `client/src/dashboard.rs` or relevant render submodule

### Adding a New Dashboard Tab

1. Update tab count in `DashboardApp::next_tab()` and `previous_tab()`
2. Add tab title in `draw_header()`
3. Add drawing function (e.g., `draw_new_tab()`)
4. Update `draw_main_content()` to call the new function
5. Add help text in `draw_help_popup()`

### Modifying Authentication

The authentication flow involves:
- `server/src/config.rs`: `validate_access_token()` method
- `server/src/service.rs`: `validate_request()` method (checks `x-access-token` and `Authorization: Bearer`)
- `client/src/client.rs`: `create_request()` and `create_request_with_token()` methods (inject token into metadata)

### Adding TLS

1. Generate certificates: `./generate-certs.sh ./certs`
2. Server: add `[tls]` section to `config.toml` with `cert_path` and `key_path`
3. Client: add `[servers.tls]` section to `client-config.toml` with `ca_cert_path`
4. Optional mTLS: also add `client_cert_path` + `client_key_path` on client, and `ca_path` on server
5. Rebuild and restart both components

## Troubleshooting

### Build Issues

```bash
# Clean and rebuild
cargo clean
cargo build --release

# Check dependencies
cargo tree
```

### Protocol Buffer Issues

Ensure `protoc` is installed:
```bash
# Ubuntu/Debian
sudo apt-get install protobuf-compiler

# macOS
brew install protobuf

# Windows
choco install protoc
```

### Windows Build Issues

The TUI dependencies may require Windows SDK. See `WINDOWS_BUILD.md` for workarounds. The client uses automatic detection for Nerd Font support and falls back to ASCII icons on Windows.

## Deployment

### Quick Install (Linux)

```bash
# Download and install server with systemd
curl -sSL https://github.com/codigocentral/code-monitor/releases/latest/download/install.sh | sudo bash

# Or manually
sudo ./install.sh

# Start service
sudo systemctl start code-monitor-server
sudo systemctl enable code-monitor-server
```

### Certificate Generation

```bash
# Development self-signed certificates
./generate-certs.sh ./certs

# Or via Makefile
make certs
```

## Files to Know

| File | Purpose |
|------|---------|
| `Cargo.toml` | Workspace definition and shared dependencies |
| `Makefile` | Common development tasks (build, test, lint, docker, certs) |
| `install.sh` | Server installer with systemd integration |
| `install.ps1` | Windows PowerShell installer |
| `generate-certs.sh` | Self-signed certificate generator |
| `.github/workflows/ci.yml` | CI pipeline (lint, test, coverage, security audit) |
| `.github/workflows/release.yml` | Cross-platform release builds on tags |
| `Dockerfile.server` / `Dockerfile.client` | Multi-stage Docker builds |
| `docker-compose.yml` | Local Docker orchestration |
| `shared/src/proto/monitoring.proto` | gRPC service definition |
| `config.example.toml` | Server runtime configuration example |
| `client-config.example.toml` | Client runtime configuration example |
| `README.md` | User-facing documentation |
| `BUILD.md` | Build and deployment guide |
| `WINDOWS_BUILD.md` | Windows-specific build instructions |
| `necessidade.md` | Original requirements (Portuguese) |
