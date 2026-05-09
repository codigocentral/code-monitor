# Aplicação na infraestrutura real

> Como o **code-monitor** se encaixa nos servidores **A6 / A7 / A8**.
> Estado real medido em **2026-05-08 12:41 UTC-3**.

---

## 1. Mapa da infraestrutura

### A6 — `10.10.0.6` — DEV INFRASTRUCTURE

| Item | Valor | Observação |
|---|---|---|
| Hostname | dev-server (provável) | confirmar com `hostname` |
| RAM | 15 Gi (8.3 used, 6.0 GB swap usado!) | swap alto pré-existente |
| Disco | 194 GB (70% usado) | atenção |
| Load | 1.5 / 1.4 / 1.4 | OK |
| Containers | **27** | paraiso (workers/api/frontend dev+prod), gitlab, sonar, scrutin, vip4link-dev, didaticos, codestripe, keycloak-dev, etc. |
| Postgres | cluster `dev` em **:5433** (único) | shared_buffers=4GB, pg_stat_statements ativo |
| Top bancos | paraiso (7.2 GB), codemail (1.3 GB), x3rpt0 (871 MB), gitlab (435 MB), sonar (345 MB), plane (133 MB) + 25 outros |
| Conexões pg | 38 idle (sonar=20, paraiso=3, etc.) | 0 idle in transaction ✅ |

### A7 — `10.10.0.1` — FERRAMENTAS

| Item | Valor | Observação |
|---|---|---|
| RAM | 5.8 Gi (1.5 used, 1.3 GB swap) | folgado |
| Disco | 391 GB (34% usado) | folgado |
| Load | 0.39 | excelente |
| Containers | **13** | tempo, loki, grafana, seq, rabbitmq, uptime-kuma (×2), affine_server, affine_redis, vaultwarden, portainer, matomo, meilisearch |
| GitLab Runner | nativo (não container) | concurrent=2 ✅ |
| Postgres | nenhum local (usa A6) | — |

**Observação importante:** A7 é o **candidato natural pra ser o "agregador central"** se decidirmos por isso futuramente — tem RAM sobrando, load baixo, já hospeda Grafana/Loki/Tempo (stack de observability complementar).

### A8 — `10.10.0.8` — PRODUÇÃO

| Item | Valor | Observação |
|---|---|---|
| RAM | 15 Gi (8.3 used, 807 MB swap) | saudável após cleanup |
| Disco | 391 GB (18% usado) | folgado |
| Load | 0.64 / 0.43 / 0.50 | OK |
| Containers | **41** | bellesintra (×3), vip4link (×3), freescout, mautic + mautic_db, keycloak-prod, codenews, blocklist, agatha, wepper, webui, plane, n8n, sites estáticos, etc. |
| Postgres `main` | **:5432** | configs default (shared_buffers=128MB) |
| Postgres `prod` | **:5433** | shared_buffers=2GB (após nosso tuning), pg_stat_statements em 10 bancos, cache hit 100% |
| MariaDB | container `mautic_db` | mautic schema 4.3 GB, segments rebuild a cada 30min |
| APIs .NET | 7 ativas | BelleSintra (228MB), GeraWebP (449MB), Vip4Link (298MB), site.diogocosta (242MB), TechNewsAggregator (196MB), BlockList (135MB), codigocentra.ips (78MB) |

---

## 2. Mapeamento: o que o code-monitor precisa coletar em cada host

### A6 (cobertura mais complexa)

| Coletor | Item | Frequência | Crítico? |
|---|---|---|---|
| system | CPU/mem/disk/load | 5s | ✅ |
| processes | Top 20 por CPU+MEM | 10s | ✅ |
| docker | Stats de 27 containers | 10s | ✅ |
| postgres | Cluster dev:5433, top 10 bancos por size, cache hit, top 5 queries | 30s | ✅ |
| postgres | Conexões por banco, idle in tx | 60s | ✅ |
| systemd | postgresql@18-dev, gitlab-runner se nativo, sonarqube se nativo | 60s | 🟡 |
| disk_io | rate por device (sda3) | 10s | 🟡 |
| swap | uso, swap-out rate | 30s | ✅ (pré-existente alto) |

### A7 (cobertura simples)

| Coletor | Item | Frequência | Crítico? |
|---|---|---|---|
| system | CPU/mem/disk/load | 5s | ✅ |
| docker | 13 containers (especial: gitlab-runner se for container, RabbitMQ, Loki/Tempo/Grafana) | 10s | ✅ |
| processes | top 10 | 30s | 🟡 |
| systemd | gitlab-runner.service | 60s | ✅ |

### A8 (mais containers, mas mais simples por banco)

| Coletor | Item | Frequência | Crítico? |
|---|---|---|---|
| system | CPU/mem/disk/load | 5s | ✅ |
| docker | 41 containers — atenção em mautic_db, keycloak-prod, freescout, .NET apps | 10s | ✅ |
| processes | top 10 .NET apps por RSS | 30s | ✅ |
| postgres | Clusters main:5432 + prod:5433 (2 instâncias coletadas separadamente) | 30s | ✅ |
| mariadb | mautic_db: processlist, slow queries, processo de segments | 30s | ✅ |
| systemd | postgresql@18-main, postgresql@18-prod | 60s | ✅ |

---

## 3. Topologia de deploy proposta

### Opção A — N-pra-N (MVP, simples)

```
┌──────────────────┐
│  Diogo Workstation│
│  monitor-client  │◄──┬─── gRPC :50051 ──┐
│  (TUI)           │   │                  │
└──────────────────┘   │                  │
                       ▼                  ▼
                  ┌─────────┐        ┌─────────┐        ┌─────────┐
                  │   A6    │        │   A7    │        │   A8    │
                  │ agent   │        │ agent   │        │ agent   │
                  │ :50051  │        │ :50051  │        │ :50051  │
                  └─────────┘        └─────────┘        └─────────┘
```

**Vantagens:** simples, sem ponto único de falha, agent leve.
**Desvantagens:** sem histórico se workstation desligar, sem dashboard pelo celular.

✅ **Recomendado para MVP (Épicos 1-5).**

### Opção B — Aggregator central (pós-MVP)

```
┌──────────────────┐
│  Diogo Workstation│
│  monitor-client  │
└────────┬─────────┘
         │ gRPC ou HTTP
         ▼
   ┌──────────────┐
   │  Aggregator  │  ◄── pode rodar no A7 (folgado)
   │  +  Web UI   │
   │  +  SQLite   │
   │  +  AlertMgr │
   └──────┬───────┘
          │ collect
   ┌──────┴───────┬──────────┐
   ▼              ▼          ▼
┌──────┐    ┌──────┐    ┌──────┐
│ A6   │    │ A7   │    │ A8   │
│ agent│    │ agent│    │ agent│
└──────┘    └──────┘    └──────┘
```

**Vantagens:** histórico, web, alertas mesmo sem cliente.
**Desvantagens:** mais código, mais um ponto de falha.

⏳ **Pós-MVP — Épico 6.**

---

## 4. Configuração proposta por host

### A6 (`/etc/code-monitor/config.toml`)

```toml
update_interval_seconds = 5
log_level = "info"

[server]
address = "10.10.0.6"
port = 50051

[auth]
enable = true
# token gerado na primeira execução

[tls]
enabled = true
cert_path = "/etc/code-monitor/certs/server.crt"
key_path = "/etc/code-monitor/certs/server.key"
ca_path = "/etc/code-monitor/certs/ca.crt"

[health]
port = 8080

[collectors.system]
enabled = true

[collectors.processes]
enabled = true
top_n = 20

[collectors.docker]
enabled = true
socket_path = "/var/run/docker.sock"

[[collectors.postgres]]
name = "dev"
host = "/var/run/postgresql"  # socket Unix (mais seguro)
port = 5433
user = "postgres"
# password vem de env: POSTGRES_DEV_PASSWORD ou peg auth via socket
application_name = "code-monitor-agent"
collect_pg_stat_statements = true
top_queries = 5

[collectors.systemd]
enabled = true
units = ["postgresql@18-dev.service", "docker.service"]
```

### A7

```toml
update_interval_seconds = 5

[server]
address = "10.10.0.1"
port = 50051

[collectors.system]
enabled = true

[collectors.docker]
enabled = true
# atenção: gitlab-runner roda nativo, não container

[collectors.systemd]
enabled = true
units = ["gitlab-runner.service", "docker.service"]

# postgres não — A7 não tem postgres local
```

### A8

```toml
update_interval_seconds = 5

[server]
address = "10.10.0.8"
port = 50051

[collectors.docker]
enabled = true

[[collectors.postgres]]
name = "main"
host = "/var/run/postgresql"
port = 5432
user = "postgres"
application_name = "code-monitor-agent"

[[collectors.postgres]]
name = "prod"
host = "/var/run/postgresql"
port = 5433
user = "postgres"
application_name = "code-monitor-agent"
collect_pg_stat_statements = true
top_queries = 5

[[collectors.mariadb]]
name = "mautic"
host = "127.0.0.1"  # via docker port-mapping ou socket
port = 3306
user = "monitor"   # criar usuário read-only
# password via env

[collectors.systemd]
enabled = true
units = ["postgresql@18-main.service", "postgresql@18-prod.service", "docker.service"]
```

### Cliente (`~/.config/code-monitor/client.toml`)

```toml
update_interval_seconds = 5
auto_reconnect = true

[[servers]]
id = "uuid-a6"
name = "A6 Dev"
address = "10.10.0.6"
port = 50051
access_token = "..."

[[servers]]
id = "uuid-a7"
name = "A7 Ferramentas"
address = "10.10.0.1"
port = 50051
access_token = "..."

[[servers]]
id = "uuid-a8"
name = "A8 Prod"
address = "10.10.0.8"
port = 50051
access_token = "..."

[[notifications.discord]]
name = "alerts-prod"
webhook_url = "..."

[[alerts.rules]]
name = "CPU host > 85% por 5min"
type = "cpu_high"
severity = "warning"
threshold = 85.0
duration_seconds = 300
servers = []  # todos
channels = ["alerts-prod"]

[[alerts.rules]]
name = "Memória > 90%"
type = "memory_high"
severity = "critical"
threshold = 90.0
duration_seconds = 60
channels = ["alerts-prod"]

[[alerts.rules]]
name = "Disco > 85%"
type = "disk_high"
severity = "warning"
threshold = 85.0
duration_seconds = 300
channels = ["alerts-prod"]

[[alerts.rules]]
name = "Container memory leak (BelleSintra > 1GB)"
type = "container_memory_high"
severity = "critical"
container_name_pattern = "bellesintra*"
threshold_bytes = 1073741824
duration_seconds = 600
channels = ["alerts-prod"]

[[alerts.rules]]
name = "mautic_db CPU > 95% por 10min"
type = "container_cpu_high"
severity = "warning"
container_name = "mautic_db"
threshold = 95.0
duration_seconds = 600
channels = ["alerts-prod"]

[[alerts.rules]]
name = "Postgres cache hit ratio < 70%"
type = "postgres_cache_low"
severity = "warning"
threshold = 70.0
duration_seconds = 600
channels = ["alerts-prod"]
```

---

## 5. Decisões de segurança

### Autenticação Postgres pelo agent

**Recomendação:** Conexão via **socket Unix com peer auth**.

- O agent roda como user `code-monitor` (criar)
- Em pg_hba.conf: `local all code-monitor peer`
- Em postgres: `CREATE USER "code-monitor" WITH LOGIN; GRANT pg_monitor TO "code-monitor";`
- `pg_monitor` é role built-in do postgres 10+ que dá leitura em `pg_stat_*` e funções de monitoramento

**Vantagens:**
- Sem senha em arquivo
- Sem porta TCP exposta para o agent
- Auditável (peer auth = só user `code-monitor` consegue)

### Autenticação MariaDB

**Recomendação:** usuário read-only com privs mínimos.

```sql
CREATE USER 'monitor'@'localhost' IDENTIFIED BY '<senha>';
GRANT PROCESS, REPLICATION CLIENT, SELECT ON information_schema.* TO 'monitor'@'localhost';
GRANT SELECT ON mysql.user TO 'monitor'@'localhost';
```

Senha em arquivo `/etc/code-monitor/secrets.env` com `chmod 600`.

### Acesso ao Docker socket

Agent precisa de read no `/var/run/docker.sock`. Adicionar user ao group `docker`:

```bash
usermod -aG docker code-monitor
```

⚠️ **Atenção**: dar acesso ao docker socket = dar acesso root efetivo. Apenas no servidor que vai rodar o agent.

### TLS entre agent e cliente

- CA self-signed em `/etc/code-monitor/certs/ca.crt`
- Server cert assinado pela CA, com SAN `DNS:host.local, IP:10.10.0.X`
- Cliente verifica CA
- Mutual TLS (mTLS) é nice-to-have, não bloqueante pro MVP

---

## 6. Cuidados específicos

### A6 com swap alto

A6 hoje tem 6 GB de swap em uso (gitlab, sonarqube, redis paginados). O agent precisa ser **muito leve** pra não pressionar mais. Meta: <100MB RSS. Configurar:

```toml
[performance]
max_history_in_memory = 60   # 5min de samples
gc_interval_seconds = 300
```

### A8 com 41 containers

Coleta de docker stats em 41 containers via `bollard` deve ser eficiente. A API do docker é nativa async — não vai bloquear thread. Mas atenção: bench em ambiente real, **não fazer polling síncrono em loop**.

### Postgres em A6 com swap

Quando a coleta rodar `pg_stat_statements` (query pesada se tiver milhões de queries), pode disparar paginação. Mitigação:
- `application_name='code-monitor'` na conexão (pra distinguir nos logs)
- `statement_timeout=30s` na sessão do agent
- Coletar a cada 60s (não 5s)

### MariaDB conexões múltiplas

mautic_db tem cap de 1.5 vCPU. Se o agent abrir conexões a cada 5s sem reuso, pode somar carga. Mitigação:
- Pool de 1-2 conexões persistentes
- Reabrir só em erro

---

## 7. Próxima ação

1. Ler `03-backlog.md` (issues acionáveis)
2. Decidir: começar pelo **EP1 (Docker)** ou **EP2 (Postgres)**?
   - EP1 é mais visual (vê containers no TUI no dia 1)
   - EP2 cobre o caso de uso mais doloroso atual (mautic_db, paraiso slow queries)
3. Criar branch `feat/docker-collector` ou `feat/postgres-collector`
