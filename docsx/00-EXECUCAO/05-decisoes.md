# ADRs — Architecture Decision Records

> Decisões importantes com contexto, opções e justificativa.
> Formato: contexto → opções → escolha → consequências.

---

## ADR-001: Persistência server-side via SQLite local

**Data:** 2026-05-08
**Status:** Proposto
**Contexto:** Hoje, o histórico de métricas só vive no cliente (SQLite no workstation). Se o cliente fechar, perdemos dados. Para alertas server-side e dashboards web, precisamos de persistência no agent.

**Opções:**
- A) **SQLite local** em cada agent (`/var/lib/code-monitor/metrics.db`)
- B) **Postgres centralizado** (usar A6 dev cluster)
- C) **TimescaleDB** dedicado (instalar em A7)

**Decisão:** **A) SQLite local**

**Justificativa:**
- Sem dependência circular (postgres do A6 é o que monitoramos)
- Zero infra adicional
- Cabe na RAM/disco dos agents (4 KB por sample × 17280 samples/dia × 30 dias × 5 séries ≈ 10 GB — usar retention agressiva)
- Replicar pra Postgres central é trivial depois (export job)

**Consequências:**
- + Operação simples
- + Resiliente (cada agent autônomo)
- − Query cross-host requer N consultas (uma por agent)
- − Mitigado: aggregator pós-MVP (Épico 6)

---

## ADR-002: Auth simples via access token

**Data:** 2026-05-08
**Status:** Aceito
**Contexto:** Existe infra Ed25519 parcialmente implementada mas não conectada. Decidir se mantém ou remove.

**Opções:**
- A) Manter access token (header gRPC `x-access-token`)
- B) Plugar Ed25519 (chave privada no client, pública autorizada no server)

**Decisão:** **A** para MVP, **B** opcional pós-MVP.

**Justificativa:**
- Token + TLS resolve 95% dos casos de threat (rede interna VPN 10.10.0.x)
- Ed25519 adiciona complexidade de gerenciamento de chaves sem ganho proporcional
- Se virar produto comercial, B vira diferencial (pós-MVP)

**Consequências:**
- + Simples, debugável
- − Token vaza? Rotacionar (já tem `new-token` command)
- TODO: remover código Ed25519 morto se não plugar até v0.5

---

## ADR-003: Postgres collector via socket Unix com peer auth

**Data:** 2026-05-08
**Status:** Proposto
**Contexto:** Como o agent autentica no postgres? Senha em config? mTLS? socket?

**Opções:**
- A) Socket Unix `/var/run/postgresql/.s.PGSQL.<port>` + peer auth
- B) TCP localhost com password em env var
- C) TCP localhost com cert client (postgres mTLS)

**Decisão:** **A**

**Justificativa:**
- Sem senha em arquivo
- Sem porta TCP exposta nem mesmo localhost (postgres pode escutar só socket)
- Peer auth = kernel valida UID = `code-monitor` user
- Funciona out-of-the-box com role `pg_monitor` (built-in postgres 10+)

**Consequências:**
- + Mais seguro
- − Só funciona em mesmo host (impossível agent remoto)
- − Mitigado: agent sempre roda no host do banco no nosso modelo

---

## ADR-004: N-pra-N no MVP, aggregator depois

**Data:** 2026-05-08
**Status:** Aceito
**Contexto:** Cliente conecta direto a cada agent? Ou broker central recebe dos agents e cliente lê do broker?

**Opções:**
- A) N-pra-N: cliente abre N gRPC streams (1 por agent)
- B) Aggregator: agents POSTam para broker central, cliente lê do broker

**Decisão:** **A no MVP**, reavaliar quando N>10 agents.

**Justificativa:**
- N=3 hoje, simplicidade vence
- Sem ponto único de falha
- TUI é o único cliente (não tem "muitos clientes" pedindo dado dos mesmos agents)
- Aggregator vira parte do Épico 6 com web dashboard

**Consequências:**
- + Simples
- − Sem alertas se cliente desligado (mitigado: alertas no agent local)
- − Mitigação adicional: pós-MVP, aggregator opcional

---

## ADR-005: Agent rodando como systemd nativo (não Docker)

**Data:** 2026-05-08
**Status:** Aceito
**Contexto:** Empacotar agent em Docker ou systemd?

**Opções:**
- A) Container Docker
- B) systemd unit nativo

**Decisão:** **B**

**Justificativa:**
- Agent precisa ler `/proc`, `/var/run/docker.sock`, sockets do postgres — bind-mount tudo isso vira complicado
- systemd dá controle granular de recursos (`MemoryMax`, `CPUQuota`)
- Cross-platform: pode portar pra Windows depois (service nativo)
- Binário único Rust não justifica overhead de container

**Consequências:**
- + Footprint mínimo
- + Acesso direto a recursos
- − Não cabe em "stack monitoring containerizado" — mitigado: provê endpoints HTTP pra Prometheus integrar com Grafana já no A7

---

## ADR-006: Bollard para Docker (não shell out)

**Data:** 2026-05-08
**Status:** Proposto
**Contexto:** Coleta de docker stats — usar API nativa ou parsing CLI?

**Opções:**
- A) `bollard` crate (cliente Rust nativo do Docker API)
- B) Spawn `docker stats --no-stream --format json` e parse

**Decisão:** **A**

**Justificativa:**
- Tipado (structs Rust)
- Streaming events disponível (futuro: detectar containers novos)
- Sem overhead de spawn de processo
- Mantido (releases regulares, autor confiável)

**Consequências:**
- + Performance
- + Robustez
- − Dep adicional (~50 deps transitivas, vai pesar build) — aceitável

---

## ADR-007: Alertas residem no servidor (agent), não no cliente

**Data:** 2026-05-08
**Status:** Proposto
**Contexto:** Hoje `AlertManager` está em `shared/`. Onde rodar a engine de alertas?

**Opções:**
- A) Engine roda no agent (server-side)
- B) Engine roda no cliente (TUI)

**Decisão:** **A**

**Justificativa:**
- Quero alerta mesmo se TUI fechado / Diogo dormindo
- Engine local = baixa latência (sample → alert em <5s)
- Disparo direto pra Discord do agent: 1 hop
- Cliente recebe estado já calculado (active alerts list)

**Consequências:**
- + Funciona 24/7 sem cliente
- − Cada agent precisa de webhook URL (config replicado)
- − Mitigação: futuro aggregator centraliza notificações

---

## ADR-008: tokio-postgres em vez de sqlx

**Data:** 2026-05-08
**Status:** Proposto
**Contexto:** Driver postgres em Rust pro coletor.

**Opções:**
- A) `tokio-postgres`
- B) `sqlx` (com feature postgres)
- C) `postgres` (sync)

**Decisão:** **A**

**Justificativa:**
- Não precisamos das features de query macro / migrations do sqlx
- `tokio-postgres` é a base do sqlx — menos peso
- Tokio já é nosso runtime
- API simples e estável

**Consequências:**
- + Build mais leve
- + Menos magia
- − Sem type safety em queries (mas: queries são strings hardcoded e pequenas)

---

## ADR-009: Linguagem Rust mantida (não reescrever em Go)

**Data:** 2026-05-08
**Status:** Aceito (legado)
**Contexto:** Codebase já está em Rust. Vale considerar Go (ecossistema mais maduro pra observability)?

**Opções:**
- A) Manter Rust
- B) Reescrever em Go

**Decisão:** **A**

**Justificativa:**
- 11 commits, código funcional
- Performance Rust > Go pra agent leve
- Ecossistema observability em Go é maior, mas não temos requisito de OpenTelemetry / Prometheus instrumentation pesada
- Custo de reescrever supera benefícios

---

## ADR-010: Versionamento gRPC com prefixo na mensagem (futuro)

**Data:** 2026-05-08
**Status:** Proposto
**Contexto:** Quando expandirmos proto (containers, postgres), clientes velhos com servers novos vão ter problemas.

**Opções:**
- A) Adicionar campo `api_version` em cada response
- B) Versionar service: `MonitorServiceV2`
- C) Não versionar (assumir cliente sempre atualiza)

**Decisão:** **A** combinado com `// reserved` em campos descontinuados.

**Justificativa:**
- protobuf é forward/backward compatible se não remover campos
- `api_version` ajuda cliente a saber quais features pode usar
- B é over-engineering pra escala atual

**Consequências:**
- Adicionar `string api_version = 99;` em todas responses no próximo breaking-friendly release
- Cliente faz `if response.api_version >= "0.3" { use new field }`

---

## Template para novos ADRs

```markdown
## ADR-XXX: Título da decisão

**Data:** YYYY-MM-DD
**Status:** Proposto | Aceito | Substituído por ADR-YYY
**Contexto:** Problema que motiva a decisão.

**Opções:**
- A) ...
- B) ...

**Decisão:** **X**

**Justificativa:**
- ...

**Consequências:**
- + ...
- − ...
```
