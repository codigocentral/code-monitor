# Code Monitor — Plano de Atividades

> **Documento gerencial** — visão consolidada do que existe, o que falta e o caminho mais curto pra ter o sistema rodando em produção monitorando A6/A7/A8.
>
> **Audiência:** Diogo (decisor) + futuros colaboradores
> **Status:** v1.0 — 2026-05-08
> **Documentos detalhados:** `docsx/00-EXECUCAO/`

---

## TL;DR

O **code-monitor** já tem **80% da fundação técnica pronta** (gRPC, TUI rica, alertas, notificações, SQLite no cliente, health checks, Docker). Mas falta o **20% que importa pro nosso caso de uso real**:

1. **Coletar Docker** (CPU/RAM por container) — hoje só pega processos do host
2. **Coletar PostgreSQL/MariaDB** (cache hit, conexões, top queries) — hoje zero
3. **TLS funcional** (existe `generate-certs.sh` mas channel/server não usam)
4. **Persistência server-side** (histórico hoje só fica no cliente)
5. **Empacotar e instalar** nos 3 servidores reais (A6/A7/A8)

**MVP usável em produção: 4-6 semanas de trabalho focado.**

---

## 1. Onde estamos

### 1.1 Estrutura do projeto

```
code-monitor/
├── shared/      # Tipos comuns + protobuf + alertas + notificações
├── server/      # Agente que roda nos servidores monitorados (gRPC)
├── client/      # CLI/TUI no workstation que vê tudo
├── docsx/       # Docs estratégicos (produto, mercado, roadmap, técnico, ação)
└── doc/         # Doc operacional (APT repository guide)
```

Stack: **Rust 2021** + Tokio + tonic (gRPC) + sysinfo + tui-rs + rusqlite + axum (health) + reqwest (notifications).

### 1.2 Já implementado (✅ 11 commits)

| Camada | Item | Onde |
|---|---|---|
| Protocolo | gRPC + protobuf | `shared/src/proto/monitoring.proto` |
| Coleta | CPU, mem, disco, processos, serviços, rede | `server/src/monitor.rs` |
| Servidor | Auth via access token | `server/src/config.rs`, `service.rs` |
| Servidor | Health/ready/metrics HTTP | `server/src/health.rs` |
| Cliente | Multi-server TUI (4 tabs) | `client/src/dashboard.rs` (113KB) |
| Cliente | Histórico SQLite | `client/src/storage.rs` |
| Cliente | Wizard, filtros, sparklines | `client/src/dashboard.rs` |
| Cross-cutting | Sistema de alertas (CPU/Mem/Disk) | `shared/src/alerts.rs` |
| Cross-cutting | Notificações Webhook/Slack/Discord | `shared/src/notifications.rs` |
| Infra | Dockerfile.server + Dockerfile.client | raiz |
| Infra | docker-compose.yml | raiz |
| Infra | TLS cert generation script | `generate-certs.sh` |
| Docs | 6 categorias: PRODUTO/MERCADO/ESTRATEGIA/ROADMAP/TECNICO/ACAO | `docsx/` |

### 1.3 O que está parcial (⚠️)

| Item | Estado | Detalhe |
|---|---|---|
| TLS | Cert script existe, **channel/server não usam** | `client/src/client.rs:49` faz `Channel::from_shared` sem `tls_config()`; servidor não tem `Server::builder().tls_config()` |
| Auth Ed25519 | Infra existe, **não está integrada** | `server/src/auth.rs` e `client/src/auth.rs` definem chaves mas o fluxo de auth usa só access token |
| Service detection | Heurística por nome | `monitor.rs:280` filtra processos com nome terminando em "d", "service", "svc" — **não usa systemd nativo** |
| Alert manager | Existe em `shared/`, **não conectado ao client** | Não vi integração no dashboard pra disparar `process_metrics` |
| Streaming | Implementado no servidor | Cliente usa `stream_system_updates` em modo `#[allow(dead_code)]` |

### 1.4 O que falta crítico para nosso uso (❌)

| Lacuna | Impacto pra nós |
|---|---|
| **Sem coleta de Docker** | A8 tem 41 containers; A6 tem 27. Não saberíamos uso real. |
| **Sem coleta de Postgres** | Não saberíamos cache hit, conexões, top queries — exatamente o que mais usamos hoje. |
| **Sem coleta de MariaDB** | mautic_db é peça crítica — sem visibilidade. |
| **Sem coleta de systemd units** | `systemctl is-active` pra cada serviço (gitlab-runner, postgresql@18-prod, etc.). |
| **Sem persistência no server** | Histórico só sobrevive enquanto o client estiver aberto. |
| **Sem agregador central** | Cliente faz N conexões diretas (N=3). Pra crescer, precisa de um broker/agregador. |
| **Sem suporte a TLS real** | Trafegar via internet hoje exige VPN ou expor token em claro. |
| **Sem packaging** | Pra instalar em A6/A7/A8 vc compila manualmente. |
| **Sem dashboards web** | Hoje só TUI — bom pra dev, ruim pra ver no celular. |

### 1.5 Estado dos docs em `docsx/`

Tem **31 arquivos .md** com plano comercial (preços, mercado, posicionamento), roadmap 12 meses dividido em 4 fases, e detalhes técnicos. **Esses docs assumem produto comercial SaaS** (Tiers Community/Pro/Business/Enterprise, projeção de ARR, etc.). Pode ficar pra depois — o objetivo imediato é **ter algo funcionando pra mim antes de virar produto**.

---

## 2. Onde queremos chegar (MVP de uso interno)

### 2.1 Caso de uso primário

**"Toda manhã o Diogo abre 1 comando no terminal e vê o estado dos 3 servidores (A6, A7, A8) num dashboard, com alertas pra Discord quando algo passar do limite."**

Cenários reais que tenho em mãos hoje (e o sistema atual não cobre):

1. mautic_db no A8 saturando 100% CPU → quero alerta automático.
2. Bloat acumulando em `email_queue` no codemindra_dev → quero gráfico de tamanho da tabela.
3. Container BelleSintra subindo de 192MB → 3.5GB em 8 dias → quero alerta de memory leak.
4. Postgres prod 5433 com cache hit caindo → quero acompanhar.
5. Disco do A6 em 70% subindo → quero alerta antes de bater 90%.
6. Worker do Paraiso disparando REFRESH MATERIALIZED VIEW em loop → quero detectar.

### 2.2 Funcionalidades MVP

**Must-have (fase 1):**

- ✅ Já existe: CPU, mem, disco do host, processos, rede
- ➕ Adicionar: **docker stats** (CPU/MEM por container, status, restart count)
- ➕ Adicionar: **postgres** (lista de bancos, sizes, cache hit, conexões, top 5 queries via pg_stat_statements)
- ➕ Adicionar: **mariadb** (processlist, slow query)
- ➕ Adicionar: **systemd units** importantes (status + uptime)
- ➕ Adicionar: **alertas → Discord** (já temos `notifications.rs::DiscordChannel`, falta plugar)
- ➕ Persistência: opção de **server-side SQLite** pra ter histórico mesmo sem cliente conectado
- ➕ TLS real: clientes e servidor usando o `generate-certs.sh`
- ➕ Packaging: **systemd unit** + script de instalação pra Ubuntu

**Should-have (fase 2):**

- Dashboard web (axum + websocket + alguma UI leve, htmx ou astro)
- Aggregator central que coleta de N agents (1 sumário pra mim)
- Coleta de logs (tail dos últimos N de containers selecionados)
- Histórico longo (ex: 30 dias) com retention policy

**Nice-to-have (fase 3+):**

- Plugin/Extension system (custom collectors)
- Tudo que está em `docsx/03-ESTRATEGIA/` (modelo SaaS, tiers, billing)

---

## 3. Plano de execução

### 3.1 Caminho mais curto pra MVP (semanas 1-6)

| Semana | Entrega | Por quê |
|---|---|---|
| **1** | Docker collector + protobuf novo + UI no dashboard | Maior gap, maior valor imediato |
| **2** | Postgres collector (multi-cluster, suporta 5432 + 5433) | Segundo maior gap |
| **3** | MariaDB + systemd collectors + alertas plugados | Fecha coleta + ativa o que já temos |
| **4** | TLS funcionando + packaging (deb/systemd) | Pré-requisito pra rodar 24/7 |
| **5** | Deploy nos 3 servidores reais + ajuste fino | Operação real |
| **6** | Persistência server-side + dashboard web básico | Quality of life |

### 3.2 Issues acionáveis (backlog inicial)

> Detalhes em `docsx/00-EXECUCAO/03-backlog.md`. Resumo:

**Épico 1 — Docker collector** (estimativa 3-4 dias)
- [ ] EP1.T1: Novo trait `Collector` em `server/src/collectors/mod.rs`
- [ ] EP1.T2: Implementar `DockerCollector` lendo `/var/run/docker.sock` (crate `bollard`)
- [ ] EP1.T3: Adicionar `ContainerInfo` no protobuf
- [ ] EP1.T4: Adicionar tab "Docker" no dashboard
- [ ] EP1.T5: Testes unitários + benchmarks

**Épico 2 — Postgres collector** (estimativa 4-5 dias)
- [ ] EP2.T1: Decidir biblioteca (`tokio-postgres` ou `sqlx`) — sugestão: `tokio-postgres` (mais leve)
- [ ] EP2.T2: Config TOML pra múltiplos clusters (host, port, user, password ou socket)
- [ ] EP2.T3: Coletor com queries: `pg_database_size`, `pg_stat_database`, `pg_stat_activity`, `pg_stat_statements`
- [ ] EP2.T4: Proto + UI no dashboard
- [ ] EP2.T5: Testes contra postgres em docker

**Épico 3 — MariaDB + systemd + alertas** (estimativa 3 dias)
- [ ] EP3.T1: MariaDB collector (`mysql_async`) com `SHOW PROCESSLIST` + `SHOW ENGINE INNODB STATUS`
- [ ] EP3.T2: systemd collector via dbus (`zbus`) ou parsing `systemctl --no-pager`
- [ ] EP3.T3: Plugar `AlertManager` no dashboard (rodar `process_metrics` a cada update)
- [ ] EP3.T4: Configurar canais Discord + email no `client-config.toml`

**Épico 4 — TLS + packaging** (estimativa 3 dias)
- [ ] EP4.T1: TLS no servidor (`Server::builder().tls_config()`) + cert path no config
- [ ] EP4.T2: TLS no cliente (`Channel::tls_config()`)
- [ ] EP4.T3: Script `install.sh` que baixa binário, gera config, cria systemd unit
- [ ] EP4.T4: Pipeline GitHub Actions pra build cross-platform (Linux x86_64 + ARM)

**Épico 5 — Deploy real** (estimativa 2 dias)
- [ ] EP5.T1: Instalar agent no A6, A7, A8
- [ ] EP5.T2: Configurar canais de alerta (Discord do Diogo)
- [ ] EP5.T3: Definir thresholds reais com base no que vimos hoje (CPU>85% por 5min, mem>90%, disco>85%)
- [ ] EP5.T4: Validar 1 semana sem incidentes falsos

**Épico 6 — Server-side storage + Web** (estimativa 5 dias)
- [ ] EP6.T1: Mover `MetricsStorage` pra server (com retention policy)
- [ ] EP6.T2: Endpoint REST pra query histórico
- [ ] EP6.T3: Dashboard web (axum + htmx ou Astro) — mínimo: gráficos por host

**Total estimado:** 20-22 dias úteis = ~5 semanas tempo integral, ~10 semanas em paralelo com outras coisas.

### 3.3 Métricas de sucesso

| KPI | Meta MVP |
|---|---|
| Tempo entre sintoma e detecção | <5min (vs hoje: minha sessão manual) |
| Servidores monitorados | 3 (A6, A7, A8) |
| Containers monitorados | ≥80 (A6=27, A7=13, A8=41) |
| Bancos postgres monitorados | ≥17 (1 dev + 12 prod + 4 main) |
| Falsos positivos em 1 semana | <3 |
| Uso de CPU do agent | <2% |
| Uso de RAM do agent | <100MB |

---

## 4. Decisões arquiteturais a tomar

> Detalhes em `docsx/00-EXECUCAO/05-decisoes.md`.

| # | Decisão | Opções | Recomendação |
|---|---|---|---|
| ADR-1 | Persistência server-side: SQLite ou Postgres? | A) SQLite local; B) Postgres do A6 | **A** — não criar dependência circular (postgres do A6 é o que monitoramos) |
| ADR-2 | Auth: token simples ou Ed25519? | A) Manter token; B) Ativar Ed25519 | **A** — token via env var + TLS resolve 95% |
| ADR-3 | Coleta postgres: agent local ou remota? | A) Agent no A6 conecta socket Unix; B) Agent remoto via TCP | **A** — mais seguro, sem expor postgres |
| ADR-4 | Aggregator: precisa? | A) Cliente fala N-pra-N; B) Server agregador único | **A no MVP** — N=3, não justifica complexidade. Reavaliar quando N>10 |
| ADR-5 | Onde rodar agent: docker ou nativo? | A) Docker; B) systemd nativo | **B** — agent precisa ler /proc, /var/run/docker.sock, e ser leve |
| ADR-6 | Coleta docker: bollard ou shell out? | A) `bollard` crate; B) `docker stats` parsing | **A** — tipado, menos erro |
| ADR-7 | Alertas: aonde residem? | A) Cliente; B) Servidor | **B** — quero alertas mesmo sem cliente aberto |

---

## 5. Riscos e mitigações

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Coleta postgres pesar no banco real | Média | Alto | Queries leves + `statement_timeout=5s`; rodar em conexão dedicada com `application_name='code-monitor'` |
| Agent vazar memória em uptime longo | Média | Alto | Bench em CI; limitar com cgroup/systemd |
| Mudança de schema sysinfo quebrar | Alta | Médio | Pin de versão + testes integrados |
| Discord rate-limit em flood de alertas | Alta | Baixo | Cooldown de 5min entre alertas do mesmo tipo (já existe em `alerts.rs::can_trigger_again`) |
| Conflito de portas 50051 com outros serviços | Baixa | Baixo | Configurável, default seguro |
| Eu (Diogo) não ter tempo de seguir plano | Alta | Alto | Priorizar Épico 1 (Docker) — ROI claro mesmo standalone |

---

## 6. Próximos passos imediatos

### Esta semana
1. **Validar este plano** (~30 min de revisão)
2. **Criar issues no GitHub** com base no backlog
3. **Começar Épico 1** (Docker collector) — sufixo `feat/docker-collector`

### Próximas 2 semanas
4. Épico 1 entregue + Épico 2 começado
5. Demo: rodar agent no A8 e ver containers no TUI

### Próximas 4 semanas
6. Épicos 1-3 entregues
7. Decidir: continuar (Épicos 4-5) ou pausar e usar o que tem

---

## 7. Referências cruzadas

- **Análise técnica detalhada:** `docsx/00-EXECUCAO/01-gap-analysis.md`
- **Como aplicar na infra real:** `docsx/00-EXECUCAO/02-aplicacao-infra.md`
- **Backlog técnico completo:** `docsx/00-EXECUCAO/03-backlog.md`
- **Documentação produto/comercial:** `docsx/01-PRODUTO/` até `docsx/06-ACAO/`
- **Requisito original:** `necessidade.md`
- **Guia para agents (LLM):** `AGENTS.md`

---

*Mantido por: Diogo Costa*
*Última revisão: 2026-05-08*
