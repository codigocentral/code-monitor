# Code Monitor

> **Lightweight multi-server monitoring built in Rust**

[![CI](https://github.com/codigocentral/code-monitor/workflows/CI/badge.svg)]()
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)]()
[![Version](https://img.shields.io/badge/version-0.1.0-green)]()

Code Monitor is a **high-performance, low-overhead system monitoring platform** that combines the immediacy and simplicity of `htop` with the multi-node visibility and alerting of modern observability stacks. Engineered from scratch in pure Rust for maximum efficiency, predictability, and memory safety.

---

## 🚀 Key Highlights & Capabilities

### Core Values
- ⚡ **Ultra-lightweight:** Consumes under 1.5% CPU and ~15MB RAM on monitored servers (up to 10x lighter than Python/Go agents).
- 🖥️ **Terminal-native TUI:** Clean, keyboard-driven dashboard over SSH with zero web browser dependency.
- 🔒 **100% On-Premise & Privacy-First:** Telemetry and server metrics remain entirely within your private infrastructure.
- 📦 **Single Static Binary:** Self-contained executable with zero runtime dependencies.
- 🔐 **Hardened Security:** End-to-end TLS encryption and token-based authentication.
- 🚨 **Threshold Alerts:** Built-in alerting supporting Discord, Slack, and generic webhooks.

### Server Agent (`monitor-server`)
- **System Metrics:** Real-time CPU, RAM, Swap, Disk I/O, Network interfaces, Processes, and systemd units.
- **Container Telemetry:** Docker container enumeration, resource consumption, and health status.
- **Database Clusters:** PostgreSQL and MariaDB metrics (connection pools, cache hit ratios, running queries).
- **Dual Interface:** High-speed streaming gRPC API + HTTP health/readiness endpoints (`/health`, `/ready`, `/metrics` for Prometheus).

### Client Dashboard (`monitor-client`)
- Interactive Terminal UI (TUI) powered by `tui-rs` / `crossterm`.
- Multi-server switching from a single pane of glass.
- 8 dedicated operational tabs: Overview, Services, Processes, Network, Containers, Postgres, MariaDB, Systemd.
- Local historical metric caching using embedded SQLite.

---

## 🏛️ System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                       ADMIN / WORKSTATION                       │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  monitor-client (Rust TUI)                              │    │
│  │  ├── TUI Dashboard (crossterm / tui-rs)                 │    │
│  │  ├── Local Historical Storage (SQLite / rusqlite)       │    │
│  │  └── Alert Manager (Discord / Slack / Webhook)          │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
                               │
                               │ gRPC streaming over TLS (Tonic / Prost)
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                     MONITORED SERVERS (FLEET)                   │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  monitor-server (Rust Daemon)                           │    │
│  │  ├── System Collectors (sysinfo / /proc reader)         │    │
│  │  ├── Docker & Database Collectors (Postgres / MariaDB)  │    │
│  │  ├── gRPC Service (Tonic gRPC Server)                   │    │
│  │  └── Health & Prometheus HTTP Server (Axum)             │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

---

## 📂 Repository & Crate Structure

This repository is structured as a Cargo workspace:

```
code-monitor/
├── Cargo.toml                         # Workspace definition (members: shared, server, client)
├── AGENTS.md                          # Comprehensive technical guidelines for AI agents & developers
├── CLAUDE.md                          # Session guide, sibling repos, and publication rules
├── BUILD.md / WINDOWS_BUILD.md        # Detailed build, cross-compilation, and platform instructions
├── CONTRIBUTING.md                    # Guidelines for open-source contributors
├── PLANO-MESTRE-OPEN-SOURCE.md        # Product roadmap and open-source strategy
├── scripts/
│   ├── publish-to-github.sh           # Safe release pipeline (filters internal data via .publishignore)
│   └── generate-certs.sh              # Script to generate TLS certificates
├── shared/                            # Shared data types, gRPC protobufs, and utilities
│   ├── proto/monitor.proto            # gRPC protocol buffer service definitions
│   └── src/                           # Common models, configuration schemas, and error types
├── server/                            # monitor-server daemon
│   └── src/                           # Metric collectors, gRPC endpoints, Axum HTTP server
└── client/                            # monitor-client TUI application
    └── src/                           # Terminal UI views, event loop, connection manager, SQLite
```

---

## 📦 Installation & Quick Start

### 1. Automated Installation (Linux / macOS)
```bash
curl -sSL https://github.com/codigocentral/code-monitor/releases/latest/download/install.sh | sudo bash
```

### 2. Windows (PowerShell Administrator)
```powershell
iwr https://github.com/codigocentral/code-monitor/releases/latest/download/install.ps1 -UseB | iex
```

### 3. Running the Server Daemon
```bash
# Initialize default configuration
sudo monitor-server --config /etc/code-monitor/config.toml init

# Run the server daemon
sudo systemctl start code-monitor-server
# Or run directly:
./monitor-server --address 0.0.0.0 --port 50051
```

### 4. Running the Client TUI
```bash
# Launch interactive dashboard
monitor-client

# Or connect directly to a remote host
monitor-client connect 10.10.0.8 --port 50051
```

---

## 🛠️ Development & Testing

### Prerequisites
- **Rust Toolchain:** 1.75+ (`rustup default stable`)
- **Protocol Buffers Compiler:** `protoc` (required by `prost-build` / `tonic-build`)
- **OpenSSL / pkg-config:** Required on Linux builds (`sudo apt install pkg-config libssl-dev protobuf-compiler`)

### Commands
```bash
# Build the entire workspace
cargo build --workspace

# Run all unit and integration tests
cargo test --workspace

# Check lints with Clippy
cargo clippy --workspace -- -D warnings

# Build release binaries
cargo build --release --workspace

# Run server locally
cargo run -p server -- --config config.example.toml

# Run client locally
cargo run -p client
```

---

## 🤖 Guidelines for Developers and AI Agents

1. **Safety and Panic Prevention:**
   - Collector threads inside `server/` must **never panic**. Always handle missing metrics or failing system calls gracefully using `Option` or `Result` and fall back to zero/empty values.
   - Do not use `unwrap()` or `expect()` in production collector paths.
2. **Asynchronous Concurrency:**
   - Network streaming and HTTP endpoints use **Tokio**. Blocking I/O (such as file reads or heavy system queries) must run on `tokio::task::spawn_blocking` to avoid stalling the async reactor.
3. **gRPC Protocol Contracts:**
   - Any modification to message schemas must occur in `shared/proto/monitor.proto`. Run `cargo build` to trigger `tonic-build` code generation.
4. **Sibling Repositories & Confidentiality:**
   - This project is part of a triad: `code-monitor` (public product code), `code-monitor-ops` (private fleet infrastructure), and `code-monitor-clientes` (private client data).
   - **Never commit real server hostnames, VPN IPs, customer tokens, or private secrets to this repository.**
   - All external releases are filtered through `scripts/publish-to-github.sh` respecting `.publishignore`.
   - Before implementing complex changes or collectors, consult [`AGENTS.md`](AGENTS.md) and [`CLAUDE.md`](CLAUDE.md).

---

## 📝 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
