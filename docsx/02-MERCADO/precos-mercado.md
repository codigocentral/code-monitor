# Precos do Mercado

## Panorama de Pricing

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    ESPECTRO DE PREÇOS (por host/mês)                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  $0        $5        $10       $15       $20       $25       $30           │
│   │         │         │         │         │         │         │            │
│   ├─────────┼─────────┼─────────┼─────────┼─────────┼─────────┤            │
│   │         │         │         │         │         │         │            │
│   │  ┌──────┴──┐      │         │         │         │         │            │
│   │  │ CODE    │      │         │         │         │         │            │
│   │  │ MONITOR │      │         │         │         │         │            │
│   │  │ $0-$4   │      │         │         │         │         │            │
│   │  └─────────┘      │         │         │         │         │            │
│   │         │         │         │         │         │         │            │
│   │   ┌─────┴─────┐   │         │         │         │         │            │
│   │   │  NETDATA  │   │         │         │         │         │            │
│   │   │  $0-$4.5  │   │         │         │         │         │            │
│   │   └───────────┘   │         │         │         │         │            │
│   │         │         │         │         │         │         │            │
│   │         │         │   ┌─────┴─────────┴─────────┴────┐    │            │
│   │         │         │   │         DATADOG              │    │            │
│   │         │         │   │        $15-$31               │    │            │
│   │         │         │   └──────────────────────────────┘    │            │
│   │         │         │         │         │         │         │            │
│   ▼         ▼         ▼         ▼         ▼         ▼         ▼            │
│  Open     Budget    Mid-tier  Premium  Enterprise                          │
│  Source                                                                     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Analise por Concorrente

### Glances

| Tier | Preço | Inclui |
|------|-------|--------|
| Core | $0 | TUI, métricas básicas |
| Web | $0 | +Interface web (bottle) |
| Plugins | $0 | +Docker, GPU, etc. |

**Modelo:** 100% gratuito, open source
**Monetização:** Nenhuma (projeto comunidade)

**Custo Real:**
- Licença: $0
- Overhead de CPU: ~15% (custa infra)
- Custo de infra adicional: ~$10-20/host/mês*

*Baseado em overhead de CPU em cloud

---

### Netdata

| Tier | Preço/node | Nodes | Retenção | Inclui |
|------|------------|-------|----------|--------|
| Free | $0 | Unlimited local | - | Agente local |
| Community | $0 | 5 conectados | 24h | Cloud básico |
| Homelab | $0 | Unlimited | 24h | Personal use |
| Pro | $3.00 | Unlimited | 14 dias | Alertas, multi-user |
| Business | $4.50 | Unlimited | 90 dias | SSO, audit log |
| Enterprise | Custom | Unlimited | Custom | On-prem, SLA |

**Modelo:** Freemium com cloud lock-in
**Monetização:** Features avançadas requerem cloud

**Custo Real (50 nodes, Business):**
- Licença: $4.50 × 50 = $225/mês
- Anual: $2,700

**Problemas de Pricing:**
- Precisa de cloud para histórico >24h
- Precisa de cloud para alertas avançados
- Dados vão para servidores deles

---

### Datadog

| Componente | Preço | Notas |
|------------|-------|-------|
| Infrastructure | $15/host | Base |
| APM | $31/host | Com tracing |
| Log Management | $0.10/GB ingest | + indexing |
| Custom Metrics | $0.05/métrica | Após 100 |
| Synthetics | $5/10k tests | API monitoring |
| RUM | $1.50/1k sessions | Real user |

**Modelo:** À la carte, pay-per-use
**Monetização:** Cada feature cobra separado

**Custo Real (50 hosts, típico):**
```
Infrastructure:     50 × $15   = $750
Custom Metrics:     500 × $0.05 = $25
Log Ingest:        100GB × $0.10 = $10
Log Indexing:      50M × $1.70 = $85
APM (se usar):     50 × $31    = $1,550
─────────────────────────────────────
SUBTOTAL:                        $2,420/mês
High-water mark adjustment:      +$200
─────────────────────────────────────
TOTAL TÍPICO:                    $2,620/mês
ANUAL:                           $31,440
```

**Problemas de Pricing:**
- High-water mark: cobra pelo pico, não média
- Custom metrics: explodem facilmente
- Billing surpresa: muito comum
- Lock-in: difícil sair depois

---

### Prometheus + Grafana

| Componente | Licença | Custo Real |
|------------|---------|------------|
| Prometheus | $0 | Setup + manutenção |
| Grafana | $0 | Setup + manutenção |
| AlertManager | $0 | Configuração |
| Exporters | $0 | Instalação em cada host |

**Modelo:** 100% open source
**Monetização:** Grafana Cloud opcional

**Custo Real (50 hosts, 1 ano):**
```
Licenças:           $0
Infra (storage):    $2,000/ano
Tempo DevOps:
  - Setup inicial:  40h × $100 = $4,000
  - Manutenção:     4h/mês × $100 × 12 = $4,800
  - Troubleshooting: 10h × $100 = $1,000
─────────────────────────────────────
TOTAL ANO 1:        $11,800
TOTAL ANO 2+:       $7,800/ano
```

**Problemas de Pricing:**
- TCO escondido em tempo de engenharia
- Requer especialista PromQL
- Manutenção contínua necessária

---

## Nossa Estrategia de Pricing

### Filosofia

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                                                                             │
│   "PREÇO JUSTO = Valor Claro + Custo Previsível + Sem Surpresas"           │
│                                                                             │
│   ┌────────────────┐  ┌────────────────┐  ┌────────────────┐               │
│   │                │  │                │  │                │               │
│   │   Preço Fixo   │  │  Por Node      │  │  Sem Extras    │               │
│   │   por Node     │  │  Não por       │  │  Escondidos    │               │
│   │                │  │  Métrica       │  │                │               │
│   └────────────────┘  └────────────────┘  └────────────────┘               │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Nossos Tiers

| Tier | Preço | Nodes | Histórico | Alertas | Suporte |
|------|-------|-------|-----------|---------|---------|
| **Community** | $0 | 10 | 24h | 5 | Community |
| **Pro** | $2/node | ∞ | 7 dias | 50 | Email |
| **Business** | $4/node | ∞ | 90 dias | ∞ | Priority |
| **Enterprise** | Custom | ∞ | ∞ | ∞ | 24/7 SLA |

### Por Que Esses Preços

1. **Community $0:**
   - Compete com Glances/htop
   - Suficiente para hobbyistas
   - Boca a boca natural
   - Limite de 10 nodes = upgrade natural

2. **Pro $2/node:**
   - 85% mais barato que Datadog
   - 33% mais barato que Netdata Business
   - Sweet spot para startups
   - Valor claro: alertas + histórico

3. **Business $4/node:**
   - Ainda 73% mais barato que Datadog
   - Features enterprise (SSO, API)
   - Para empresas estabelecidas
   - Suporte prioritário incluso

4. **Enterprise Custom:**
   - Para >500 nodes
   - On-prem completo
   - SLA garantido
   - Integrações custom

---

## Comparativo de Custo Total

### Cenário: 50 Servidores, 1 Ano

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    CUSTO TOTAL ANUAL - 50 HOSTS                            │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Datadog        ████████████████████████████████████████  $31,440          │
│                                                                             │
│  Prometheus*    ██████████████████████████               $11,800           │
│                                                                             │
│  Netdata Biz    █████                                     $2,700           │
│                                                                             │
│  CODE MONITOR   ██                                        $1,200           │
│  (Pro)                                                                      │
│                                                                             │
│  Glances        █                                            $0            │
│  (+ overhead)   ████████                                  $6,000*          │
│                                                                             │
│  * Inclui custo de overhead de CPU/RAM em infra cloud                      │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### ROI vs Concorrentes

| vs | Economia Anual | % Economia |
|----|----------------|------------|
| Datadog | $30,240 | 96% |
| Prometheus | $10,600 | 90% |
| Netdata Business | $1,500 | 56% |
| Glances (real) | $4,800 | 80% |

---

## Messaging de Preco

### Para CTOs/Managers

> "Code Monitor custa menos que um café por servidor por mês.
> Datadog custa um almoço executivo. Todo dia."

### Para Developers

> "Monitoring que não come seu budget de cloud.
> $2/servidor. Sem asteriscos. Sem surpresas."

### Para Enterprises

> "Seus dados, seu datacenter, seu controle.
> Pricing transparente que cabe no orçamento."

---

## Proximos Passos

- [Falhas dos Concorrentes](./falhas-concorrentes.md) - Onde eles falham
- [Nosso Posicionamento](./posicionamento.md) - Onde nos encaixamos
