# Code Monitor

> Lightweight multi-server monitoring and auditing, built in Rust.

[![CI](https://github.com/codigocentral/code-monitor/actions/workflows/ci.yml/badge.svg)](https://github.com/codigocentral/code-monitor/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Code Monitor pairs a small agent (`monitor-server`) that runs on each host with a
terminal dashboard (`monitor-client`) that watches many hosts at once. Think of
`htop` with multi-host visibility, plus an audit pass that turns raw figures into
findings you can act on: a database reachable from the internet, a certificate
renewal that silently stopped, a container that would be OOM-killed by a
`swapoff`, a host whose configuration promises more memory than it has.

Code Monitor runs in production on Código Central's own servers. It is pre-1.0:
the gRPC protocol and the configuration format may still change between
releases.

## Features

**Agent (`monitor-server`)**

- System metrics: CPU, memory, swap rates and Linux PSI pressure, disks,
  network interfaces, processes and systemd units.
- Docker: containers, memory limits, restart counts (crash loops), healthcheck
  streaks, image digest drift for floating tags.
- PostgreSQL and MariaDB: settings with their source file, connections,
  idle/orphaned instances.
- TLS: expiry of the certificates actually served, and health of the renewal
  unit.
- Network and firewall: data stores bound to every interface, cross-checked
  against `iptables-save`/`ip6tables-save` (including Docker `DNAT`).
- Scheduled jobs and disk policy checks that run without an open dashboard.
- gRPC streaming API with token authentication and optional TLS/mTLS.
- HTTP endpoints: `/health`, `/ready`, `/findings` (JSON) and `/metrics`
  (Prometheus; see [docs/prometheus-metrics.md](docs/prometheus-metrics.md)).

**Client (`monitor-client`)**

- Keyboard-driven TUI (`tui-rs` + `crossterm`) that works over SSH.
- Many servers from a single dashboard; tabs for overview, services, processes,
  network, containers, databases and systemd.
- Local metric history in SQLite with CSV/JSON export.
- Headless mode: `monitor-client monitor --format json`.
- Alerts to Slack, Discord or a generic webhook, with per-server silencing.

## Architecture

```
┌──────────────────────────────┐
│  workstation                 │
│  monitor-client (TUI)        │
│  ├─ dashboard                │
│  ├─ SQLite history           │
│  └─ alert notifications      │
└──────────────┬───────────────┘
               │ gRPC (optionally TLS / mTLS)
               ▼
┌──────────────────────────────┐
│  each monitored host         │
│  monitor-server (agent)      │
│  ├─ collectors + audit pass  │
│  ├─ gRPC service             │
│  └─ HTTP health / metrics    │
└──────────────────────────────┘
```

The repository is a Cargo workspace:

| Crate | Path | Contents |
|---|---|---|
| `shared` | `shared/` | Protobuf definitions, shared types, findings, alerts |
| `monitor-server` | `server/` | Agent: collectors, audit, gRPC and HTTP servers |
| `monitor-client` | `client/` | TUI, CLI, SQLite storage |

## Installation

Prebuilt binaries are attached to each
[GitHub release](https://github.com/codigocentral/code-monitor/releases).

Linux / macOS:

```bash
curl -sSL https://github.com/codigocentral/code-monitor/releases/latest/download/install.sh | sudo bash
```

Windows (PowerShell as Administrator):

```powershell
iwr https://github.com/codigocentral/code-monitor/releases/latest/download/install.ps1 -UseB | iex
```

Or build from source (needs Rust stable and `protoc`):

```bash
cargo build --release --workspace
```

See [BUILD.md](BUILD.md) and [WINDOWS_BUILD.md](WINDOWS_BUILD.md) for details.

## Quick start

On each host:

```bash
# Interactive setup: writes the config and generates an access token
sudo monitor-server --config /etc/code-monitor/config.toml init
sudo systemctl enable --now code-monitor-server

# Print the token the client will need
sudo monitor-server --config /etc/code-monitor/config.toml show-token
```

Bind the agent to a private or VPN address, not to a public interface.

On your workstation:

```bash
monitor-client add --name "web-1" --address 203.0.113.10 --token <token>
monitor-client            # open the dashboard
```

Example configurations: [config.example.toml](config.example.toml) (agent) and
[client-config.example.toml](client-config.example.toml) (client). Database
collectors need a read-only account; see
[docs/enabling-database-collectors.md](docs/enabling-database-collectors.md).

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

[CONTRIBUTING.md](CONTRIBUTING.md) has the full checklist, and
[AGENTS.md](AGENTS.md) describes the architecture and conventions in depth.

## Security

Please report vulnerabilities privately; see [SECURITY.md](SECURITY.md).

## License

MIT. See [LICENSE](LICENSE).
