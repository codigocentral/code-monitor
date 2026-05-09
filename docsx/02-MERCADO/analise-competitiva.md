# Analise Competitiva Detalhada

## Panorama do Mercado de Monitoramento

O mercado de monitoramento de infraestrutura é fragmentado em 4 categorias principais:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    MAPA COMPETITIVO                                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   PREÇO                                                                     │
│     ▲                                                                       │
│     │                                                                       │
│  $23│                              ┌──────────┐                             │
│     │                              │ Datadog  │ ← Enterprise SaaS           │
│  $15│                              └──────────┘                             │
│     │                                                                       │
│     │                    ┌──────────┐                                       │
│   $5│                    │ Netdata  │ ← Cloud-first                         │
│     │                    │  Cloud   │                                       │
│   $2│    ┌─────────────────────────────────────┐                           │
│     │    │        CODE MONITOR                 │ ← NOSSO ESPAÇO            │
│   $0│────┼──────────────────────────────────────────────────────────►      │
│     │    │                                     │              FEATURES      │
│     │ ┌──┴────┐  ┌────────┐  ┌────────────────┐                            │
│     │ │Glances│  │ htop   │  │ Prometheus +   │                            │
│     │ │       │  │        │  │ Grafana        │                            │
│     │ └───────┘  └────────┘  └────────────────┘                            │
│     │                                                                       │
│     │    SIMPLES ─────────────────────────────────────► COMPLEXO           │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Concorrentes Diretos

### 1. Glances (Python)

**Overview:**
- Ferramenta de monitoramento escrita em Python
- TUI similar ao htop com mais informações
- Open source, comunidade ativa

**Pontos Fortes:**
- Gratuito e open source
- Fácil de instalar via pip
- Web UI opcional
- Plugins extensíveis
- Comunidade estabelecida

**Pontos Fracos:**
- Muito pesado (15% CPU em idle)
- Lento para iniciar (~3s)
- Dependências Python complexas
- Sem multi-servidor nativo
- Interface datada

**Números:**
| Métrica | Valor |
|---------|-------|
| GitHub Stars | ~25k |
| CPU Idle | 15% |
| RAM Usage | 450MB |
| Startup Time | 3s |
| Dependencies | 20+ |

---

### 2. Netdata (C/Cloud)

**Overview:**
- Monitoramento real-time com foco em visualização
- Modelo freemium com cloud obrigatório para features
- Empresa VC-funded, crescimento agressivo

**Pontos Fortes:**
- Dashboards muito bonitos
- Detecção de anomalias com ML
- Milhares de métricas out-of-box
- Alertas integrados
- Grande comunidade

**Pontos Fracos:**
- Pesado (500MB+ RAM)
- Requer cloud para recursos avançados
- Telemetria/dados enviados para cloud
- Pricing confuso
- Vendor lock-in crescente

**Números:**
| Métrica | Valor |
|---------|-------|
| GitHub Stars | ~68k |
| CPU Idle | 5% |
| RAM Usage | 500MB+ |
| Startup Time | 2s |
| Cloud Required | Parcialmente |

---

### 3. Datadog (Enterprise SaaS)

**Overview:**
- Líder de mercado em observabilidade enterprise
- Full-stack: APM, logs, métricas, segurança
- Muito caro, pricing complexo

**Pontos Fortes:**
- Feature-complete
- Integrações com tudo
- Suporte enterprise
- ML e analytics avançados
- Credibilidade de marca

**Pontos Fracos:**
- Extremamente caro ($15-31/host)
- Billing surpresa comum
- Vendor lock-in total
- Overhead de agente
- Overkill para maioria

**Números:**
| Métrica | Valor |
|---------|-------|
| Preço Base | $15/host/mês |
| Com APM | $31/host/mês |
| Custo 50 hosts | $2,620/mês |
| Billing Surprises | Frequentes |
| Setup Time | 1 semana+ |

---

### 4. Prometheus + Grafana

**Overview:**
- Stack open source para monitoramento
- Padrão da indústria para Kubernetes
- Requer conhecimento técnico profundo

**Pontos Fortes:**
- 100% open source
- Escalável horizontalmente
- Ecossistema rico
- Padrão CNCF
- Sem lock-in

**Pontos Fracos:**
- Setup extremamente complexo
- Requer PromQL (curva de aprendizado)
- Manutenção contínua necessária
- Sem alertas simples out-of-box
- Precisa de DevOps dedicado

**Números:**
| Métrica | Valor |
|---------|-------|
| Setup Time | 4-6 semanas |
| Manutenção | Contínua |
| Curva de Aprendizado | Alta |
| TCO (50 hosts) | ~$500/mês* |
| Expertise Requerida | DevOps senior |

*TCO inclui tempo de engenharia

---

## Concorrentes Indiretos

### htop / btop / bottom

- Ferramentas locais apenas
- Sem multi-servidor
- Sem histórico
- Sem alertas

**Por que não competem diretamente:**
- Diferentes use cases
- Complementares, não substitutos

### New Relic / Dynatrace

- Focados em APM, não infra
- Preço ainda maior que Datadog
- Enterprise-only

**Por que não competem diretamente:**
- Mercado diferente (enterprise/APM)
- Preço proibitivo para nosso target

### Zabbix / Nagios

- Legacy, complexos
- Interface antiga
- Curva de aprendizado alta

**Por que não competem diretamente:**
- Percebidos como "velhos"
- Não são opção para novos projetos

---

## Matriz de Comparacao

### Features

| Feature | Code Monitor | Glances | Netdata | Datadog | Prometheus |
|---------|--------------|---------|---------|---------|------------|
| Multi-servidor | ✅ | ⚠️ | ✅ | ✅ | ✅ |
| TUI nativa | ✅ | ✅ | ❌ | ❌ | ❌ |
| Web UI | 🔜 | ✅ | ✅ | ✅ | ✅ |
| Alertas | 🔜 | ❌ | ✅ | ✅ | ✅ |
| Histórico | 🔜 | ❌ | ✅ | ✅ | ✅ |
| API REST | 🔜 | ✅ | ✅ | ✅ | ✅ |
| 100% On-prem | ✅ | ✅ | ⚠️ | ❌ | ✅ |
| Zero config | ✅ | ✅ | ✅ | ❌ | ❌ |
| Docker support | 🔜 | ✅ | ✅ | ✅ | ✅ |
| K8s support | 🔜 | ❌ | ✅ | ✅ | ✅ |

### Performance

| Métrica | Code Monitor | Glances | Netdata | Datadog | Prometheus |
|---------|--------------|---------|---------|---------|------------|
| CPU Idle | 1.5% | 15% | 5% | 3% | 2% |
| RAM Usage | 50MB | 450MB | 500MB | 150MB | 300MB |
| Startup | 0.1s | 3s | 2s | 5s | 10s |
| Binary Size | 15MB | N/A | 50MB | 100MB | 80MB |

### Custo Total (50 hosts, 1 ano)

| Solução | Licença | Infra | Engenharia | Total |
|---------|---------|-------|------------|-------|
| **Code Monitor Pro** | $1,200 | $0 | $500 | **$1,700** |
| Glances | $0 | $0 | $2,000 | $2,000 |
| Netdata Cloud | $2,700 | $0 | $1,000 | $3,700 |
| Datadog | $31,440 | $0 | $500 | $31,940 |
| Prometheus | $0 | $2,000 | $10,000 | $12,000 |

---

## Oportunidades de Mercado

### 1. Migração de Glances
- **Tamanho:** ~100k usuários ativos
- **Pain point:** Performance, falta de recursos
- **Mensagem:** "10x mais rápido, recursos que faltam"

### 2. Escape de Datadog
- **Tamanho:** Milhares de empresas frustradas
- **Pain point:** Custo, billing surpresa
- **Mensagem:** "90% de economia, sem surpresas"

### 3. Alternativa a Netdata Cloud
- **Tamanho:** ~500k instalações
- **Pain point:** Cloud obrigatório, privacidade
- **Mensagem:** "100% on-prem, seus dados são seus"

### 4. Simplicidade vs Prometheus
- **Tamanho:** Imenso (evitam por complexidade)
- **Pain point:** Setup, manutenção, PromQL
- **Mensagem:** "5 minutos, não 5 semanas"

---

## Proximos Passos

- [Preços do Mercado](./precos-mercado.md) - Análise detalhada de pricing
- [Falhas dos Concorrentes](./falhas-concorrentes.md) - Onde eles falham
- [Nosso Posicionamento](./posicionamento.md) - Onde nos encaixamos
