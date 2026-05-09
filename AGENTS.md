# System Monitor - Agent Guide

## Project Overview

This is a **cross-platform system monitoring application** built in Rust, similar to htop, with a client-server architecture. It allows monitoring multiple remote servers from a single command-line interface with a rich TUI (Terminal User Interface) dashboard.

The project originated from a Portuguese requirement document (`necessidade.md`) but the codebase uses **English** for all code, comments, and documentation.

## Architecture

```
┌─────────────────┐    gRPC/HTTP2    ┌─────────────────┐
│   monitor-      │◄────────────────►│   monitor-      │
│   client        │   Port 50051     │   server        │
│   (TUI CLI)     │                  │   (Data Collector│
│                 │                  │   & Auth)        │
└─────────────────┘                  └─────────────────┘
```

### Workspace Structure

The project uses a Cargo workspace with three crates:

| Crate | Path | Description | Binary Name |
|-------|------|-------------|-------------|
| `shared` | `shared/` | Common types, Protocol Buffers definitions, and shared protocol code | Library only |
| `server` | `server/` | System monitoring daemon that collects metrics | `monitor-server` |
| `client` | `client/` | CLI tool with TUI dashboard for viewing metrics | `monitor-client` |

## Technology Stack

- **Language**: Rust (Edition 2021)
- **Async Runtime**: Tokio
- **RPC Framework**: gRPC (tonic)
- **Protocol Buffers**: prost (compiled via build.rs)
- **TUI Framework**: tui-rs + crossterm
- **System Info**: sysinfo crate
- **Docker API**: bollard crate
- **Postgres**: tokio-postgres crate
- **MariaDB**: mysql_async crate
- **systemd**: systemctl parsing (Linux only)
- **Authentication**: Simple access tokens (Ed25519 key infrastructure exists but is not fully integrated)
- **TLS**: rustls 0.21/0.22, tokio-rustls 0.24 (server + client identity, optional mTLS)
- **Serialization**: serde, toml, bincode
- **Error Handling**: anyhow, thiserror
- **Logging**: tracing, tracing-subscriber
- **CLI Parsing**: clap

## Build Commands

### Prerequisites

- Rust 1.70+ with Cargo
- Protocol Buffers compiler (`protoc`)
- Linux or Windows (cross-platform supported)

### Build Everything

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release
```

### Build Individual Components

```bash
# Build shared library only
cd shared && cargo build --release

# Build server only
cd server && cargo build --release

# Build client only
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

# Server commands
./target/release/monitor-server show-token      # Display access token
./target/release/monitor-server new-token       # Generate new token
./target/release/monitor-server disable-auth    # Disable authentication
./target/release/monitor-server enable-auth     # Enable authentication
./target/release/monitor-server list-clients    # List authorized clients
./target/release/monitor-server remove-client --name <client>  # Remove a client
```

### Client (runs on the monitoring workstation)

```bash
# Open interactive dashboard (default)
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

# List services from a server
./target/release/monitor-client services --server <uuid>

# List processes from a server
./target/release/monitor-client processes --server <uuid> --filter <pattern>
```

## Configuration Files

### Server Configuration (`config.toml`)

```toml
update_interval_seconds = 5
max_clients = 100
enable_authentication = true
log_level = "info"
access_token = "auto-generated-token"
```

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

[servers.tls]
ca_cert_path = "/path/to/ca.crt"
client_cert_path = "/path/to/client.crt"  # optional, for mTLS
client_key_path = "/path/to/client.key"   # optional, for mTLS
```

## Code Organization

### Shared Library (`shared/`)

- **`src/lib.rs`**: Error types (`MonitorError`) and data structures (`SystemInfo`, `ProcessInfo`, `ServiceInfo`, `NetworkInfo`, etc.)
- **`src/proto/monitoring.proto`**: gRPC service definition and Protocol Buffer messages
- **`build.rs`**: Compiles .proto files using tonic-build

### Server (`server/`)

- **`src/main.rs`**: Entry point, CLI argument parsing, server startup banner
- **`src/config.rs`**: Configuration management with TOML serialization, access token generation
- **`src/monitor.rs`**: System information collection using sysinfo (CPU, memory, disk, processes, services, network)
- **`src/service.rs`**: gRPC service implementation (`MonitorServiceImpl`)
- **`src/auth.rs`**: Ed25519 key generation and signature verification (partially implemented)
- **`src/tls.rs`**: TLS identity loader and availability checker

### Client (`client/`)

- **`src/main.rs`**: Entry point, command dispatch
- **`src/cli.rs`**: CLI argument definitions using clap
- **`src/client.rs`**: gRPC client implementation (`MonitorClient`) with connection management
- **`src/config.rs`**: Client configuration management (add/remove/list servers)
- **`src/dashboard.rs`**: Interactive TUI dashboard (~2200 lines) with real-time updates
- **`src/ui.rs`**: Legacy UI components (superseded by dashboard)
- **`src/auth.rs`**: Client-side Ed25519 authentication (partially implemented)
- **`src/tls.rs`**: Client TLS configuration for gRPC connections

## Key Features

1. **Authentication**: Uses simple access tokens (base64 random strings) by default. Ed25519 public-key authentication infrastructure exists but is not fully integrated.

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
   - Systemd: unit status (active/inactive/failed), PID, memory, start time

3. **Dashboard UI**:
   - 8 tabs: Overview, Services, Processes, Network, Containers, Postgres, MariaDB, Systemd
   - Server list sidebar with connection status
   - Sparkline charts for CPU/memory history (last 60 samples)
   - Process filtering with `/` key
   - Sortable tables (services/processes)
   - Settings menu (icon style, update interval, auto-connect)

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

## Testing

Run tests with:

```bash
# Run all tests
cargo test

# Test specific crate
cargo test --package shared
cargo test --package monitor-server
cargo test --package monitor-client
```

### Existing Tests

- `shared/src/lib.rs`: No tests (data structures only)
- `server/src/config.rs`: Config save/load tests, token generation tests
- `server/src/auth.rs`: Auth manager key generation test
- `client/src/config.rs`: Config manager CRUD tests
- `client/src/auth.rs`: Client auth key generation test
- `client/src/cli.rs`: CLI argument parsing tests

## Code Style Guidelines

1. **Documentation**: All modules have doc comments (`//!`). Functions use `///` documentation where appropriate.

2. **Error Handling**: 
   - Use `anyhow::Result` for application-level errors
   - Use `thiserror` for structured error types (in `shared`)
   - Convert internal errors to tonic `Status` for gRPC responses

3. **Naming Conventions**:
   - Snake_case for functions and variables
   - PascalCase for types and structs
   - UPPER_SNAKE_CASE for constants

4. **Imports**: Grouped by std -> external crates -> internal modules

5. **Comments**: 
   - Use `//` for inline comments
   - Use `/* */` for multi-line only when necessary
   - Explain "why" not "what" when possible

## Security Considerations

1. **Authentication**: Current implementation uses simple access tokens passed in gRPC metadata (`x-access-token` header or `Authorization: Bearer` header). The Ed25519 key infrastructure exists but is not the primary authentication method.

2. **Network**: TLS is supported via rustls. Server auto-enables TLS when `cert_path` + `key_path` are configured. Client uses CA cert for server validation and optional client cert/key for mTLS. Plaintext gRPC is still the fallback when TLS is not configured.

3. **Token Storage**: Client stores access tokens in plain text TOML file. This is intentional for ease of use but not suitable for high-security environments.

## Common Development Tasks

### Adding a New Metric

1. Add the field to the protobuf message in `shared/src/proto/monitoring.proto`
2. Regenerate code: `cargo build` (runs build.rs)
3. Update `shared/src/lib.rs` types if needed
4. Update collection logic in `server/src/monitor.rs`
5. Update service response in `server/src/service.rs`
6. Update client display in `client/src/dashboard.rs`

### Adding a New Dashboard Tab

1. Update tab count in `DashboardApp::next_tab()` and `previous_tab()`
2. Add tab title in `draw_header()`
3. Add drawing function (e.g., `draw_new_tab()`)
4. Update `draw_main_content()` to call the new function
5. Add help text in `draw_help_popup()`

### Modifying Authentication

The authentication flow involves:
- `server/src/config.rs`: Token validation logic (`validate_access_token`)
- `server/src/service.rs`: `validate_request()` method
- `client/src/client.rs`: `create_request()` method (adds token to metadata)

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
```

### Windows Build Issues

The TUI dependencies may require Windows SDK. See `WINDOWS_BUILD.md` for workarounds. The client uses automatic detection for Nerd Font support and falls back to ASCII icons on Windows.

## Deployment

### Quick Install (Linux)

```bash
# Download and install server with systemd
curl -sSL https://github.com/diogo/code-monitor/releases/latest/download/install.sh | sudo bash

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
```

### GitHub Actions Release

The `.github/workflows/release.yml` builds cross-platform binaries on every `v*` tag:
- `linux-x86_64`
- `linux-aarch64`
- `windows-x86_64`
- `macos-x86_64`
- `macos-aarch64`

## Files to Know

| File | Purpose |
|------|---------|
| `Cargo.toml` | Workspace definition and shared dependencies |
| `install.sh` | Server installer with systemd integration |
| `generate-certs.sh` | Self-signed certificate generator |
| `.github/workflows/release.yml` | Cross-platform release CI/CD |
| `shared/Cargo.toml` | Shared library dependencies |
| `server/Cargo.toml` | Server binary dependencies |
| `client/Cargo.toml` | Client binary dependencies |
| `shared/src/proto/monitoring.proto` | gRPC service definition |
| `config.toml` | Server runtime configuration |
| `client-config.toml` | Client runtime configuration |
| `README.md` | User-facing documentation |
| `BUILD.md` | Build and deployment guide |
| `necessidade.md` | Original requirements (Portuguese) |
