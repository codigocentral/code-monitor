# Fase 2: Expansao (Meses 4-6)

## Objetivo

> Expandir para mercado web, lançar publicamente, validar modelo de negócio com primeiros clientes pagantes.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    FASE 2: EXPANSÃO                                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   ENTRADA                              SAÍDA                               │
│   ────────                             ──────                               │
│                                                                             │
│   ✅ Produto estável                   ✅ Web Dashboard                     │
│   ✅ 100 beta users                    ✅ SaaS funcionando                  │
│   ✅ Feedback incorporado              ✅ 50+ clientes pagantes             │
│   ✅ Docs completa                     ✅ $1k+ MRR                          │
│   ✅ TLS + Alertas                     ✅ Product-market fit sinal          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Mes 4: Web Dashboard e API

### Semana 1-2: API REST

**Objetivo:** Interface programática para integrações

**Endpoints:**
```
GET  /api/v1/servers                 # Lista servidores
GET  /api/v1/servers/:id             # Detalhes servidor
GET  /api/v1/servers/:id/metrics     # Métricas atuais
GET  /api/v1/servers/:id/history     # Histórico
GET  /api/v1/servers/:id/processes   # Lista processos
GET  /api/v1/servers/:id/services    # Lista serviços
GET  /api/v1/alerts                  # Alertas ativos
POST /api/v1/alerts/:id/acknowledge  # Reconhecer alerta
```

**Stack:**
- Framework: Axum
- Auth: API Keys + JWT
- Docs: OpenAPI/Swagger

**Tasks:**
```
[ ] Setup Axum server
[ ] Endpoints de leitura
[ ] Autenticação API key
[ ] Rate limiting
[ ] OpenAPI spec
[ ] SDK gerado (opcional)
```

### Semana 2-4: Web Dashboard

**Objetivo:** Interface web para quem não quer TUI

**Stack:**
```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    WEB DASHBOARD STACK                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Frontend:                                                                 │
│   ├── React 18 + TypeScript                                                │
│   ├── Tailwind CSS                                                         │
│   ├── Recharts (gráficos)                                                  │
│   ├── React Query (data fetching)                                          │
│   └── Vite (build)                                                         │
│                                                                             │
│   Backend:                                                                  │
│   ├── API REST (Axum)                                                      │
│   ├── WebSocket (real-time updates)                                        │
│   └── Embedded ou separado                                                 │
│                                                                             │
│   Deploy:                                                                   │
│   ├── Static files servidos pelo server                                    │
│   └── ou CDN (Cloudflare) + API                                            │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Layout Web:**
```
┌─────────────────────────────────────────────────────────────────────────────┐
│  Code Monitor                                    [Search]  [?] [Settings]   │
├────────────────┬────────────────────────────────────────────────────────────┤
│                │                                                            │
│  SERVERS       │   Dashboard: prod-web-01                                  │
│  ──────────    │   ─────────────────────────                               │
│                │                                                            │
│  🟢 prod-web   │   ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐ │
│  🟢 prod-db    │   │   CPU    │  │  Memory  │  │   Disk   │  │ Network  │ │
│  🟡 staging    │   │   45%    │  │   78%    │  │   58%    │  │  1.2Gb/s │ │
│                │   └──────────┘  └──────────┘  └──────────┘  └──────────┘ │
│  + Add Server  │                                                            │
│                │   CPU History (24h)                                       │
│                │   ┌────────────────────────────────────────────────────┐  │
│                │   │    ▁▂▃▄▅▆▇█▇▆▅▄▃▂▁▂▃▄▅▆▇█▇▆▅▄▃▂▁▂▃▄▅             │  │
│                │   └────────────────────────────────────────────────────┘  │
│                │                                                            │
│                │   Top Processes                                           │
│                │   ┌────────────────────────────────────────────────────┐  │
│                │   │ nginx      12%   1.2GB                             │  │
│                │   │ postgres   34%   4.5GB                             │  │
│                │   │ redis       2%   0.5GB                             │  │
│                │   └────────────────────────────────────────────────────┘  │
│                │                                                            │
└────────────────┴────────────────────────────────────────────────────────────┘
```

**Tasks:**
```
[ ] Setup React + Vite
[ ] Layout responsivo
[ ] Server list component
[ ] Dashboard overview
[ ] Charts (CPU, Memory, Disk)
[ ] Process table
[ ] WebSocket real-time
[ ] Dark/Light mode
[ ] Mobile responsive
```

### Docker Monitoring

**Objetivo:** Métricas de containers Docker

**Métricas:**
- Container status (running, stopped)
- CPU/Memory por container
- Network I/O
- Disk I/O
- Logs (básico)

**Tasks:**
```
[ ] Docker socket connection
[ ] Container list
[ ] Container stats
[ ] Integração com dashboard
[ ] Alertas por container
```

---

## Mes 5: SaaS e Monetizacao

### Semana 1-2: Plataforma SaaS

**Arquitetura:**
```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    ARQUITETURA SAAS                                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Usuários                                                                  │
│      │                                                                      │
│      ▼                                                                      │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                     CLOUDFLARE                                      │   │
│   │                   (WAF, DDoS, CDN)                                  │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│      │                                                                      │
│      ▼                                                                      │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                     API GATEWAY                                     │   │
│   │                   (Rate limit, Auth)                                │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│      │                    │                    │                            │
│      ▼                    ▼                    ▼                            │
│   ┌───────────┐     ┌───────────┐      ┌───────────┐                       │
│   │ Web App   │     │  API      │      │ Collector │                       │
│   │ (React)   │     │  (Axum)   │      │ (gRPC)    │                       │
│   └───────────┘     └───────────┘      └───────────┘                       │
│                          │                    │                            │
│                          ▼                    ▼                            │
│                    ┌───────────────────────────────┐                       │
│                    │         PostgreSQL            │                       │
│                    │   (users, billing, config)    │                       │
│                    └───────────────────────────────┘                       │
│                                   │                                        │
│                    ┌──────────────┴──────────────┐                         │
│                    ▼                             ▼                         │
│               ┌─────────┐                  ┌─────────┐                     │
│               │TimescaleDB               │ Redis   │                     │
│               │ (metrics)                │ (cache) │                     │
│               └─────────┘                  └─────────┘                     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Tasks:**
```
[ ] Multi-tenancy básica
[ ] User authentication (auth0 ou próprio)
[ ] Billing integration (Stripe)
[ ] Subscription management
[ ] Usage tracking
[ ] Onboarding flow
```

### Semana 2-3: Webhooks

**Objetivo:** Integração com ferramentas externas

**Integrações v1:**
- Slack
- Discord
- Microsoft Teams
- Email (SMTP)
- Generic Webhook

**Payload:**
```json
{
  "event": "alert.triggered",
  "alert": {
    "id": "uuid",
    "name": "CPU Critical",
    "type": "cpu_high",
    "severity": "critical",
    "server": "prod-web-01",
    "value": 95.2,
    "threshold": 90,
    "triggered_at": "2026-01-15T10:30:00Z"
  }
}
```

**Tasks:**
```
[ ] Webhook sender
[ ] Slack integration
[ ] Discord integration
[ ] Email sender
[ ] Retry logic
[ ] Delivery logs
```

### Semana 3-4: Billing e Checkout

**Stripe Integration:**
```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    BILLING FLOW                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   User signup (free)                                                        │
│         │                                                                   │
│         ▼                                                                   │
│   Uses Community tier                                                       │
│         │                                                                   │
│         ▼                                                                   │
│   Hits limit (11+ nodes)                                                    │
│         │                                                                   │
│         ▼                                                                   │
│   Upgrade prompt                                                            │
│         │                                                                   │
│         ▼                                                                   │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                     STRIPE CHECKOUT                                 │   │
│   │                                                                     │   │
│   │   Pro Plan - $2/node/month                                          │   │
│   │   ┌─────────────────────────────────────────────────────────────┐   │   │
│   │   │ Nodes: [____15____]  ×  $2  =  $30/month                    │   │   │
│   │   └─────────────────────────────────────────────────────────────┘   │   │
│   │                                                                     │   │
│   │   [ ] Annual billing (save 17%)                                     │   │
│   │                                                                     │   │
│   │   [Credit Card Input]                                               │   │
│   │                                                                     │   │
│   │   [Subscribe Now]                                                   │   │
│   │                                                                     │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│         │                                                                   │
│         ▼                                                                   │
│   Webhook: subscription.created                                             │
│         │                                                                   │
│         ▼                                                                   │
│   Unlock Pro features                                                       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Tasks:**
```
[ ] Stripe account setup
[ ] Product/Price creation
[ ] Checkout session
[ ] Webhook handler
[ ] Subscription management
[ ] Invoice portal
[ ] Usage-based billing (nodes)
```

---

## Mes 6: Launch e Estabilizacao

### Semana 1: Pre-Launch

**Checklist:**
```
[ ] Landing page final
[ ] Pricing page
[ ] Documentation completa
[ ] Blog post de lançamento
[ ] Press kit
[ ] Social media assets
[ ] Email sequences
[ ] Demo video
[ ] Launch plan timeline
```

### Semana 2: Launch

**Launch Day Timeline:**
```
06:00 - Blog post publicado
07:00 - Tweet thread (10 tweets)
08:00 - Post no HN (Show HN)
09:00 - Post no r/programming
10:00 - Post no r/devops
11:00 - LinkedIn announcement
12:00 - Product Hunt submission
14:00 - Responder comentários
16:00 - Email para newsletter
18:00 - Discord announcement
20:00 - Review do dia
```

### Semana 3-4: Estabilização

**Foco:**
- Bug fixes críticos
- Performance tuning
- Feedback collection
- Quick wins implementation
- Customer success

**Métricas diárias:**
```
[ ] Signups
[ ] Active users
[ ] Conversions
[ ] Churn signals
[ ] NPS/feedback
[ ] Bug reports
```

---

## Entregaveis Fase 2

| Entregável | Mês | Critério de Aceite |
|------------|-----|-------------------|
| API REST | M4 | Endpoints funcionando |
| Web Dashboard | M4-5 | MVP usável |
| Docker Support | M4 | Container metrics |
| SaaS Platform | M5 | Multi-tenant |
| Webhooks | M5 | Slack/Discord |
| Billing | M5 | Stripe checkout |
| Public Launch | M6 | HN, PH, Reddit |
| 50 clientes | M6 | Pagando |

---

## Metricas de Sucesso

| Métrica | Target | Como Medir |
|---------|--------|------------|
| Website visitors | 50k | Analytics |
| Signups | 500 | Database |
| Trial → Paid | 10% | Stripe |
| Clientes pagantes | 50 | Stripe |
| MRR | $1.000 | Stripe |
| NPS | > 40 | Survey |
| Churn mensal | < 5% | Stripe |

---

## Próximos Passos

- [Fase 3: Escala](./fase3-escala.md) - Meses 7-9
- [Go-to-Market](../03-ESTRATEGIA/go-to-market.md) - Plano de lançamento
