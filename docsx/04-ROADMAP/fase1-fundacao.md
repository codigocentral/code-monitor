# Fase 1: Fundacao (Meses 1-3)

## Objetivo

> Transformar o MVP funcional em produto production-ready com segurança, persistência e qualidade.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    FASE 1: FUNDAÇÃO                                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   ANTES (MVP)                         DEPOIS (Production Ready)            │
│   ─────────────                       ──────────────────────────           │
│                                                                             │
│   ❌ Sem TLS                          ✅ TLS obrigatório                    │
│   ❌ Sem histórico                    ✅ SQLite 7 dias                      │
│   ❌ Sem alertas                      ✅ 5 tipos de alerta                  │
│   ❌ Deploy manual                    ✅ Docker + APT                       │
│   ❌ Sem CI/CD                        ✅ Pipeline completo                  │
│   ❌ Docs mínimo                      ✅ Docs exemplar                      │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Mes 1: Seguranca e Infraestrutura

### Semana 1-2: TLS/SSL

**Objetivo:** Comunicação segura entre client e server

**Tasks:**
```
[ ] Gerar certificados self-signed para dev
[ ] Implementar TLS no gRPC server (rustls)
[ ] Implementar TLS no gRPC client
[ ] Configuração para certs customizados
[ ] Documentar setup de certificados
[ ] Testes de conexão TLS
```

**Código Exemplo:**
```rust
// server/src/tls.rs
use tonic::transport::{Certificate, Identity, ServerTlsConfig};

pub fn configure_tls(
    cert_path: &Path,
    key_path: &Path,
) -> Result<ServerTlsConfig> {
    let cert = std::fs::read_to_string(cert_path)?;
    let key = std::fs::read_to_string(key_path)?;

    let identity = Identity::from_pem(cert, key);
    Ok(ServerTlsConfig::new().identity(identity))
}
```

**Config:**
```toml
# config.toml
[tls]
enabled = true
cert_path = "/etc/code-monitor/server.crt"
key_path = "/etc/code-monitor/server.key"
```

### Semana 2-3: Health Checks

**Objetivo:** Endpoints para monitorar o próprio monitor

**Tasks:**
```
[ ] HTTP server simples (hyper)
[ ] Endpoint /health (200 OK)
[ ] Endpoint /ready (checks internos)
[ ] Endpoint /metrics (Prometheus format)
[ ] Documentar endpoints
```

**Endpoints:**
```
GET /health
    → 200 OK {"status": "healthy"}

GET /ready
    → 200 OK {"status": "ready", "checks": {...}}
    → 503 Service Unavailable

GET /metrics
    → 200 OK (Prometheus text format)
```

### Semana 3-4: CI/CD Pipeline

**Objetivo:** Qualidade automática em cada PR

**GitHub Actions:**
```yaml
# .github/workflows/ci.yml
name: CI

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
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
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release
      - uses: actions/upload-artifact@v4
        with:
          name: binaries-${{ matrix.os }}
          path: target/release/monitor-*
```

---

## Mes 2: Persistencia e Alertas

### Semana 1-2: Sistema de Histórico (SQLite)

**Objetivo:** Persistir métricas para análise temporal

**Schema:**
```sql
-- Métricas de sistema
CREATE TABLE system_metrics (
    id INTEGER PRIMARY KEY,
    server_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    cpu_usage REAL,
    memory_used INTEGER,
    memory_total INTEGER,
    disk_used INTEGER,
    disk_total INTEGER
);

CREATE INDEX idx_metrics_server_time
    ON system_metrics(server_id, timestamp);

-- Configuração de retenção
-- Purge automático > 7 dias (Pro) ou 24h (Community)
```

**Tasks:**
```
[ ] Setup SQLite com rusqlite
[ ] Schema de métricas
[ ] Writer assíncrono (batch inserts)
[ ] Reader com queries otimizadas
[ ] Purge automático por tier
[ ] API para histórico (gRPC)
[ ] Sparklines com dados históricos
```

### Semana 2-3: Sistema de Alertas

**Objetivo:** Notificações quando thresholds são atingidos

**Tipos de Alertas v1:**

| Tipo | Descrição | Exemplo |
|------|-----------|---------|
| CPU High | CPU acima de X% por Y minutos | > 90% por 5 min |
| Memory High | Memória acima de X% | > 85% |
| Disk High | Disco acima de X% | > 90% |
| Process Down | Processo não encontrado | nginx não rodando |
| Server Unreachable | Conexão perdida | Offline > 1 min |

**Config:**
```toml
# client-config.toml
[[alerts]]
name = "CPU Critical"
type = "cpu_high"
threshold = 90
duration_seconds = 300
notify = ["webhook", "email"]

[[alerts]]
name = "Disk Warning"
type = "disk_high"
threshold = 85
notify = ["webhook"]
```

**Tasks:**
```
[ ] Definir struct Alert
[ ] Alert evaluator (check conditions)
[ ] Alert manager (dedupe, silence)
[ ] Webhook notifier
[ ] Email notifier (SMTP)
[ ] TUI: Alert indicator
[ ] Histórico de alertas
```

### Semana 4: Docker Support

**Docker Compose:**
```yaml
# docker-compose.yml
version: '3.8'

services:
  monitor-server:
    image: codemonitor/server:latest
    ports:
      - "50051:50051"
      - "8080:8080"
    volumes:
      - ./config.toml:/etc/code-monitor/config.toml
      - ./certs:/etc/code-monitor/certs
      - monitor-data:/var/lib/code-monitor
    environment:
      - LOG_LEVEL=info
    restart: unless-stopped

volumes:
  monitor-data:
```

**Tasks:**
```
[ ] Dockerfile otimizado (multi-stage)
[ ] Docker Compose completo
[ ] Health check no container
[ ] Volume para persistência
[ ] Docs de deployment
```

---

## Mes 3: Qualidade e Beta

### Semana 1-2: Documentação Completa

**Estrutura:**
```
docs/
├── getting-started/
│   ├── quick-start.md
│   ├── installation.md
│   └── first-dashboard.md
├── configuration/
│   ├── server-config.md
│   ├── client-config.md
│   ├── tls-setup.md
│   └── alerts.md
├── guides/
│   ├── multi-server.md
│   ├── docker-deployment.md
│   └── troubleshooting.md
├── reference/
│   ├── cli-commands.md
│   ├── keyboard-shortcuts.md
│   └── config-options.md
└── api/
    └── grpc-reference.md
```

**Tasks:**
```
[ ] Getting started guide
[ ] Configuration reference
[ ] Deployment guides
[ ] API documentation
[ ] Troubleshooting guide
[ ] Video: 5-minute setup
```

### Semana 2-3: Beta Testing

**Processo:**
```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    BETA TESTING PROCESS                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   1. Recrutar 100 beta testers                                             │
│      └── Reddit, Twitter, Discord                                          │
│                                                                             │
│   2. Onboarding                                                            │
│      └── Email com instruções, Discord invite                              │
│                                                                             │
│   3. Feedback collection                                                    │
│      ├── GitHub Issues (bugs)                                              │
│      ├── Discord (discussões)                                              │
│      └── Survey (NPS, features)                                            │
│                                                                             │
│   4. Iteration                                                             │
│      └── Weekly releases com fixes                                         │
│                                                                             │
│   5. Graduation                                                            │
│      └── Beta → Public launch ready                                        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Tasks:**
```
[ ] Beta signup form
[ ] Discord server setup
[ ] Onboarding email sequence
[ ] Feedback form
[ ] Weekly office hours
[ ] Bug triage process
```

### Semana 4: APT Repository

**Objetivo:** `apt install code-monitor`

**Tasks:**
```
[ ] Debian package (.deb)
[ ] GPG signing
[ ] APT repository setup
[ ] Installation docs
[ ] Systemd service file
```

**Instalação:**
```bash
# Adicionar repositório
curl -fsSL https://apt.codemonitor.io/gpg.key | sudo gpg --dearmor -o /etc/apt/keyrings/codemonitor.gpg
echo "deb [signed-by=/etc/apt/keyrings/codemonitor.gpg] https://apt.codemonitor.io stable main" | sudo tee /etc/apt/sources.list.d/codemonitor.list

# Instalar
sudo apt update
sudo apt install code-monitor

# Iniciar
sudo systemctl enable --now code-monitor
```

---

## Entregaveis Fase 1

| Entregável | Mês | Critério de Aceite |
|------------|-----|-------------------|
| TLS/SSL | M1 | Conexão encriptada funcionando |
| Health Checks | M1 | /health, /ready, /metrics |
| CI/CD | M1 | PRs testados automaticamente |
| SQLite Histórico | M2 | 7 dias de métricas |
| Alertas (5 tipos) | M2 | Notificação funcionando |
| Docker Compose | M2 | Deploy em 1 comando |
| Docs Completa | M3 | Getting started funcional |
| Beta 100 users | M3 | Feedback coletado |
| APT Repository | M3 | apt install funcionando |

---

## Metricas de Sucesso

| Métrica | Target | Como Medir |
|---------|--------|------------|
| GitHub Stars | 500 | GitHub |
| Beta testers | 100 | Signup list |
| NPS | > 40 | Survey |
| Bugs críticos | 0 | GitHub Issues |
| Test coverage | 70% | cargo tarpaulin |
| Docs coverage | 100% core | Checklist |

---

## Próximos Passos

- [Fase 2: Expansão](./fase2-expansao.md) - Meses 4-6
- [Quick Wins](../06-ACAO/quick-wins.md) - Começar hoje
