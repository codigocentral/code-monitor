# Arquitetura Futura

## Visao Geral

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    ARQUITETURA CODE MONITOR v2.0                            │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│                           ┌─────────────────┐                              │
│                           │    USUARIOS     │                              │
│                           └────────┬────────┘                              │
│                                    │                                        │
│              ┌─────────────────────┼─────────────────────┐                 │
│              │                     │                     │                 │
│              ▼                     ▼                     ▼                 │
│     ┌─────────────┐      ┌─────────────┐       ┌─────────────┐            │
│     │  TUI Client │      │ Web Client  │       │ Mobile App  │            │
│     │   (Rust)    │      │  (React)    │       │   (RN/FL)   │            │
│     └──────┬──────┘      └──────┬──────┘       └──────┬──────┘            │
│            │                    │                     │                    │
│            └────────────────────┼─────────────────────┘                    │
│                                 │                                          │
│                                 ▼                                          │
│     ┌───────────────────────────────────────────────────────────────────┐  │
│     │                       API GATEWAY                                 │  │
│     │              (Rate Limit, Auth, Routing)                          │  │
│     └───────────────────────────────────────────────────────────────────┘  │
│                                 │                                          │
│         ┌───────────────────────┼───────────────────────┐                  │
│         │                       │                       │                  │
│         ▼                       ▼                       ▼                  │
│   ┌───────────┐          ┌───────────┐           ┌───────────┐            │
│   │ REST API  │          │ WebSocket │           │  gRPC     │            │
│   │  (Axum)   │          │  (Tokio)  │           │  (Tonic)  │            │
│   └─────┬─────┘          └─────┬─────┘           └─────┬─────┘            │
│         │                      │                       │                  │
│         └──────────────────────┼───────────────────────┘                  │
│                                │                                          │
│                                ▼                                          │
│     ┌───────────────────────────────────────────────────────────────────┐  │
│     │                      CORE SERVICES                                │  │
│     │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐          │  │
│     │  │ Metrics  │  │  Alerts  │  │  Users   │  │ Billing  │          │  │
│     │  │ Service  │  │ Service  │  │ Service  │  │ Service  │          │  │
│     │  └──────────┘  └──────────┘  └──────────┘  └──────────┘          │  │
│     └───────────────────────────────────────────────────────────────────┘  │
│                                 │                                          │
│         ┌───────────────────────┼───────────────────────┐                  │
│         │                       │                       │                  │
│         ▼                       ▼                       ▼                  │
│   ┌───────────┐          ┌───────────┐           ┌───────────┐            │
│   │PostgreSQL │          │TimescaleDB│           │   Redis   │            │
│   │ (config)  │          │ (metrics) │           │  (cache)  │            │
│   └───────────┘          └───────────┘           └───────────┘            │
│                                                                            │
│     ┌───────────────────────────────────────────────────────────────────┐  │
│     │                       AGENTS                                      │  │
│     │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐          │  │
│     │  │  Linux   │  │ Windows  │  │   K8s    │  │  Docker  │          │  │
│     │  │  Agent   │  │  Agent   │  │  Agent   │  │  Agent   │          │  │
│     │  └──────────┘  └──────────┘  └──────────┘  └──────────┘          │  │
│     └───────────────────────────────────────────────────────────────────┘  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Componentes Principais

### 1. Agent Layer

**Responsabilidade:** Coletar métricas dos hosts monitorados

```rust
// Agent architecture
pub struct Agent {
    config: AgentConfig,
    collectors: Vec<Box<dyn Collector>>,
    sender: MetricsSender,
    buffer: MetricsBuffer,
}

pub trait Collector: Send + Sync {
    fn name(&self) -> &str;
    fn collect(&self) -> Vec<Metric>;
    fn interval(&self) -> Duration;
}

// Collectors disponíveis
impl Agent {
    pub fn default_collectors() -> Vec<Box<dyn Collector>> {
        vec![
            Box::new(CpuCollector::new()),
            Box::new(MemoryCollector::new()),
            Box::new(DiskCollector::new()),
            Box::new(NetworkCollector::new()),
            Box::new(ProcessCollector::new()),
            Box::new(ServiceCollector::new()),
        ]
    }
}
```

**Tipos de Agent:**

| Agent | Target | Features |
|-------|--------|----------|
| Linux | Ubuntu, Debian, CentOS, etc. | Full metrics |
| Windows | Server, Desktop | Full metrics |
| K8s | Kubernetes clusters | Pods, Services, Deployments |
| Docker | Docker hosts | Containers, Images |

### 2. Core Services

**Metrics Service:**
```rust
pub struct MetricsService {
    storage: TimescaleDB,
    cache: Redis,
    aggregator: MetricsAggregator,
}

impl MetricsService {
    // Ingest de métricas dos agents
    pub async fn ingest(&self, metrics: Vec<Metric>) -> Result<()>;

    // Query de métricas
    pub async fn query(&self, q: MetricsQuery) -> Result<Vec<Metric>>;

    // Aggregations
    pub async fn aggregate(&self, q: AggregationQuery) -> Result<Aggregation>;
}
```

**Alerts Service:**
```rust
pub struct AlertsService {
    evaluator: AlertEvaluator,
    notifier: Notifier,
    storage: PostgreSQL,
}

impl AlertsService {
    // Avalia condições de alerta
    pub async fn evaluate(&self) -> Result<Vec<Alert>>;

    // Envia notificações
    pub async fn notify(&self, alert: &Alert) -> Result<()>;

    // Gerenciamento de alertas
    pub async fn acknowledge(&self, id: AlertId) -> Result<()>;
    pub async fn resolve(&self, id: AlertId) -> Result<()>;
}
```

### 3. API Layer

**REST API (Axum):**
```rust
pub fn api_routes() -> Router {
    Router::new()
        // Servers
        .route("/api/v1/servers", get(list_servers))
        .route("/api/v1/servers/:id", get(get_server))
        .route("/api/v1/servers/:id/metrics", get(get_metrics))

        // Alerts
        .route("/api/v1/alerts", get(list_alerts))
        .route("/api/v1/alerts/:id/acknowledge", post(acknowledge_alert))

        // Dashboards
        .route("/api/v1/dashboards", get(list_dashboards).post(create_dashboard))
        .route("/api/v1/dashboards/:id", get(get_dashboard).put(update_dashboard))

        // Users (admin)
        .route("/api/v1/users", get(list_users).post(create_user))

        // Middleware
        .layer(AuthLayer::new())
        .layer(RateLimitLayer::new())
        .layer(TraceLayer::new())
}
```

**WebSocket (Real-time):**
```rust
pub async fn ws_handler(ws: WebSocketUpgrade, state: AppState) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (sender, receiver) = socket.split();

    // Subscribe to metrics updates
    let mut rx = state.metrics_broadcast.subscribe();

    tokio::spawn(async move {
        while let Ok(metrics) = rx.recv().await {
            if sender.send(Message::Text(serde_json::to_string(&metrics)?)).await.is_err() {
                break;
            }
        }
    });
}
```

### 4. Storage Layer

**PostgreSQL (Config/Users):**
```sql
-- Users
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email VARCHAR(255) UNIQUE NOT NULL,
    password_hash VARCHAR(255),
    org_id UUID REFERENCES organizations(id),
    role VARCHAR(50) NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Servers
CREATE TABLE servers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID REFERENCES organizations(id),
    name VARCHAR(255) NOT NULL,
    hostname VARCHAR(255),
    ip_address INET,
    status VARCHAR(50),
    last_seen_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Alert Rules
CREATE TABLE alert_rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID REFERENCES organizations(id),
    name VARCHAR(255) NOT NULL,
    condition JSONB NOT NULL,
    severity VARCHAR(50) NOT NULL,
    enabled BOOLEAN DEFAULT true,
    created_at TIMESTAMPTZ DEFAULT NOW()
);
```

**TimescaleDB (Metrics):**
```sql
-- Hypertable para métricas
CREATE TABLE metrics (
    time TIMESTAMPTZ NOT NULL,
    server_id UUID NOT NULL,
    org_id UUID NOT NULL,
    metric_name VARCHAR(255) NOT NULL,
    value DOUBLE PRECISION NOT NULL,
    tags JSONB
);

SELECT create_hypertable('metrics', 'time');

-- Compression policy
ALTER TABLE metrics SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'server_id,metric_name'
);

SELECT add_compression_policy('metrics', INTERVAL '7 days');

-- Retention policy
SELECT add_retention_policy('metrics', INTERVAL '90 days');

-- Continuous aggregates (downsampling)
CREATE MATERIALIZED VIEW metrics_hourly
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', time) AS bucket,
    server_id,
    metric_name,
    AVG(value) as avg_value,
    MAX(value) as max_value,
    MIN(value) as min_value
FROM metrics
GROUP BY bucket, server_id, metric_name;
```

**Redis (Cache):**
```
# Cache patterns

# Current metrics (latest)
metrics:current:{server_id} -> JSON{cpu, memory, disk, ...}
TTL: 30s

# Server status
server:status:{server_id} -> "online"|"offline"|"warning"
TTL: 60s

# Alert states
alert:state:{alert_id} -> JSON{state, last_triggered, count}
TTL: none (persistent)

# User sessions
session:{session_id} -> JSON{user_id, org_id, permissions}
TTL: 24h

# Rate limiting
ratelimit:{ip}:{endpoint} -> count
TTL: 1min
```

---

## Fluxos Criticos

### Ingest de Métricas

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    METRICS INGESTION FLOW                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Agent                                                                     │
│     │                                                                       │
│     │ 1. Collect metrics (every 5s)                                        │
│     ▼                                                                       │
│   ┌─────────────┐                                                          │
│   │   Buffer    │  ← Batch metrics locally                                 │
│   └──────┬──────┘                                                          │
│          │                                                                  │
│          │ 2. Batch send (every 10s or 100 metrics)                        │
│          ▼                                                                  │
│   ┌─────────────┐                                                          │
│   │ gRPC Stream │  ← Compressed, authenticated                             │
│   └──────┬──────┘                                                          │
│          │                                                                  │
│          │ 3. Receive batch                                                │
│          ▼                                                                  │
│   ┌─────────────┐                                                          │
│   │ Metrics API │                                                          │
│   └──────┬──────┘                                                          │
│          │                                                                  │
│          ├──────────────────────────────────────┐                          │
│          │                                      │                          │
│          ▼                                      ▼                          │
│   ┌─────────────┐                        ┌─────────────┐                   │
│   │TimescaleDB  │                        │   Redis     │                   │
│   │ (persist)   │                        │ (broadcast) │                   │
│   └─────────────┘                        └──────┬──────┘                   │
│                                                 │                          │
│                                                 │ 4. Publish                │
│                                                 ▼                          │
│                                          ┌─────────────┐                   │
│                                          │ WebSocket   │                   │
│                                          │ subscribers │                   │
│                                          └─────────────┘                   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Avaliação de Alertas

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    ALERT EVALUATION FLOW                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Every 30s:                                                               │
│                                                                             │
│   ┌─────────────┐                                                          │
│   │Alert Engine │                                                          │
│   └──────┬──────┘                                                          │
│          │                                                                  │
│          │ 1. Load active rules                                            │
│          ▼                                                                  │
│   ┌─────────────┐                                                          │
│   │ PostgreSQL  │ → [rule1, rule2, rule3, ...]                             │
│   └──────┬──────┘                                                          │
│          │                                                                  │
│          │ 2. For each rule, query metrics                                 │
│          ▼                                                                  │
│   ┌─────────────┐                                                          │
│   │TimescaleDB  │ → SELECT avg(value) WHERE metric = 'cpu' ...             │
│   └──────┬──────┘                                                          │
│          │                                                                  │
│          │ 3. Evaluate condition                                           │
│          ▼                                                                  │
│   ┌─────────────┐                                                          │
│   │  Evaluator  │ → if value > threshold for duration                      │
│   └──────┬──────┘                                                          │
│          │                                                                  │
│          │ 4. If triggered, dedupe and notify                              │
│          ▼                                                                  │
│   ┌─────────────┐     ┌─────────────┐     ┌─────────────┐                  │
│   │   Redis     │────►│  Notifier   │────►│  Webhooks   │                  │
│   │ (deduping)  │     │             │     │  Email      │                  │
│   └─────────────┘     └─────────────┘     │  Slack      │                  │
│                                           └─────────────┘                  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Decisoes de Arquitetura

### Por que Axum (não Actix)?

| Aspecto | Axum | Actix |
|---------|------|-------|
| Ecosystem | Tokio nativo | Actor model |
| Learning curve | Menor | Maior |
| Composability | Excelente (tower) | Boa |
| Maintenance | Ativo (Tokio team) | Ativo |
| Performance | Similar | Similar |

**Decisão:** Axum pela simplicidade e integração com Tokio

### Por que TimescaleDB (não InfluxDB)?

| Aspecto | TimescaleDB | InfluxDB |
|---------|-------------|----------|
| Query language | SQL | InfluxQL/Flux |
| Schema | Relational | Schema-less |
| Joins | Suportado | Limitado |
| Compression | Excelente | Boa |
| Ecosystem | PostgreSQL | Próprio |

**Decisão:** TimescaleDB pelo SQL e ecossistema PostgreSQL

### Por que Redis (não Memcached)?

| Aspecto | Redis | Memcached |
|---------|-------|-----------|
| Data structures | Rich | Simple |
| Pub/Sub | Nativo | Não |
| Persistence | Opcional | Não |
| Clustering | Redis Cluster | Externo |

**Decisão:** Redis pelo pub/sub e estruturas de dados

---

## Escalabilidade

### Horizontal Scaling

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    HORIZONTAL SCALING                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Load Balancer (HAProxy/nginx)                                            │
│         │                                                                   │
│         ├────────────────┬────────────────┐                                │
│         ▼                ▼                ▼                                │
│   ┌───────────┐    ┌───────────┐    ┌───────────┐                          │
│   │  API #1   │    │  API #2   │    │  API #3   │  ← Stateless            │
│   └─────┬─────┘    └─────┬─────┘    └─────┬─────┘                          │
│         │                │                │                                │
│         └────────────────┼────────────────┘                                │
│                          │                                                  │
│                          ▼                                                  │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                      Redis Cluster                                  │   │
│   │   (session, cache, pub/sub)                                         │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                          │                                                  │
│   ┌──────────────────────┼──────────────────────┐                          │
│   │                      │                      │                          │
│   ▼                      ▼                      ▼                          │
│ ┌─────────┐        ┌─────────────┐        ┌─────────────┐                  │
│ │PostgreSQL        │ TimescaleDB │        │ TimescaleDB │                  │
│ │ Primary │        │  Primary    │        │  Replica    │                  │
│ └─────────┘        └─────────────┘        └─────────────┘                  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Capacity Planning

| Nodes | API Instances | DB Size/month | Redis Memory |
|-------|--------------|---------------|--------------|
| 100 | 1 | 5GB | 512MB |
| 500 | 2 | 25GB | 1GB |
| 2000 | 4 | 100GB | 4GB |
| 10000 | 8 | 500GB | 16GB |

---

## Próximos Passos

- [Implementação Curto Prazo](./implementacao-curto-prazo.md) - Código para agora
- [Implementação Médio Prazo](./implementacao-medio-prazo.md) - Web, Cloud, IA
