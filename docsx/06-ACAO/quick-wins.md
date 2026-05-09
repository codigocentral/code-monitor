# Quick Wins - Fazer Agora

## Prioridade Imediata (Esta Semana)

### 1. GitHub Repository Setup

```bash
# Ações imediatas
[ ] Criar README.md profissional
[ ] Adicionar LICENSE (MIT)
[ ] Criar CONTRIBUTING.md
[ ] Setup issue templates
[ ] Setup PR template
[ ] Adicionar badges (CI, version, license)
```

**README Template:**
```markdown
# Code Monitor

> Lightweight multi-server monitoring built in Rust

[![CI](https://github.com/org/code-monitor/workflows/CI/badge.svg)]()
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)]()

## Features

- 🚀 **10x lighter** than Python alternatives (1.5% CPU vs 15%)
- 🖥️ **Terminal-native** - works via SSH, no browser needed
- 🔒 **100% on-prem** - your data never leaves your servers
- 📦 **Single binary** - zero dependencies, 5-minute setup

## Quick Start

\`\`\`bash
# Download
curl -sSL https://get.codemonitor.io | bash

# Start server (on each machine to monitor)
./monitor-server

# Start client (on your workstation)
./monitor-client
\`\`\`

## Documentation

- [Getting Started](./docs/getting-started.md)
- [Configuration](./docs/configuration.md)
- [FAQ](./docs/faq.md)

## License

MIT
```

### 2. CI/CD Pipeline

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --all

  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings

  build:
    needs: [test, lint]
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --release
```

**Tempo estimado:** 1-2 horas

### 3. Docker Compose Básico

```yaml
# docker-compose.yml
version: '3.8'

services:
  monitor-server:
    build:
      context: .
      dockerfile: Dockerfile.server
    ports:
      - "50051:50051"
      - "8080:8080"
    volumes:
      - ./config.toml:/etc/code-monitor/config.toml
    restart: unless-stopped
```

```dockerfile
# Dockerfile.server
FROM rust:1.75 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin monitor-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/monitor-server /usr/local/bin/
EXPOSE 50051 8080
CMD ["monitor-server"]
```

**Tempo estimado:** 1-2 horas

---

## Próximos 3 Dias

### 4. TLS Básico (Self-Signed)

```bash
# Gerar certificados para desenvolvimento
mkdir -p certs
openssl req -x509 -newkey rsa:4096 -keyout certs/server.key -out certs/server.crt \
    -days 365 -nodes -subj "/CN=localhost"
```

```rust
// Adicionar ao config.toml
[tls]
enabled = true
cert_path = "./certs/server.crt"
key_path = "./certs/server.key"
```

**Tempo estimado:** 4-6 horas

### 5. Health Check Endpoint

```rust
// Adicionar endpoint simples
use axum::{routing::get, Router, Json};
use serde::Serialize;

#[derive(Serialize)]
struct Health {
    status: String,
    version: String,
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

// Adicionar ao router
Router::new()
    .route("/health", get(health))
```

**Tempo estimado:** 1-2 horas

### 6. Documentação Inicial

```
docs/
├── README.md           # Overview
├── getting-started.md  # Quick start
├── installation.md     # Install options
├── configuration.md    # Config reference
└── faq.md             # Common questions
```

**Tempo estimado:** 4-6 horas

---

## Esta Semana

### 7. SQLite para Histórico

```rust
// Cargo.toml
[dependencies]
rusqlite = { version = "0.30", features = ["bundled"] }
```

```rust
// client/src/storage.rs
pub struct MetricsStorage {
    conn: Connection,
}

impl MetricsStorage {
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS metrics (
                id INTEGER PRIMARY KEY,
                server_id TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                cpu_usage REAL,
                memory_used INTEGER,
                memory_total INTEGER
            )",
            [],
        )?;
        Ok(Self { conn })
    }

    pub fn store(&self, server_id: &str, metrics: &Metrics) -> Result<()> {
        self.conn.execute(
            "INSERT INTO metrics (server_id, timestamp, cpu_usage, memory_used, memory_total)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                server_id,
                Utc::now().timestamp(),
                metrics.cpu_usage,
                metrics.memory_used,
                metrics.memory_total,
            ],
        )?;
        Ok(())
    }
}
```

**Tempo estimado:** 4-6 horas

### 8. Alerta Simples (CPU)

```rust
// Primeiro alerta: CPU > threshold por X segundos
pub struct CpuAlert {
    threshold: f64,
    duration: Duration,
    samples: VecDeque<f64>,
}

impl CpuAlert {
    pub fn check(&mut self, cpu_usage: f64) -> Option<Alert> {
        self.samples.push_back(cpu_usage);

        // Manter apenas samples do período
        while self.samples.len() > (self.duration.as_secs() / 5) as usize {
            self.samples.pop_front();
        }

        // Verificar se todos estão acima do threshold
        if self.samples.iter().all(|&v| v > self.threshold) {
            Some(Alert {
                type_: AlertType::CpuHigh,
                message: format!("CPU above {:.1}% for {:?}", self.threshold, self.duration),
                value: cpu_usage,
            })
        } else {
            None
        }
    }
}
```

**Tempo estimado:** 3-4 horas

---

## Próximas 2 Semanas

### 9. Webhook Notifier

```rust
pub async fn send_webhook(url: &str, alert: &Alert) -> Result<()> {
    let client = reqwest::Client::new();

    let payload = serde_json::json!({
        "event": "alert",
        "server": alert.server_id,
        "type": alert.type_.to_string(),
        "message": alert.message,
        "value": alert.value,
        "timestamp": Utc::now().to_rfc3339(),
    });

    client.post(url)
        .json(&payload)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    Ok(())
}
```

**Tempo estimado:** 2-3 horas

### 10. Discord/Slack Integration

```rust
// Slack
pub async fn send_slack(webhook_url: &str, alert: &Alert) -> Result<()> {
    let payload = serde_json::json!({
        "text": format!("🚨 *Alert*: {}", alert.message),
        "attachments": [{
            "color": "danger",
            "fields": [
                {"title": "Server", "value": alert.server_id, "short": true},
                {"title": "Value", "value": format!("{:.2}", alert.value), "short": true}
            ]
        }]
    });

    reqwest::Client::new()
        .post(webhook_url)
        .json(&payload)
        .send()
        .await?;

    Ok(())
}

// Discord (formato similar)
pub async fn send_discord(webhook_url: &str, alert: &Alert) -> Result<()> {
    let payload = serde_json::json!({
        "content": format!("🚨 **Alert**: {}", alert.message),
        "embeds": [{
            "color": 15158332,
            "fields": [
                {"name": "Server", "value": alert.server_id, "inline": true},
                {"name": "Value", "value": format!("{:.2}", alert.value), "inline": true}
            ]
        }]
    });

    reqwest::Client::new()
        .post(webhook_url)
        .json(&payload)
        .send()
        .await?;

    Ok(())
}
```

**Tempo estimado:** 2-3 horas

---

## Checklist Resumido

### Hoje
- [ ] GitHub README profissional
- [ ] GitHub Actions CI básico
- [ ] Docker Compose funcional

### Esta Semana
- [ ] TLS (self-signed)
- [ ] Health check endpoint
- [ ] SQLite histórico
- [ ] Documentação inicial

### Próximas 2 Semanas
- [ ] Alerta de CPU
- [ ] Webhook notifier
- [ ] Slack/Discord integration
- [ ] Beta testing (10 users)

---

## Impacto por Quick Win

| Quick Win | Esforço | Impacto | ROI |
|-----------|---------|---------|-----|
| README | 1h | Alto | ⭐⭐⭐⭐⭐ |
| CI/CD | 2h | Alto | ⭐⭐⭐⭐⭐ |
| Docker | 2h | Médio | ⭐⭐⭐⭐ |
| TLS | 4h | Alto | ⭐⭐⭐⭐ |
| Health Check | 1h | Médio | ⭐⭐⭐⭐ |
| SQLite | 4h | Alto | ⭐⭐⭐⭐ |
| Alertas | 4h | Alto | ⭐⭐⭐⭐⭐ |
| Webhooks | 3h | Alto | ⭐⭐⭐⭐ |
| Docs | 4h | Alto | ⭐⭐⭐⭐ |

---

## Próximos Passos

- [Checklist de Lançamento](./checklist-lancamento.md) - Para o launch
- [Métricas de Sucesso](./metricas-sucesso.md) - KPIs
