# Backlog técnico — Code Monitor MVP

> Issues acionáveis priorizadas. Granularidade alta o suficiente pra virar GitHub issue/PR.
> Estimativas em **dias úteis ideais** (1d = 6h focado).

**Legenda:** P0=imediato | P1=próximas 2 semanas | P2=mês 1 | P3=mês 2+

---

## Épico 1 — Docker Collector (P0, 3-4 dias)

**Problema:** A8 tem 41 containers, A6 tem 27. Hoje o agente reporta processos do host mas não distingue containers.

**Resultado esperado:** Tab "Containers" no TUI mostrando lista com nome, imagem, status, CPU%, MEM, healthcheck.

### EP1.T1 — Refatorar `monitor.rs` em módulo `collectors/`
- **Tipo:** refactor
- **Estimativa:** 0.5d
- **Definition of done:**
  - Trait `Collector` em `server/src/collectors/mod.rs`
  - `system.rs` migrado mantendo todos os testes passando
  - `cargo test --all` verde

### EP1.T2 — Adicionar `bollard` e implementar `DockerCollector`
- **Tipo:** feature
- **Estimativa:** 1.5d
- **Dependências:** EP1.T1
- **Aceitação:**
  - `cargo add bollard` (workspace)
  - Conecta a `/var/run/docker.sock` (default) ou path configurável
  - Coleta para cada container: `id, name, image, status, state, health, restart_count, started_at, networks`
  - Coleta stats: `cpu_percent, memory_usage, memory_limit, network_rx, network_tx, block_io`
  - Trata graceful: docker daemon down ou socket inacessível → log warn, retorna lista vazia
  - Bench: <50ms para 50 containers

### EP1.T3 — Adicionar mensagens proto + RPC
- **Tipo:** feature
- **Estimativa:** 0.5d
- **Dependências:** EP1.T2
- **Aceitação:**
  - Mensagem `ContainerInfo` em `monitoring.proto`
  - RPC `GetContainers(ContainersRequest) returns (ContainersResponse)` com filter opcional
  - Variant `Container(ContainerInfo)` em `SystemUpdate.update_type`
  - `cargo build` regenera código sem erro

### EP1.T4 — Tab "Containers" no dashboard
- **Tipo:** feature
- **Estimativa:** 1d
- **Dependências:** EP1.T3
- **Aceitação:**
  - Tab acessível via `5` ou navegação Tab
  - Tabela com colunas: Name, Image, Status, CPU%, MEM, Started
  - Sort por coluna (clique numérico ou tecla)
  - Filter por nome (`/`)
  - Cor: vermelho se unhealthy, amarelo se restart_count>0
  - Atalho `l` mostra logs (futuro EP)

### EP1.T5 — Testes + docs
- **Estimativa:** 0.5d
- **Aceitação:**
  - Teste integrado: subir container alpine, esperar coletor ver
  - Update `AGENTS.md` e `README.md`

**Total EP1:** 4 dias

---

## Épico 2 — Postgres Collector multi-cluster (P0, 4-5 dias)

**Problema:** Hoje, todo monitoramento de banco é manual (script Python ad-hoc). Quero ver cache hit, conexões, top queries no TUI.

**Resultado esperado:** Tab "Postgres" no TUI mostrando, para cada cluster configurado: bancos por size, conexões, top 5 queries.

### EP2.T1 — Decisão: tokio-postgres vs sqlx
- **Tipo:** ADR
- **Estimativa:** 0.25d
- **Output:** ADR-008 em `05-decisoes.md`
- **Recomendação preliminar:** `tokio-postgres` — mais leve, sem feature de macro/migration que não precisamos

### EP2.T2 — Schema de config postgres + parser
- **Estimativa:** 0.5d
- **Aceitação:**
  - `[[collectors.postgres]]` arrays no config.toml com `name, host, port, user, password (opcional)`, `socket_path` alternativo
  - Suporta auth via socket Unix (peer auth)
  - Suporta password via env var `${POSTGRES_<NAME>_PASSWORD}`

### EP2.T3 — Implementar `PostgresCollector`
- **Estimativa:** 2d
- **Dependências:** EP2.T2
- **Aceitação:**
  - Pool de conexões (1 conexão persistente por cluster)
  - Coleta:
    - Lista de bancos: `SELECT datname, pg_database_size(datname), numbackends FROM pg_stat_database WHERE datname NOT LIKE 'template%'`
    - Cache hit ratio: `blks_hit*100.0/nullif(blks_hit+blks_read,0)`
    - Conexões por estado: `state, count(*) FROM pg_stat_activity GROUP BY state`
    - Idle in transaction: count separado
    - Top queries (se `pg_stat_statements` instalado): `SELECT query, calls, total_exec_time, mean_exec_time FROM pg_stat_statements ORDER BY total_exec_time DESC LIMIT 5`
  - `application_name='code-monitor-agent'`
  - `statement_timeout=10s`
  - Reconnect automático em caso de erro

### EP2.T4 — Proto + UI
- **Estimativa:** 1d
- **Aceitação:**
  - Mensagens `PostgresInfo, DatabaseInfo, TopQuery`
  - Tab "Postgres" mostra todos clusters configurados
  - Por cluster: lista de bancos com sparkline de cache hit, contador de conexões
  - Detail panel: top 5 queries

### EP2.T5 — Testes
- **Estimativa:** 0.5d
- **Aceitação:**
  - docker compose com postgres + dados sintéticos
  - Coleta retorna dados consistentes

**Total EP2:** 4-5 dias

---

## Épico 3 — MariaDB + systemd + Alertas plugados (P0, 3 dias)

### EP3.T1 — MariaDBCollector
- **Estimativa:** 1d
- **Aceitação:**
  - `mysql_async` adicionado
  - Coleta: tamanho dos schemas (`information_schema.tables`), processlist filtrado por `command<>'Sleep'`, status InnoDB básico
  - Conexão via TCP localhost ou socket
  - Pool com 1 conexão

### EP3.T2 — SystemdCollector
- **Estimativa:** 1d
- **Aceitação:**
  - `zbus` (preferido) ou parsing `systemctl --no-pager`
  - Lista de units configuráveis no TOML
  - Por unit: `ActiveState, SubState, MainPID, ExecMainStartTimestamp, MemoryCurrent`

### EP3.T3 — Plugar AlertManager no client
- **Estimativa:** 0.5d
- **Aceitação:**
  - No loop principal do dashboard, após cada update, chamar `alert_manager.process_metrics()`
  - Carregar regras de `client-config.toml [[alerts.rules]]`
  - Disparar canais configurados via `notifications.rs`

### EP3.T4 — Discord notifier ponta-a-ponta
- **Estimativa:** 0.5d
- **Aceitação:**
  - Webhook URL via TOML
  - Alert dispara mensagem real no Discord
  - Cooldown de 5min funcional (não floodar)

**Total EP3:** 3 dias

---

## Épico 4 — TLS + Packaging (P1, 3 dias)

### EP4.T1 — TLS no servidor
- **Estimativa:** 0.5d
- **Aceitação:**
  - `[tls] cert_path, key_path, ca_path` no config
  - `Server::builder().tls_config(ServerTlsConfig::new().identity(...).client_ca_root(...))`
  - Logs mostram TLS habilitado no banner

### EP4.T2 — TLS no cliente
- **Estimativa:** 0.5d
- **Aceitação:**
  - `Channel::tls_config(ClientTlsConfig::new().ca_certificate(...))`
  - Suporta cert do cliente (opcional, mTLS)

### EP4.T3 — install.sh
- **Estimativa:** 1d
- **Aceitação:**
  - Detecta arch (x86_64/aarch64) e baixa binário do GitHub Releases
  - Cria user `code-monitor`
  - Cria diretórios `/etc/code-monitor`, `/var/lib/code-monitor`, `/var/log/code-monitor`
  - Gera certs via `generate-certs.sh` se não existirem
  - Cria systemd unit `code-monitor-server.service`
  - Inicia o serviço

### EP4.T4 — GitHub Actions release
- **Estimativa:** 1d
- **Aceitação:**
  - Build cross-platform: linux-x86_64, linux-aarch64, darwin-x86_64, darwin-aarch64, windows-x86_64
  - Tag `v*` dispara release
  - Anexa binários comprimidos no GitHub Release

**Total EP4:** 3 dias

---

## Épico 5 — Deploy real em A6/A7/A8 (P1, 2 dias)

### EP5.T1 — Deploy A6, A7, A8
- **Estimativa:** 1d
- **Aceitação:**
  - 3 systemd units rodando
  - 3 entradas no client-config do Diogo
  - TUI mostra os 3 com status "connected"

### EP5.T2 — Configurar regras de alerta reais
- **Estimativa:** 0.5d
- **Aceitação:**
  - Regras documentadas em `02-aplicacao-infra.md` aplicadas
  - Webhook do Discord do Diogo configurado
  - Trigger manual de teste (ex: stress de CPU)

### EP5.T3 — 1 semana de soak test
- **Estimativa:** observação contínua
- **Aceitação:**
  - <3 falsos positivos em 7 dias
  - 0 incidentes do agent (crash, vazamento)
  - Documentar achados em `06-soak-test-report.md` (criar)

**Total EP5:** 2 dias trabalho + 7 dias observação

---

## Épico 6 — Server-side storage + Web Dashboard (P2, 5 dias)

### EP6.T1 — Mover storage pro server
- **Estimativa:** 1.5d
- **Aceitação:**
  - SQLite em `/var/lib/code-monitor/metrics.db` no agent
  - Schema com tabelas separadas: `system_metrics, container_metrics, postgres_metrics, mariadb_metrics`
  - Retention policy configurável (default 30 dias)
  - Job de purge a cada 1h

### EP6.T2 — REST API para query histórico
- **Estimativa:** 1d
- **Aceitação:**
  - axum router em `/api/v1/`
  - Endpoints: `GET /servers/:id/metrics?from=...&to=...&type=...`
  - Auth via mesmo access_token

### EP6.T3 — Web dashboard mínimo
- **Estimativa:** 2d
- **Aceitação:**
  - Página única servida pelo agent (axum) ou aggregator
  - Stack: htmx + Pico CSS (sem frontend pesado)
  - Mostra: lista de servers, gráfico CPU/mem últimas 24h por server

### EP6.T4 — Mobile-friendly view
- **Estimativa:** 0.5d
- **Aceitação:**
  - CSS responsivo
  - Layout single-column em <600px

**Total EP6:** 5 dias

---

## Épico 7 — Coleta de logs (P3, futuro)

- Tail dos últimos N de logs de containers selecionados
- Stream via gRPC com filter por nível
- Visualizar no TUI (tab Logs)

**Estimativa:** 4-5 dias

---

## Épico 8 — Cost insights / Anomaly detection (P3, futuro)

- Detectar containers com leak progressivo (regressão linear simples)
- Estimar custo baseado em consumo (por classe de instância configurada)
- Sugestões: "container X cresceu 200% em 7 dias, considerar restart agendado"

**Estimativa:** 1-2 semanas

---

## Resumo cronológico

| Sprint | Período | Épicos | Total dias |
|---|---|---|---|
| 1 | Sem 1 | EP1 (Docker) | 4d |
| 2 | Sem 2 | EP2 (Postgres) | 5d |
| 3 | Sem 3 | EP3 (MariaDB+systemd+alertas) | 3d |
| 4 | Sem 4 | EP4 (TLS+packaging) | 3d |
| 5 | Sem 5 | EP5 (deploy) + soak | 2d + obs |
| 6 | Sem 6-7 | EP6 (storage+web) | 5d |
| **MVP** | **6-7 sem** | **EP1-6** | **22d** |
| Pós-MVP | Sem 8+ | EP7-8 | 10-15d |

---

## Tracking

Sugestão: criar GitHub Project com colunas `Backlog → In Progress → Review → Done`. Cada T (tarefa) vira issue. Cada Épico vira milestone.

Tags úteis: `collector`, `frontend`, `infra`, `security`, `docs`, `breaking-change`, `mvp`.
