# Implementacao Curto Prazo

## Prioridades Imediatas (Fase 1)

### 1. TLS/SSL para gRPC

**Objetivo:** Comunicação segura entre client e server

**Arquivos a modificar:**
```
server/src/
├── main.rs          # TLS config no server
├── config.rs        # TLS settings
└── tls.rs           # NEW: TLS utilities

client/src/
├── client.rs        # TLS config no client
└── config.rs        # TLS settings

shared/src/
└── lib.rs           # Shared TLS types
```

**Implementação Server:**
```rust
// server/src/tls.rs
use rustls::{Certificate, PrivateKey, ServerConfig};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use tonic::transport::{Identity, ServerTlsConfig};

pub struct TlsConfig {
    pub enabled: bool,
    pub cert_path: String,
    pub key_path: String,
    pub ca_path: Option<String>,
}

impl TlsConfig {
    pub fn load_server_config(&self) -> Result<ServerTlsConfig, Box<dyn std::error::Error>> {
        let cert = std::fs::read_to_string(&self.cert_path)?;
        let key = std::fs::read_to_string(&self.key_path)?;

        let identity = Identity::from_pem(cert, key);
        let mut tls_config = ServerTlsConfig::new().identity(identity);

        if let Some(ca_path) = &self.ca_path {
            let ca = std::fs::read_to_string(ca_path)?;
            let ca_cert = tonic::transport::Certificate::from_pem(ca);
            tls_config = tls_config.client_ca_root(ca_cert);
        }

        Ok(tls_config)
    }
}
```

**Implementação Client:**
```rust
// client/src/tls.rs
use tonic::transport::{Certificate, ClientTlsConfig};

pub struct ClientTls {
    pub enabled: bool,
    pub ca_path: Option<String>,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub skip_verify: bool,  // Dev only!
}

impl ClientTls {
    pub fn load_config(&self) -> Result<ClientTlsConfig, Box<dyn std::error::Error>> {
        let mut tls = ClientTlsConfig::new();

        if let Some(ca_path) = &self.ca_path {
            let ca = std::fs::read_to_string(ca_path)?;
            let ca_cert = Certificate::from_pem(ca);
            tls = tls.ca_certificate(ca_cert);
        }

        if let (Some(cert_path), Some(key_path)) = (&self.cert_path, &self.key_path) {
            let cert = std::fs::read_to_string(cert_path)?;
            let key = std::fs::read_to_string(key_path)?;
            let identity = tonic::transport::Identity::from_pem(cert, key);
            tls = tls.identity(identity);
        }

        Ok(tls)
    }
}
```

**Config TOML:**
```toml
# config.toml (server)
[tls]
enabled = true
cert_path = "/etc/code-monitor/server.crt"
key_path = "/etc/code-monitor/server.key"
# ca_path = "/etc/code-monitor/ca.crt"  # For mTLS

# client-config.toml
[tls]
enabled = true
# ca_path = "/etc/code-monitor/ca.crt"  # Custom CA
# skip_verify = false  # Never in production!
```

---

### 2. Sistema de Histórico (SQLite)

**Objetivo:** Persistir métricas para análise temporal

**Schema:**
```sql
-- migrations/001_initial.sql

-- Métricas de sistema
CREATE TABLE IF NOT EXISTS system_metrics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,  -- Unix timestamp
    cpu_usage REAL NOT NULL,
    memory_used INTEGER NOT NULL,
    memory_total INTEGER NOT NULL,
    created_at INTEGER DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_system_metrics_server_time
    ON system_metrics(server_id, timestamp DESC);

-- Métricas de disco
CREATE TABLE IF NOT EXISTS disk_metrics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    mount_point TEXT NOT NULL,
    used_bytes INTEGER NOT NULL,
    total_bytes INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_disk_metrics_server_time
    ON disk_metrics(server_id, timestamp DESC);

-- Alertas disparados
CREATE TABLE IF NOT EXISTS alert_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id TEXT NOT NULL,
    alert_type TEXT NOT NULL,
    severity TEXT NOT NULL,
    message TEXT NOT NULL,
    value REAL,
    threshold REAL,
    triggered_at INTEGER NOT NULL,
    resolved_at INTEGER,
    acknowledged_at INTEGER,
    acknowledged_by TEXT
);

CREATE INDEX IF NOT EXISTS idx_alert_history_server
    ON alert_history(server_id, triggered_at DESC);
```

**Implementação:**
```rust
// client/src/storage.rs
use rusqlite::{Connection, params};
use std::path::PathBuf;

pub struct MetricsStorage {
    conn: Connection,
    retention_days: u32,
}

impl MetricsStorage {
    pub fn new(db_path: PathBuf, retention_days: u32) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(include_str!("../migrations/001_initial.sql"))?;

        Ok(Self { conn, retention_days })
    }

    pub fn store_metrics(&self, server_id: &str, metrics: &SystemMetrics) -> Result<()> {
        self.conn.execute(
            "INSERT INTO system_metrics (server_id, timestamp, cpu_usage, memory_used, memory_total)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                server_id,
                metrics.timestamp,
                metrics.cpu_usage,
                metrics.memory_used,
                metrics.memory_total,
            ],
        )?;
        Ok(())
    }

    pub fn get_history(
        &self,
        server_id: &str,
        from: i64,
        to: i64,
    ) -> Result<Vec<SystemMetrics>> {
        let mut stmt = self.conn.prepare(
            "SELECT timestamp, cpu_usage, memory_used, memory_total
             FROM system_metrics
             WHERE server_id = ?1 AND timestamp BETWEEN ?2 AND ?3
             ORDER BY timestamp ASC"
        )?;

        let rows = stmt.query_map(params![server_id, from, to], |row| {
            Ok(SystemMetrics {
                timestamp: row.get(0)?,
                cpu_usage: row.get(1)?,
                memory_used: row.get(2)?,
                memory_total: row.get(3)?,
            })
        })?;

        rows.collect()
    }

    pub fn cleanup_old(&self) -> Result<usize> {
        let cutoff = chrono::Utc::now().timestamp() - (self.retention_days as i64 * 86400);

        let deleted = self.conn.execute(
            "DELETE FROM system_metrics WHERE timestamp < ?1",
            params![cutoff],
        )?;

        Ok(deleted)
    }
}
```

---

### 3. Sistema de Alertas

**Objetivo:** Notificar quando thresholds são atingidos

**Tipos de Alerta:**
```rust
// shared/src/alerts.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertType {
    CpuHigh {
        threshold: f64,
        duration_secs: u64,
    },
    MemoryHigh {
        threshold: f64,
    },
    DiskHigh {
        threshold: f64,
        mount_point: Option<String>,
    },
    ProcessDown {
        process_name: String,
    },
    ServerUnreachable {
        timeout_secs: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: String,
    pub name: String,
    pub alert_type: AlertType,
    pub severity: AlertSeverity,
    pub enabled: bool,
    pub notify: Vec<NotifyChannel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotifyChannel {
    Webhook { url: String },
    Email { to: String },
    Slack { webhook_url: String },
    Discord { webhook_url: String },
}
```

**Alert Evaluator:**
```rust
// client/src/alerts/evaluator.rs

pub struct AlertEvaluator {
    rules: Vec<AlertRule>,
    states: HashMap<String, AlertState>,
    storage: Arc<MetricsStorage>,
}

struct AlertState {
    triggered: bool,
    triggered_at: Option<DateTime<Utc>>,
    last_value: f64,
    consecutive_failures: u32,
}

impl AlertEvaluator {
    pub async fn evaluate(&mut self, server: &ServerState) -> Vec<TriggeredAlert> {
        let mut alerts = Vec::new();

        for rule in &self.rules {
            if !rule.enabled {
                continue;
            }

            let result = self.evaluate_rule(rule, server).await;

            if let Some(alert) = result {
                alerts.push(alert);
            }
        }

        alerts
    }

    async fn evaluate_rule(
        &mut self,
        rule: &AlertRule,
        server: &ServerState,
    ) -> Option<TriggeredAlert> {
        match &rule.alert_type {
            AlertType::CpuHigh { threshold, duration_secs } => {
                let avg_cpu = self.get_avg_cpu(server.id, *duration_secs).await;

                if avg_cpu > *threshold {
                    Some(TriggeredAlert {
                        rule_id: rule.id.clone(),
                        server_id: server.id.clone(),
                        message: format!("CPU at {:.1}% (threshold: {:.1}%)", avg_cpu, threshold),
                        value: avg_cpu,
                        threshold: *threshold,
                    })
                } else {
                    None
                }
            }
            AlertType::MemoryHigh { threshold } => {
                let mem_percent = server.metrics.memory_percent();

                if mem_percent > *threshold {
                    Some(TriggeredAlert {
                        rule_id: rule.id.clone(),
                        server_id: server.id.clone(),
                        message: format!("Memory at {:.1}% (threshold: {:.1}%)", mem_percent, threshold),
                        value: mem_percent,
                        threshold: *threshold,
                    })
                } else {
                    None
                }
            }
            // ... other types
        }
    }
}
```

**Notifier:**
```rust
// client/src/alerts/notifier.rs

pub struct Notifier {
    http_client: reqwest::Client,
}

impl Notifier {
    pub async fn send(&self, alert: &TriggeredAlert, channels: &[NotifyChannel]) -> Result<()> {
        for channel in channels {
            match channel {
                NotifyChannel::Webhook { url } => {
                    self.send_webhook(url, alert).await?;
                }
                NotifyChannel::Slack { webhook_url } => {
                    self.send_slack(webhook_url, alert).await?;
                }
                NotifyChannel::Discord { webhook_url } => {
                    self.send_discord(webhook_url, alert).await?;
                }
                NotifyChannel::Email { to } => {
                    self.send_email(to, alert).await?;
                }
            }
        }
        Ok(())
    }

    async fn send_webhook(&self, url: &str, alert: &TriggeredAlert) -> Result<()> {
        let payload = serde_json::json!({
            "event": "alert.triggered",
            "alert": {
                "rule_id": alert.rule_id,
                "server_id": alert.server_id,
                "message": alert.message,
                "value": alert.value,
                "threshold": alert.threshold,
                "triggered_at": Utc::now().to_rfc3339(),
            }
        });

        self.http_client
            .post(url)
            .json(&payload)
            .send()
            .await?;

        Ok(())
    }

    async fn send_slack(&self, webhook_url: &str, alert: &TriggeredAlert) -> Result<()> {
        let payload = serde_json::json!({
            "text": format!("🚨 *{}*\n{}", alert.server_id, alert.message),
            "attachments": [{
                "color": "danger",
                "fields": [
                    {"title": "Value", "value": format!("{:.2}", alert.value), "short": true},
                    {"title": "Threshold", "value": format!("{:.2}", alert.threshold), "short": true},
                ]
            }]
        });

        self.http_client
            .post(webhook_url)
            .json(&payload)
            .send()
            .await?;

        Ok(())
    }
}
```

---

### 4. Health Check Endpoints

**Objetivo:** Endpoints HTTP para monitorar o próprio Code Monitor

**Implementação:**
```rust
// server/src/health.rs

use axum::{routing::get, Json, Router};
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
    uptime_seconds: u64,
}

#[derive(Serialize)]
struct ReadyResponse {
    status: String,
    checks: Vec<HealthCheck>,
}

#[derive(Serialize)]
struct HealthCheck {
    name: String,
    status: String,
    message: Option<String>,
}

pub fn health_routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .route("/metrics", get(metrics_handler))
        .with_state(state)
}

async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: state.start_time.elapsed().as_secs(),
    })
}

async fn ready_handler(State(state): State<Arc<AppState>>) -> (StatusCode, Json<ReadyResponse>) {
    let checks = vec![
        HealthCheck {
            name: "grpc_server".to_string(),
            status: if state.grpc_ready { "ok" } else { "fail" }.to_string(),
            message: None,
        },
        HealthCheck {
            name: "storage".to_string(),
            status: if state.storage.is_available() { "ok" } else { "fail" }.to_string(),
            message: None,
        },
    ];

    let all_ok = checks.iter().all(|c| c.status == "ok");
    let status = if all_ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };

    (status, Json(ReadyResponse {
        status: if all_ok { "ready" } else { "not_ready" }.to_string(),
        checks,
    }))
}

async fn metrics_handler(State(state): State<Arc<AppState>>) -> String {
    // Prometheus format
    format!(
        r#"# HELP code_monitor_uptime_seconds Uptime in seconds
# TYPE code_monitor_uptime_seconds gauge
code_monitor_uptime_seconds {}

# HELP code_monitor_clients_connected Number of connected clients
# TYPE code_monitor_clients_connected gauge
code_monitor_clients_connected {}

# HELP code_monitor_metrics_received_total Total metrics received
# TYPE code_monitor_metrics_received_total counter
code_monitor_metrics_received_total {}
"#,
        state.start_time.elapsed().as_secs(),
        state.connected_clients.load(Ordering::Relaxed),
        state.metrics_received.load(Ordering::Relaxed),
    )
}
```

---

### 5. CI/CD Pipeline

**GitHub Actions:**
```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    name: Check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo check --all-targets

  test:
    name: Test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --all

  fmt:
    name: Format
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt
      - run: cargo fmt --all --check

  clippy:
    name: Clippy
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo clippy --all-targets -- -D warnings

  build:
    name: Build
    needs: [check, test, fmt, clippy]
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
          - os: windows-latest
            target: x86_64-pc-windows-msvc
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --release --target ${{ matrix.target }}
      - uses: actions/upload-artifact@v4
        with:
          name: binaries-${{ matrix.target }}
          path: |
            target/${{ matrix.target }}/release/monitor-server*
            target/${{ matrix.target }}/release/monitor-client*
```

---

## Checklist de Implementação

### Semana 1-2
- [ ] TLS: Server implementation
- [ ] TLS: Client implementation
- [ ] TLS: Config options
- [ ] TLS: Documentation

### Semana 2-3
- [ ] SQLite: Schema e migrations
- [ ] SQLite: Store metrics
- [ ] SQLite: Query history
- [ ] SQLite: Cleanup job

### Semana 3-4
- [ ] Alerts: Rule types
- [ ] Alerts: Evaluator
- [ ] Alerts: Notifier (webhook)
- [ ] Alerts: Notifier (Slack)
- [ ] Alerts: TUI integration

### Semana 5-6
- [ ] Health: /health endpoint
- [ ] Health: /ready endpoint
- [ ] Health: /metrics endpoint
- [ ] CI/CD: GitHub Actions
- [ ] CI/CD: Release automation

---

## Próximos Passos

- [Implementação Médio Prazo](./implementacao-medio-prazo.md) - Web, SaaS
- [Melhorias de Infra](./melhorias-infra.md) - DB, cache, deploy
