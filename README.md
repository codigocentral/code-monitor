# Code Monitor

> **Lightweight multi-server monitoring built in Rust**

[![CI](https://github.com/diogo/code-monitor/workflows/CI/badge.svg)]()
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)]()
[![Version](https://img.shields.io/badge/version-0.1.0-green)]()

Code Monitor is a **high-performance system monitoring tool** that combines the simplicity of htop with the power of modern monitoring platforms. Built in Rust for maximum efficiency.

## 🚀 Features

### Core
- ⚡ **10x lighter** than Python alternatives (1.5% CPU vs 15%)
- 🖥️ **Terminal-native** TUI - works via SSH, no browser needed  
- 🔒 **100% on-prem** - your data never leaves your servers
- 📦 **Single binary** - zero dependencies, 5-minute setup
- 🔐 **TLS encryption** - secure communication
- 🚨 **Smart alerts** - threshold-based with multiple channels

### Server (`monitor-server`)
- Real-time metrics: CPU, memory, disk, network, processes, services
- Docker container monitoring (names, CPU, memory, health)
- Postgres cluster monitoring (databases, connections, cache hit, top queries)
- MariaDB cluster monitoring (schemas, processes, InnoDB status)
- systemd unit monitoring (status, PID, memory, start time)
- gRPC API with TLS support
- HTTP health checks (`/health`, `/ready`, `/metrics`)
- Token-based authentication
- Prometheus-compatible metrics endpoint

### Client (`monitor-client`)
- Beautiful TUI dashboard with sparklines
- Multi-server monitoring from single interface
- 8 dashboard tabs: Overview, Services, Processes, Network, Containers, Postgres, MariaDB, Systemd
- Smart alerts with Discord/Slack/webhook notifications
- Historical data with SQLite storage
- Alert management and notifications
- Auto-reconnect and connection pooling

## 📦 Installation

### Quick Install
```bash
curl -sSL https://get.codemonitor.io | bash
```

### Docker
```bash
docker-compose up -d
```

### From Source
```bash
git clone https://github.com/diogo/code-monitor
cd code-monitor
cargo build --release
```

## 🚀 Quick Start

### 1. Start the Server
```bash
# On each server you want to monitor
./monitor-server --address 0.0.0.0 --port 50051
```

The server will display:
```
╔══════════════════════════════════════════════════════════════╗
║                   🖥️  SYSTEM MONITOR SERVER                   ║
╠══════════════════════════════════════════════════════════════╣
║  gRPC:    0.0.0.0:50051                                       ║
║  HTTP:    0.0.0.0:8080                                        ║
╠══════════════════════════════════════════════════════════════╣
║  🔐 Authentication: ENABLED                                   ║
║  Access Token: xxxxxxxxxxxxxxxxxxx                            ║
╚══════════════════════════════════════════════════════════════╝
```

### 2. Connect with Client
```bash
# Interactive dashboard
./monitor-client

# Or quick connect
./monitor-client connect 192.168.1.100 --port 50051
```

## 🛠️ Development

```bash
# Build
make build

# Test
make test

# Lint
make lint

# Docker
make docker-up

# Generate TLS certs
make certs
```

## 📊 Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         WORKSTATION                              │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  monitor-client                                         │    │
│  │  ├── TUI Dashboard (tui-rs)                            │    │
│  │  ├── SQLite Storage (rusqlite)                         │    │
│  │  └── Alert Manager                                     │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
                              │ gRPC/TLS
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                         SERVERS                                  │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  monitor-server                                         │    │
│  │  ├── System Monitor (sysinfo)                          │    │
│  │  ├── gRPC Service (tonic)                              │    │
│  │  ├── Health HTTP (axum)                                │    │
│  │  └── Metrics (/metrics)                                │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

## 🔧 Configuration

### Server (`config.toml`)
```toml
update_interval_seconds = 5
max_clients = 100
enable_authentication = true
access_token = "your-token-here"
log_level = "info"

[tls]
enabled = true
cert_path = "/etc/code-monitor/server.crt"
key_path = "/etc/code-monitor/server.key"
```

### Client (`client-config.toml`)
```toml
update_interval_seconds = 5
auto_reconnect = true
reconnect_delay_seconds = 5

[[servers]]
id = "uuid"
name = "Production Server"
address = "192.168.1.100"
port = 50051
access_token = "token-from-server"
```

## 🚨 Alert Configuration

```toml
[[alerts]]
name = "CPU Critical"
type = "cpu_high"
threshold = 90
duration_seconds = 300  # 5 minutes
severity = "critical"
notify = ["webhook", "slack"]

[[alerts]]
name = "Memory Warning"
type = "memory_high"
threshold = 80
duration_seconds = 60
severity = "warning"
notify = ["discord"]
```

## 📡 Health Check Endpoints

- `GET /health` - Basic health status
- `GET /ready` - Readiness probe with checks
- `GET /metrics` - Prometheus-compatible metrics

## 🐳 Docker Compose

```yaml
version: '3.8'
services:
  monitor-server:
    image: codemonitor/server:latest
    ports:
      - "50051:50051"
      - "8080:8080"
    volumes:
      - ./config.toml:/etc/code-monitor/config.toml
      - monitor-data:/var/lib/code-monitor
    restart: unless-stopped
```

## 📚 Documentation

- [Product Vision](docsx/01-PRODUTO/visao-geral.md)
- [Market Analysis](docsx/02-MERCADO/analise-competitiva.md)
- [Technical Architecture](docsx/05-TECNICO/arquitetura-futura.md)
- [Roadmap](docsx/04-ROADMAP/visao-12-meses.md)
- [Quick Wins](docsx/06-ACAO/quick-wins.md)

## 🤝 Contributing

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## 📝 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

---

**Built with Rust** 🦀 | **Fast** ⚡ | **Secure** 🔒 | **Open Source** 📖
