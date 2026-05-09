# Gap Analysis — Code Monitor

> Análise técnica linha-a-linha do que está implementado, parcial e faltando.
> **Última revisão:** 2026-05-08 (após leitura completa do código)

---

## 1. Camada de coleta (`server/src/monitor.rs`)

### O que existe

| Métrica | Função | Linhas | Estado |
|---|---|---|---|
| Hostname/OS/kernel | `get_system_info()` | 136-194 | ✅ |
| CPU global | `sys.global_cpu_info().cpu_usage()` | 171 | ✅ funciona, mas **não tem por-core** |
| Memória | `sys.total_memory() / used_memory()` | 142-144 | ✅ |
| Disco por mount | `sys.disks().iter()` | 147-169 | ✅ |
| Processos | `get_processes()` | 196-247 | ✅ — sort por CPU desc, filter por nome |
| "Serviços" | `get_services()` | 249-319 | ⚠️ **heurística**: filtro por nome (`*service`, `*svc`, `*d`, `*server`, etc.) e `run_time>60s`. **Não consulta systemd**. |
| Rede | `get_network_info()` | 321-360 | ✅ — IP via `ip addr show` parse manual (linha 26) |

### Lacunas críticas

| Lacuna | Como resolver |
|---|---|
| **Sem Docker** | Adicionar `bollard` crate, ler `/var/run/docker.sock`, expor `ContainerInfo { id, name, image, status, cpu_pct, mem_used, mem_limit, restart_count, healthy, started_at, networks }` |
| **Sem Postgres** | `tokio-postgres` async, query `pg_stat_database`, `pg_stat_activity`, `pg_database_size`, `pg_stat_statements`. Suportar **múltiplos clusters por host** (A8 tem 5432 + 5433) |
| **Sem MariaDB** | `mysql_async`, `SHOW PROCESSLIST`, `SHOW ENGINE INNODB STATUS`, `information_schema.tables` (tamanho) |
| **Sem systemd nativo** | dbus via `zbus` crate, ou parsing `systemctl list-units --state=running --no-pager` |
| **Sem GPU** (não-prioritário) | `nvml-wrapper` se houver NVIDIA, futuro |
| **CPU per-core** | `sys.cpus().iter()` já existe em sysinfo, só não exposto |
| **Network rate** (KB/s) | Diff entre samples — hoje só expõe total acumulado |
| **Disk I/O rate** | sysinfo expõe `disk.read_bytes()`/`written_bytes()`, não usado |
| **Top N por mem** | Só tem por CPU. Faltar `sort_by` alternativo |

### Refatoração sugerida

```
server/src/
├── collectors/
│   ├── mod.rs           # trait Collector + Registry
│   ├── system.rs        # CPU, mem, disk, network (atual monitor.rs)
│   ├── processes.rs     # com sort configurável
│   ├── services.rs      # systemd via zbus
│   ├── docker.rs        # NOVO
│   ├── postgres.rs      # NOVO (multi-cluster)
│   └── mariadb.rs       # NOVO
└── monitor.rs           # orchestrator
```

Trait sugerida:
```rust
#[async_trait]
pub trait Collector: Send + Sync {
    fn name(&self) -> &'static str;
    fn is_enabled(&self, config: &Config) -> bool;
    async fn collect(&self) -> Result<CollectorOutput>;
    fn interval(&self) -> Duration { Duration::from_secs(5) }
}

pub enum CollectorOutput {
    System(SystemInfo),
    Containers(Vec<ContainerInfo>),
    Postgres(Vec<PostgresInfo>),
    Mariadb(MariadbInfo),
    SystemdUnits(Vec<UnitInfo>),
}
```

---

## 2. Protocolo (`shared/src/proto/monitoring.proto`)

### O que existe

```proto
service MonitorService {
  rpc GetSystemInfo, GetProcesses, GetServices, GetNetworkInfo
  rpc StreamSystemUpdates (stream)
  rpc Authenticate
}
```

### Lacunas

- **Falta `GetContainers`** — adicionar mensagem `ContainerInfo` e RPC
- **Falta `GetDatabaseStats`** — postgres + mariadb
- **Falta `GetSystemdUnits`** — substituir/complementar `GetServices`
- **`SystemUpdate` (streaming)** só comporta tipos antigos via `oneof`. Adicionar variantes novas.
- **Versionamento**: nenhum campo `api_version`. Quando rolar mudança, vai quebrar clientes velhos.

### Sugestão

```proto
message ContainerInfo {
  string id = 1;
  string name = 2;
  string image = 3;
  string status = 4;
  string health = 5;          // "healthy", "unhealthy", "starting", "none"
  double cpu_percent = 6;
  uint64 memory_bytes = 7;
  uint64 memory_limit_bytes = 8;
  uint32 restart_count = 9;
  google.protobuf.Timestamp started_at = 10;
  repeated string networks = 11;
}

message PostgresInfo {
  string cluster_name = 1;     // ex: "dev-5433", "prod-5433"
  string version = 2;
  uint32 max_connections = 3;
  uint32 active_connections = 4;
  uint32 idle_connections = 5;
  uint32 idle_in_tx = 6;
  repeated DatabaseInfo databases = 7;
  repeated TopQuery top_queries = 8;
}

message DatabaseInfo {
  string name = 1;
  uint64 size_bytes = 2;
  uint64 blks_hit = 3;
  uint64 blks_read = 4;
  double cache_hit_pct = 5;
  uint32 num_backends = 6;
}
```

---

## 3. Servidor (`server/src/`)

### `main.rs`

✅ Bom suporte a CLI subcommand (`show-token`, `new-token`, `list-clients`, etc.)
✅ Banner ASCII com infos de conexão
⚠️ **Não suporta `--tls-cert/--tls-key`** — TLS está só no doc, não no código
⚠️ **Não dá pra listar collectors** — quando expandirmos, precisa de comando `list-collectors`

### `service.rs`

✅ `validate_request()` valida access token via metadata gRPC
✅ Auth opcional (`enable_authentication = false` libera tudo)
⚠️ `Authenticate` RPC retorna token "fake" (`token_<unix_ts>`) — **nunca usado**, dead code candidato a remoção
⚠️ Quando `enable_authentication=false`, **qualquer um na rede acessa**. Sem rate limiting.
❌ **Sem TLS** no `Server::builder()`

### `health.rs`

✅ `/health`, `/ready`, `/metrics` (Prometheus formato)
⚠️ `metrics_check` só expõe `code_monitor_uptime_seconds` e `build_info` — **não exporta métricas reais coletadas**
⚠️ `readiness_check` sempre retorna `all_ready = true` mesmo se sysinfo falhar

### `auth.rs`

⚠️ Ed25519 implementado mas **não plugado** no fluxo. `pub key + sign + verify` só serve a `cargo test`.

---

## 4. Cliente (`client/src/`)

### `dashboard.rs` (113 KB!)

Maior arquivo do projeto. Contém:

- 4 tabs: Overview, Services, Processes, Network
- Sparklines pra CPU/mem (60 samples)
- Lista lateral de servidores com status visual
- Wizards interativos: add server, edit token, settings
- Filtros (`/`), ordenação, atalhos vim-style

✅ UX rica
⚠️ **Sem aba Docker** (precisa adicionar)
⚠️ **Sem aba Database** (postgres/mariadb)
⚠️ **Histórico SQLite (storage.rs)** existe mas **não vi uso no dashboard** — gráficos são em memória apenas (sparklines)
⚠️ Tamanho de 113 KB sugere refatoração futura — quebrar por tab em arquivos separados

### `client.rs`

✅ gRPC client com auto metadata (`x-access-token`)
⚠️ `connect()` faz `Channel::from_shared(addr).connect()` — **plaintext sempre**
⚠️ `auto_reconnect` é bool no struct mas implementação real depende do dashboard

### `storage.rs`

✅ SQLite com schema simples: `id, server_id, timestamp, cpu_usage, memory_used, memory_total`
⚠️ Schema **não comporta novas métricas** (containers, databases) — precisa migration
⚠️ Sem índice composto se pesquisar por (`server_id`, `timestamp DESC, metric_type`)
⚠️ `purge_old()` só roda quando chamado — sem job cron

### `auth.rs`

Mesma situação do servidor: Ed25519 existe, não usado.

---

## 5. Alertas (`shared/src/alerts.rs` + `shared/src/notifications.rs`)

### `alerts.rs`

✅ Implementação **completa e bem feita**:
- `AlertRule { name, type, severity, threshold, duration_seconds, channels }`
- `AlertState` com sliding window de samples
- Cooldown via `can_trigger_again(Duration::minutes(5))`
- Acknowledge / resolve
- Histórico até 1000 alertas

✅ Tipos: `CpuHigh`, `MemoryHigh`, `DiskHigh`, `ServerDown`, `ProcessDown`
⚠️ **Falta**: `ContainerDown`, `DatabaseSlowQuery`, `DiskSpaceFull`

### `notifications.rs`

✅ `WebhookChannel`, `SlackChannel`, `DiscordChannel` (vi os 2 primeiros, falta confirmar Discord/Email)
✅ Trait `NotificationChannel` async

### Lacunas críticas

❌ **AlertManager não está conectado ao client/dashboard** — onde rolaria `process_metrics()` no loop principal?
❌ **Não tem load/save de regras** em config — `AlertManager::new()` cria vazio sempre
❌ **Não tem CLI pra gerenciar regras** (`monitor-client alert add ...`)

---

## 6. Configuração

### Servidor (`config.toml`)

```toml
update_interval_seconds = 5
max_clients = 100
enable_authentication = true
log_level = "info"
```

⚠️ **Falta**:
- `[tls]` (cert_path, key_path) — está no README mas não suportado
- `[collectors.docker]` (enabled, socket_path)
- `[[collectors.postgres]]` (host, port, user, password, dbname, application_name)
- `[[collectors.mariadb]]` (idem)
- `[storage]` (sqlite_path, retention_days)

### Cliente (`client-config.toml`)

✅ Suporta múltiplos servers via `[[servers]]`
⚠️ **Falta**:
- `[notifications.discord]` (webhook_url) — pra alertas
- `[[alerts.rules]]` (regras configuradas)

---

## 7. Build, deploy e CI

✅ `Dockerfile.server` e `Dockerfile.client`
✅ `docker-compose.yml` básico
✅ `Makefile` com targets (`build`, `test`, `lint`, `docker-up`, `certs`)
✅ `generate-certs.sh` (cria CA + server cert + client cert)

❌ **Sem GitHub Actions CI ativo** — README menciona mas `.github/workflows/` é vazio (verificado: existe `.github/` mas não vi conteúdo no listing inicial)
❌ **Sem release automation** — sem builds de Linux/Windows/Mac em release tags
❌ **Sem APT repository real** — `doc/APT_REPOSITORY_GUIDE.md` é guia teórico

---

## 8. Resumo: matriz de prontidão

| Componente | Maturidade | Próximo passo |
|---|---|---|
| Coleta de métricas básicas (host) | 🟢 80% | Adicionar disk I/O rate, network rate |
| Coleta de containers Docker | 🔴 0% | EP1 do backlog |
| Coleta de databases | 🔴 0% | EP2 do backlog |
| Coleta de systemd units | 🟡 30% (heurística) | EP3 do backlog |
| Auth simples (token) | 🟢 100% | OK pra MVP |
| Auth forte (Ed25519/mTLS) | 🟡 30% | Pós-MVP |
| TLS | 🔴 10% | EP4 — script existe, código não usa |
| TUI client | 🟢 90% | Adicionar tabs novas |
| Histórico no client | 🟡 60% | Schema + purge automation |
| Histórico no server | 🔴 0% | EP6 |
| Sistema de alertas | 🟡 70% | Plugar no dashboard + carregar regras |
| Notificações | 🟡 70% | Plugar Discord, testar fluxo end-to-end |
| Health checks | 🟢 80% | Expor métricas reais em `/metrics` |
| Web dashboard | 🔴 0% | EP6 |
| Packaging | 🟡 40% | systemd unit + install.sh |
| CI/CD | 🟡 30% | GitHub Actions |
| Docs (técnicas) | 🟢 90% | OK |
| Docs (operacionais) | 🟡 60% | Este documento + 02-04 do `00-EXECUCAO/` |

🟢 ≥70% | 🟡 30-70% | 🔴 <30%
