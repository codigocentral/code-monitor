# Melhorias de Infraestrutura

## Database Strategy

### Evolução do Storage

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    EVOLUÇÃO DO STORAGE                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   FASE 1 (MVP)                                                             │
│   └── SQLite (client-side)                                                 │
│       └── Histórico local, simples, zero config                            │
│                                                                             │
│   FASE 2 (SaaS)                                                            │
│   ├── PostgreSQL (config, users, billing)                                  │
│   └── TimescaleDB (metrics time-series)                                    │
│                                                                             │
│   FASE 3 (Scale)                                                           │
│   ├── PostgreSQL + Read Replicas                                           │
│   ├── TimescaleDB + Continuous Aggregates                                  │
│   └── Redis Cluster (cache, pub/sub)                                       │
│                                                                             │
│   FASE 4 (Enterprise)                                                      │
│   ├── PostgreSQL HA (Patroni)                                              │
│   ├── TimescaleDB Multi-node                                               │
│   └── Redis Sentinel                                                       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### PostgreSQL Setup

**Schema Principal:**
```sql
-- Organizations
CREATE TABLE organizations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(100) UNIQUE NOT NULL,
    plan VARCHAR(50) NOT NULL DEFAULT 'community',
    stripe_customer_id VARCHAR(255),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Users
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID REFERENCES organizations(id) ON DELETE CASCADE,
    email VARCHAR(255) NOT NULL,
    password_hash VARCHAR(255),
    role VARCHAR(50) NOT NULL DEFAULT 'member',
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(org_id, email)
);

-- Servers
CREATE TABLE servers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID REFERENCES organizations(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    hostname VARCHAR(255),
    ip_address INET,
    access_token_hash VARCHAR(255) NOT NULL,
    last_seen_at TIMESTAMPTZ,
    status VARCHAR(50) DEFAULT 'unknown',
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_servers_org ON servers(org_id);
CREATE INDEX idx_servers_last_seen ON servers(last_seen_at);

-- Alert Rules
CREATE TABLE alert_rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID REFERENCES organizations(id) ON DELETE CASCADE,
    server_id UUID REFERENCES servers(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    type VARCHAR(50) NOT NULL,
    condition JSONB NOT NULL,
    severity VARCHAR(20) NOT NULL,
    enabled BOOLEAN DEFAULT true,
    notify_channels JSONB DEFAULT '[]',
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Alert History
CREATE TABLE alert_history (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rule_id UUID REFERENCES alert_rules(id) ON DELETE SET NULL,
    server_id UUID REFERENCES servers(id) ON DELETE CASCADE,
    org_id UUID REFERENCES organizations(id) ON DELETE CASCADE,
    severity VARCHAR(20) NOT NULL,
    message TEXT NOT NULL,
    value DOUBLE PRECISION,
    threshold DOUBLE PRECISION,
    triggered_at TIMESTAMPTZ NOT NULL,
    resolved_at TIMESTAMPTZ,
    acknowledged_at TIMESTAMPTZ,
    acknowledged_by UUID REFERENCES users(id)
);

CREATE INDEX idx_alert_history_org ON alert_history(org_id, triggered_at DESC);

-- Row Level Security
ALTER TABLE servers ENABLE ROW LEVEL SECURITY;
ALTER TABLE alert_rules ENABLE ROW LEVEL SECURITY;
ALTER TABLE alert_history ENABLE ROW LEVEL SECURITY;

CREATE POLICY servers_org_isolation ON servers
    USING (org_id = current_setting('app.current_org_id')::uuid);

CREATE POLICY alert_rules_org_isolation ON alert_rules
    USING (org_id = current_setting('app.current_org_id')::uuid);

CREATE POLICY alert_history_org_isolation ON alert_history
    USING (org_id = current_setting('app.current_org_id')::uuid);
```

### TimescaleDB Setup

**Metrics Schema:**
```sql
-- Create extension
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- Metrics hypertable
CREATE TABLE metrics (
    time TIMESTAMPTZ NOT NULL,
    org_id UUID NOT NULL,
    server_id UUID NOT NULL,
    metric_name VARCHAR(100) NOT NULL,
    value DOUBLE PRECISION NOT NULL,
    tags JSONB DEFAULT '{}'
);

SELECT create_hypertable('metrics', 'time',
    chunk_time_interval => INTERVAL '1 day'
);

-- Indices
CREATE INDEX idx_metrics_server ON metrics(server_id, time DESC);
CREATE INDEX idx_metrics_name ON metrics(metric_name, time DESC);
CREATE INDEX idx_metrics_org ON metrics(org_id, time DESC);

-- Compression
ALTER TABLE metrics SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'org_id, server_id, metric_name'
);

SELECT add_compression_policy('metrics', INTERVAL '7 days');

-- Retention (ajustar por tier)
-- Community: 1 day
-- Pro: 7 days
-- Business: 90 days
SELECT add_retention_policy('metrics', INTERVAL '90 days');

-- Continuous Aggregates (downsampling)
CREATE MATERIALIZED VIEW metrics_hourly
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', time) AS bucket,
    org_id,
    server_id,
    metric_name,
    AVG(value) AS avg_value,
    MAX(value) AS max_value,
    MIN(value) AS min_value,
    COUNT(*) AS sample_count
FROM metrics
GROUP BY bucket, org_id, server_id, metric_name
WITH NO DATA;

SELECT add_continuous_aggregate_policy('metrics_hourly',
    start_offset => INTERVAL '3 hours',
    end_offset => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour'
);

-- Daily aggregates para historical views
CREATE MATERIALIZED VIEW metrics_daily
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 day', time) AS bucket,
    org_id,
    server_id,
    metric_name,
    AVG(value) AS avg_value,
    MAX(value) AS max_value,
    MIN(value) AS min_value,
    percentile_cont(0.95) WITHIN GROUP (ORDER BY value) AS p95_value,
    COUNT(*) AS sample_count
FROM metrics
GROUP BY bucket, org_id, server_id, metric_name
WITH NO DATA;
```

### Redis Configuration

**Use Cases:**
```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    REDIS USE CASES                                          │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   1. CACHING                                                               │
│   ├── Current metrics (per server)                                         │
│   ├── Server status                                                        │
│   ├── User sessions                                                        │
│   └── API responses                                                        │
│                                                                             │
│   2. PUB/SUB                                                               │
│   ├── Real-time metrics broadcast                                          │
│   ├── Alert notifications                                                  │
│   └── Server status changes                                                │
│                                                                             │
│   3. RATE LIMITING                                                         │
│   ├── API rate limits                                                      │
│   └── Login attempt limits                                                 │
│                                                                             │
│   4. DISTRIBUTED LOCKS                                                     │
│   ├── Alert evaluation (prevent duplicates)                                │
│   └── Cleanup jobs                                                         │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Redis Patterns:**
```rust
// Cache patterns
impl RedisCache {
    // Current metrics (last known values)
    pub async fn set_current_metrics(&self, server_id: &str, metrics: &Metrics) -> Result<()> {
        let key = format!("metrics:current:{}", server_id);
        let value = serde_json::to_string(metrics)?;
        self.conn.set_ex(&key, value, 30).await?;  // 30s TTL
        Ok(())
    }

    pub async fn get_current_metrics(&self, server_id: &str) -> Result<Option<Metrics>> {
        let key = format!("metrics:current:{}", server_id);
        let value: Option<String> = self.conn.get(&key).await?;
        value.map(|v| serde_json::from_str(&v)).transpose()
    }

    // Session
    pub async fn set_session(&self, session_id: &str, data: &SessionData) -> Result<()> {
        let key = format!("session:{}", session_id);
        let value = serde_json::to_string(data)?;
        self.conn.set_ex(&key, value, 86400).await?;  // 24h TTL
        Ok(())
    }

    // Rate limiting
    pub async fn check_rate_limit(&self, key: &str, limit: u64, window: u64) -> Result<bool> {
        let current: u64 = self.conn.incr(&key, 1).await?;
        if current == 1 {
            self.conn.expire(&key, window as usize).await?;
        }
        Ok(current <= limit)
    }
}

// Pub/Sub
impl RedisPubSub {
    pub async fn publish_metrics(&self, server_id: &str, metrics: &Metrics) -> Result<()> {
        let channel = format!("metrics:{}", server_id);
        let message = serde_json::to_string(metrics)?;
        self.conn.publish(&channel, message).await?;
        Ok(())
    }

    pub async fn subscribe_metrics(&self, server_id: &str) -> impl Stream<Item = Metrics> {
        let channel = format!("metrics:{}", server_id);
        self.conn.subscribe(&channel).await
            .filter_map(|msg| {
                serde_json::from_str(&msg.payload).ok()
            })
    }
}
```

---

## Deployment Strategy

### Docker Compose (Development/Small)

```yaml
# docker-compose.yml
version: '3.8'

services:
  api:
    build: .
    ports:
      - "8080:8080"
      - "50051:50051"
    environment:
      - DATABASE_URL=postgres://codemonitor:secret@postgres:5432/codemonitor
      - TIMESCALE_URL=postgres://codemonitor:secret@timescale:5432/metrics
      - REDIS_URL=redis://redis:6379
      - JWT_SECRET=${JWT_SECRET}
    depends_on:
      - postgres
      - timescale
      - redis

  postgres:
    image: postgres:15
    environment:
      POSTGRES_USER: codemonitor
      POSTGRES_PASSWORD: secret
      POSTGRES_DB: codemonitor
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./init.sql:/docker-entrypoint-initdb.d/init.sql

  timescale:
    image: timescale/timescaledb:latest-pg15
    environment:
      POSTGRES_USER: codemonitor
      POSTGRES_PASSWORD: secret
      POSTGRES_DB: metrics
    volumes:
      - timescale_data:/var/lib/postgresql/data
      - ./timescale-init.sql:/docker-entrypoint-initdb.d/init.sql

  redis:
    image: redis:7-alpine
    volumes:
      - redis_data:/data

  web:
    build: ./web
    ports:
      - "3000:80"
    depends_on:
      - api

volumes:
  postgres_data:
  timescale_data:
  redis_data:
```

### Kubernetes (Production)

```yaml
# k8s/deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: codemonitor-api
spec:
  replicas: 3
  selector:
    matchLabels:
      app: codemonitor-api
  template:
    metadata:
      labels:
        app: codemonitor-api
    spec:
      containers:
        - name: api
          image: codemonitor/api:latest
          ports:
            - containerPort: 8080
            - containerPort: 50051
          resources:
            requests:
              cpu: 100m
              memory: 256Mi
            limits:
              cpu: 500m
              memory: 512Mi
          env:
            - name: DATABASE_URL
              valueFrom:
                secretKeyRef:
                  name: codemonitor-secrets
                  key: database-url
          livenessProbe:
            httpGet:
              path: /health
              port: 8080
            initialDelaySeconds: 10
            periodSeconds: 10
          readinessProbe:
            httpGet:
              path: /ready
              port: 8080
            initialDelaySeconds: 5
            periodSeconds: 5
---
apiVersion: v1
kind: Service
metadata:
  name: codemonitor-api
spec:
  selector:
    app: codemonitor-api
  ports:
    - name: http
      port: 80
      targetPort: 8080
    - name: grpc
      port: 50051
      targetPort: 50051
---
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: codemonitor-ingress
  annotations:
    kubernetes.io/ingress.class: nginx
    cert-manager.io/cluster-issuer: letsencrypt-prod
spec:
  tls:
    - hosts:
        - api.codemonitor.io
      secretName: codemonitor-tls
  rules:
    - host: api.codemonitor.io
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: codemonitor-api
                port:
                  number: 80
```

### Helm Chart

```yaml
# charts/codemonitor/values.yaml
replicaCount: 3

image:
  repository: codemonitor/api
  tag: latest
  pullPolicy: IfNotPresent

service:
  type: ClusterIP
  httpPort: 80
  grpcPort: 50051

ingress:
  enabled: true
  className: nginx
  annotations:
    cert-manager.io/cluster-issuer: letsencrypt-prod
  hosts:
    - host: api.codemonitor.io
      paths:
        - path: /
          pathType: Prefix
  tls:
    - secretName: codemonitor-tls
      hosts:
        - api.codemonitor.io

resources:
  requests:
    cpu: 100m
    memory: 256Mi
  limits:
    cpu: 500m
    memory: 512Mi

postgresql:
  enabled: true
  auth:
    postgresPassword: secret
    database: codemonitor

timescaledb:
  enabled: true
  replicaCount: 1

redis:
  enabled: true
  architecture: standalone
```

---

## Monitoring & Observability

### Metrics (Prometheus)

```rust
// server/src/metrics.rs
use prometheus::{Counter, Gauge, Histogram, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();

    pub static ref HTTP_REQUESTS_TOTAL: Counter = Counter::new(
        "http_requests_total",
        "Total HTTP requests"
    ).unwrap();

    pub static ref HTTP_REQUEST_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new(
            "http_request_duration_seconds",
            "HTTP request duration"
        ).buckets(vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0])
    ).unwrap();

    pub static ref CONNECTED_CLIENTS: Gauge = Gauge::new(
        "connected_clients",
        "Number of connected gRPC clients"
    ).unwrap();

    pub static ref METRICS_INGESTED: Counter = Counter::new(
        "metrics_ingested_total",
        "Total metrics ingested"
    ).unwrap();
}

pub fn init_metrics() {
    REGISTRY.register(Box::new(HTTP_REQUESTS_TOTAL.clone())).unwrap();
    REGISTRY.register(Box::new(HTTP_REQUEST_DURATION.clone())).unwrap();
    REGISTRY.register(Box::new(CONNECTED_CLIENTS.clone())).unwrap();
    REGISTRY.register(Box::new(METRICS_INGESTED.clone())).unwrap();
}
```

### Logging (Structured)

```rust
// server/src/logging.rs
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub fn init_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().json())
        .init();
}

// Usage
tracing::info!(
    server_id = %server_id,
    metric_count = metrics.len(),
    "Metrics ingested"
);

tracing::error!(
    error = ?err,
    user_id = %user_id,
    "Failed to process request"
);
```

### Tracing (OpenTelemetry)

```rust
// server/src/tracing.rs
use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use tracing_opentelemetry::OpenTelemetryLayer;

pub fn init_tracing() -> Result<(), Box<dyn std::error::Error>> {
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint("http://jaeger:4317"),
        )
        .with_trace_config(
            opentelemetry::sdk::trace::config()
                .with_resource(Resource::new(vec![
                    KeyValue::new("service.name", "codemonitor-api"),
                ])),
        )
        .install_batch(opentelemetry::runtime::Tokio)?;

    let telemetry = OpenTelemetryLayer::new(tracer);

    tracing_subscriber::registry()
        .with(telemetry)
        .init();

    Ok(())
}
```

---

## Backup Strategy

### PostgreSQL

```bash
#!/bin/bash
# backup-postgres.sh

TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_DIR="/backups/postgres"

# Full backup
pg_dump -Fc codemonitor > "${BACKUP_DIR}/codemonitor_${TIMESTAMP}.dump"

# Compress and encrypt
gpg --symmetric --cipher-algo AES256 \
    "${BACKUP_DIR}/codemonitor_${TIMESTAMP}.dump"

# Upload to S3
aws s3 cp "${BACKUP_DIR}/codemonitor_${TIMESTAMP}.dump.gpg" \
    "s3://codemonitor-backups/postgres/"

# Cleanup local (keep last 7 days)
find ${BACKUP_DIR} -name "*.dump.gpg" -mtime +7 -delete
```

### TimescaleDB

```bash
#!/bin/bash
# backup-timescale.sh

TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_DIR="/backups/timescale"

# Dump with timescaledb extensions
pg_dump -Fc \
    --no-owner \
    --no-privileges \
    -d metrics \
    > "${BACKUP_DIR}/metrics_${TIMESTAMP}.dump"

# Compress and upload
gzip "${BACKUP_DIR}/metrics_${TIMESTAMP}.dump"
aws s3 cp "${BACKUP_DIR}/metrics_${TIMESTAMP}.dump.gz" \
    "s3://codemonitor-backups/timescale/"
```

---

## Performance Optimization

### Database Optimization

```sql
-- Conexão pooling settings
ALTER SYSTEM SET max_connections = 200;
ALTER SYSTEM SET shared_buffers = '2GB';
ALTER SYSTEM SET effective_cache_size = '6GB';
ALTER SYSTEM SET maintenance_work_mem = '512MB';
ALTER SYSTEM SET work_mem = '32MB';

-- TimescaleDB specific
ALTER SYSTEM SET timescaledb.max_background_workers = 8;
```

### API Optimization

```rust
// Connection pooling
let pool = PgPoolOptions::new()
    .max_connections(50)
    .min_connections(5)
    .acquire_timeout(Duration::from_secs(3))
    .idle_timeout(Duration::from_secs(600))
    .connect(&database_url)
    .await?;

// Response compression
Router::new()
    .route("/api/v1/metrics", get(get_metrics))
    .layer(CompressionLayer::new())
```

---

## Próximos Passos

- [Quick Wins](../06-ACAO/quick-wins.md) - Começar agora
- [Checklist de Lançamento](../06-ACAO/checklist-lancamento.md) - Para o launch
