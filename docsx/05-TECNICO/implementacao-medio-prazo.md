# Implementacao Medio Prazo

## Prioridades Fase 2 (Meses 4-6)

### 1. API REST

**Framework:** Axum

**Estrutura:**
```
server/src/
├── api/
│   ├── mod.rs
│   ├── routes.rs
│   ├── handlers/
│   │   ├── mod.rs
│   │   ├── servers.rs
│   │   ├── metrics.rs
│   │   ├── alerts.rs
│   │   └── auth.rs
│   ├── middleware/
│   │   ├── mod.rs
│   │   ├── auth.rs
│   │   └── rate_limit.rs
│   └── models/
│       ├── mod.rs
│       └── responses.rs
```

**Implementação Base:**
```rust
// server/src/api/routes.rs
use axum::{
    routing::{get, post, put, delete},
    Router,
};

pub fn api_router(state: AppState) -> Router {
    Router::new()
        // Auth
        .route("/api/v1/auth/login", post(handlers::auth::login))
        .route("/api/v1/auth/refresh", post(handlers::auth::refresh))

        // Servers
        .route("/api/v1/servers", get(handlers::servers::list))
        .route("/api/v1/servers/:id", get(handlers::servers::get))
        .route("/api/v1/servers/:id/metrics", get(handlers::servers::metrics))
        .route("/api/v1/servers/:id/history", get(handlers::servers::history))
        .route("/api/v1/servers/:id/processes", get(handlers::servers::processes))
        .route("/api/v1/servers/:id/services", get(handlers::servers::services))

        // Alerts
        .route("/api/v1/alerts", get(handlers::alerts::list))
        .route("/api/v1/alerts/:id", get(handlers::alerts::get))
        .route("/api/v1/alerts/:id/acknowledge", post(handlers::alerts::acknowledge))

        // Alert Rules
        .route("/api/v1/rules", get(handlers::rules::list).post(handlers::rules::create))
        .route("/api/v1/rules/:id", put(handlers::rules::update).delete(handlers::rules::delete))

        // Middleware
        .layer(middleware::AuthLayer::new())
        .layer(middleware::RateLimitLayer::new(100, Duration::from_secs(60)))
        .with_state(state)
}
```

**Handlers Example:**
```rust
// server/src/api/handlers/servers.rs
use axum::{
    extract::{Path, Query, State},
    Json,
};

#[derive(Deserialize)]
pub struct MetricsQuery {
    from: Option<i64>,
    to: Option<i64>,
    resolution: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<ServerSummary>>, ApiError> {
    let servers = state.server_manager.list_for_user(&auth.user_id).await?;
    Ok(Json(servers))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    auth: AuthUser,
) -> Result<Json<ServerDetails>, ApiError> {
    let server = state.server_manager.get(&id, &auth.user_id).await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(server))
}

pub async fn metrics(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<MetricsQuery>,
    auth: AuthUser,
) -> Result<Json<Metrics>, ApiError> {
    let from = query.from.unwrap_or_else(|| Utc::now().timestamp() - 3600);
    let to = query.to.unwrap_or_else(|| Utc::now().timestamp());

    let metrics = state.metrics_service
        .query(&id, from, to, query.resolution.as_deref())
        .await?;

    Ok(Json(metrics))
}
```

**Auth Middleware:**
```rust
// server/src/api/middleware/auth.rs
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let token = token.ok_or(ApiError::Unauthorized)?;

    let claims = state.jwt_service.verify(token)
        .map_err(|_| ApiError::Unauthorized)?;

    request.extensions_mut().insert(AuthUser {
        user_id: claims.sub,
        org_id: claims.org_id,
        permissions: claims.permissions,
    });

    Ok(next.run(request).await)
}
```

---

### 2. Web Dashboard

**Stack:**
- React 18
- TypeScript
- Tailwind CSS
- Recharts
- React Query
- Vite

**Estrutura:**
```
web/
├── src/
│   ├── components/
│   │   ├── layout/
│   │   │   ├── Sidebar.tsx
│   │   │   ├── Header.tsx
│   │   │   └── Layout.tsx
│   │   ├── dashboard/
│   │   │   ├── Overview.tsx
│   │   │   ├── CpuChart.tsx
│   │   │   ├── MemoryChart.tsx
│   │   │   ├── DiskUsage.tsx
│   │   │   └── ProcessList.tsx
│   │   ├── servers/
│   │   │   ├── ServerList.tsx
│   │   │   ├── ServerCard.tsx
│   │   │   └── AddServerModal.tsx
│   │   └── alerts/
│   │       ├── AlertList.tsx
│   │       └── AlertRuleForm.tsx
│   ├── hooks/
│   │   ├── useServers.ts
│   │   ├── useMetrics.ts
│   │   └── useAlerts.ts
│   ├── services/
│   │   ├── api.ts
│   │   └── websocket.ts
│   ├── pages/
│   │   ├── Dashboard.tsx
│   │   ├── Servers.tsx
│   │   ├── Alerts.tsx
│   │   └── Settings.tsx
│   └── App.tsx
├── tailwind.config.js
├── vite.config.ts
└── package.json
```

**Componente Dashboard:**
```tsx
// web/src/pages/Dashboard.tsx
import { useParams } from 'react-router-dom';
import { useMetrics } from '../hooks/useMetrics';
import { CpuChart, MemoryChart, DiskUsage, ProcessList } from '../components/dashboard';

export function Dashboard() {
  const { serverId } = useParams();
  const { data: metrics, isLoading } = useMetrics(serverId);

  if (isLoading) return <LoadingSpinner />;

  return (
    <div className="p-6 space-y-6">
      {/* Header */}
      <div className="flex justify-between items-center">
        <h1 className="text-2xl font-bold">{metrics.hostname}</h1>
        <StatusBadge status={metrics.status} />
      </div>

      {/* Quick Stats */}
      <div className="grid grid-cols-4 gap-4">
        <StatCard
          title="CPU"
          value={`${metrics.cpu.toFixed(1)}%`}
          trend={metrics.cpuTrend}
        />
        <StatCard
          title="Memory"
          value={`${metrics.memoryPercent.toFixed(1)}%`}
          subtitle={formatBytes(metrics.memoryUsed)}
        />
        <StatCard
          title="Disk"
          value={`${metrics.diskPercent.toFixed(1)}%`}
          subtitle={formatBytes(metrics.diskUsed)}
        />
        <StatCard
          title="Network"
          value={formatBytesPerSec(metrics.networkThroughput)}
        />
      </div>

      {/* Charts */}
      <div className="grid grid-cols-2 gap-6">
        <Card title="CPU History (24h)">
          <CpuChart data={metrics.cpuHistory} />
        </Card>
        <Card title="Memory History (24h)">
          <MemoryChart data={metrics.memoryHistory} />
        </Card>
      </div>

      {/* Disk Usage */}
      <Card title="Disk Usage">
        <DiskUsage disks={metrics.disks} />
      </Card>

      {/* Process List */}
      <Card title="Top Processes">
        <ProcessList processes={metrics.topProcesses} />
      </Card>
    </div>
  );
}
```

**WebSocket Hook:**
```tsx
// web/src/hooks/useRealtimeMetrics.ts
import { useEffect, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';

export function useRealtimeMetrics(serverId: string) {
  const queryClient = useQueryClient();

  useEffect(() => {
    const ws = new WebSocket(`${WS_URL}/servers/${serverId}/stream`);

    ws.onmessage = (event) => {
      const metrics = JSON.parse(event.data);

      // Update cache
      queryClient.setQueryData(['metrics', serverId], (old: any) => ({
        ...old,
        ...metrics,
        cpuHistory: [...old.cpuHistory.slice(-59), metrics.cpu],
      }));
    };

    return () => ws.close();
  }, [serverId, queryClient]);
}
```

---

### 3. Docker Monitoring

**Collector:**
```rust
// server/src/collectors/docker.rs
use bollard::Docker;

pub struct DockerCollector {
    docker: Docker,
}

impl DockerCollector {
    pub fn new() -> Result<Self, bollard::errors::Error> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker })
    }
}

#[async_trait]
impl Collector for DockerCollector {
    fn name(&self) -> &str {
        "docker"
    }

    async fn collect(&self) -> Result<Vec<Metric>, CollectorError> {
        let containers = self.docker.list_containers::<String>(None).await?;
        let mut metrics = Vec::new();

        for container in containers {
            let id = container.id.as_ref().unwrap();
            let name = container.names.as_ref()
                .and_then(|n| n.first())
                .map(|n| n.trim_start_matches('/'))
                .unwrap_or("unknown");

            // Get stats
            let stats = self.docker
                .stats(id, Some(StatsOptions { stream: false, ..Default::default() }))
                .next()
                .await
                .ok_or(CollectorError::NoStats)??;

            // CPU
            let cpu_delta = stats.cpu_stats.cpu_usage.total_usage
                - stats.precpu_stats.cpu_usage.total_usage;
            let system_delta = stats.cpu_stats.system_cpu_usage.unwrap_or(0)
                - stats.precpu_stats.system_cpu_usage.unwrap_or(0);
            let cpu_percent = if system_delta > 0 {
                (cpu_delta as f64 / system_delta as f64) * 100.0
            } else {
                0.0
            };

            // Memory
            let memory_usage = stats.memory_stats.usage.unwrap_or(0);
            let memory_limit = stats.memory_stats.limit.unwrap_or(1);
            let memory_percent = (memory_usage as f64 / memory_limit as f64) * 100.0;

            metrics.push(Metric::Container {
                id: id.clone(),
                name: name.to_string(),
                status: container.state.clone().unwrap_or_default(),
                cpu_percent,
                memory_usage,
                memory_limit,
                memory_percent,
                network_rx: stats.networks.as_ref()
                    .map(|n| n.values().map(|v| v.rx_bytes).sum())
                    .unwrap_or(0),
                network_tx: stats.networks.as_ref()
                    .map(|n| n.values().map(|v| v.tx_bytes).sum())
                    .unwrap_or(0),
            });
        }

        Ok(metrics)
    }
}
```

---

### 4. SaaS Infrastructure

**Multi-tenancy:**
```rust
// server/src/multi_tenant/mod.rs

#[derive(Clone)]
pub struct TenantContext {
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub permissions: Vec<Permission>,
}

impl TenantContext {
    pub fn from_claims(claims: &JwtClaims) -> Self {
        Self {
            org_id: claims.org_id,
            user_id: claims.sub,
            permissions: claims.permissions.clone(),
        }
    }
}

// Middleware que injeta tenant context
pub async fn tenant_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let auth_user = request.extensions()
        .get::<AuthUser>()
        .ok_or(ApiError::Unauthorized)?;

    let tenant = TenantContext::from_claims(&auth_user.claims);
    request.extensions_mut().insert(tenant);

    Ok(next.run(request).await)
}

// Query scoped por tenant
impl MetricsRepository {
    pub async fn query_for_tenant(
        &self,
        tenant: &TenantContext,
        query: MetricsQuery,
    ) -> Result<Vec<Metric>> {
        sqlx::query_as!(
            Metric,
            r#"
            SELECT * FROM metrics
            WHERE org_id = $1
              AND server_id = $2
              AND timestamp BETWEEN $3 AND $4
            ORDER BY timestamp DESC
            "#,
            tenant.org_id,
            query.server_id,
            query.from,
            query.to,
        )
        .fetch_all(&self.pool)
        .await
    }
}
```

**Billing (Stripe):**
```rust
// server/src/billing/mod.rs
use stripe::{Client, Customer, Subscription};

pub struct BillingService {
    stripe: Client,
}

impl BillingService {
    pub async fn create_customer(&self, org: &Organization) -> Result<Customer> {
        let customer = Customer::create(
            &self.stripe,
            CreateCustomer {
                email: Some(&org.billing_email),
                name: Some(&org.name),
                metadata: Some(HashMap::from([
                    ("org_id".to_string(), org.id.to_string()),
                ])),
                ..Default::default()
            },
        ).await?;

        Ok(customer)
    }

    pub async fn create_subscription(
        &self,
        customer_id: &str,
        price_id: &str,
        quantity: u64,
    ) -> Result<Subscription> {
        let subscription = Subscription::create(
            &self.stripe,
            CreateSubscription {
                customer: customer_id.into(),
                items: Some(vec![CreateSubscriptionItems {
                    price: Some(price_id.into()),
                    quantity: Some(quantity),
                    ..Default::default()
                }]),
                ..Default::default()
            },
        ).await?;

        Ok(subscription)
    }

    pub async fn update_quantity(
        &self,
        subscription_id: &str,
        item_id: &str,
        quantity: u64,
    ) -> Result<()> {
        SubscriptionItem::update(
            &self.stripe,
            item_id,
            UpdateSubscriptionItem {
                quantity: Some(quantity),
                ..Default::default()
            },
        ).await?;

        Ok(())
    }
}

// Webhook handler
pub async fn stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let signature = headers
        .get("Stripe-Signature")
        .ok_or(ApiError::BadRequest)?
        .to_str()?;

    let event = Webhook::construct_event(
        &body,
        signature,
        &state.config.stripe_webhook_secret,
    )?;

    match event.type_ {
        EventType::CustomerSubscriptionCreated => {
            let subscription: Subscription = event.data.object.try_into()?;
            state.billing.handle_subscription_created(subscription).await?;
        }
        EventType::CustomerSubscriptionUpdated => {
            let subscription: Subscription = event.data.object.try_into()?;
            state.billing.handle_subscription_updated(subscription).await?;
        }
        EventType::InvoicePaymentSucceeded => {
            let invoice: Invoice = event.data.object.try_into()?;
            state.billing.handle_payment_succeeded(invoice).await?;
        }
        EventType::InvoicePaymentFailed => {
            let invoice: Invoice = event.data.object.try_into()?;
            state.billing.handle_payment_failed(invoice).await?;
        }
        _ => {}
    }

    Ok(StatusCode::OK)
}
```

---

## Checklist Fase 2

### Mês 4
- [ ] API REST: Estrutura base
- [ ] API REST: Auth (JWT)
- [ ] API REST: Endpoints servers
- [ ] API REST: Endpoints metrics
- [ ] Web: Setup React + Vite
- [ ] Web: Layout base
- [ ] Docker: Collector implementation

### Mês 5
- [ ] Web: Dashboard completo
- [ ] Web: Real-time updates
- [ ] SaaS: Multi-tenancy
- [ ] SaaS: User management
- [ ] Billing: Stripe integration
- [ ] Webhooks: Slack, Discord

### Mês 6
- [ ] Launch preparation
- [ ] Performance testing
- [ ] Bug fixes
- [ ] Documentation

---

## Próximos Passos

- [Melhorias de Infra](./melhorias-infra.md) - DB, cache, deploy
- [Fase 2 Roadmap](../04-ROADMAP/fase2-expansao.md) - Timeline
