# Modelo de Negocio

## Estrategia: Open Core

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    MODELO OPEN CORE                                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                         COMMUNITY (FREE)                            │   │
│   │                                                                     │   │
│   │   • TUI Dashboard completo                                          │   │
│   │   • Multi-servidor (até 10)                                         │   │
│   │   • Todas as métricas                                               │   │
│   │   • Histórico 24h                                                   │   │
│   │   • 5 alertas básicos                                               │   │
│   │   • API REST (read-only)                                            │   │
│   │                                                                     │   │
│   │   Licença: MIT                                                      │   │
│   │   Código: 100% público no GitHub                                    │   │
│   │                                                                     │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                         PRO / BUSINESS                              │   │
│   │                                                                     │   │
│   │   • Nodes ilimitados                                                │   │
│   │   • Histórico estendido (7-90 dias)                                │   │
│   │   • Alertas ilimitados                                              │   │
│   │   • Webhooks e integrações                                          │   │
│   │   • Web Dashboard                                                   │   │
│   │   • SSO / RBAC                                                      │   │
│   │   • Suporte prioritário                                             │   │
│   │                                                                     │   │
│   │   Licença: Commercial                                               │   │
│   │   Código: Binários distribuídos                                     │   │
│   │                                                                     │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Tiers Detalhados

### Community (Gratis)

**Objetivo:** Adoção massiva, comunidade, boca a boca

| Feature | Limite |
|---------|--------|
| Nodes monitorados | 10 |
| Histórico | 24 horas |
| Alertas | 5 |
| Usuários | 1 |
| API | Read-only |
| Suporte | Comunidade |

**Ideal para:**
- Desenvolvedores individuais
- Homelabs
- POCs e avaliações
- Pequenos projetos

**Conversão esperada:** 5% → Pro

---

### Pro ($2/node/mes)

**Objetivo:** Receita de cauda longa, startups

| Feature | Limite |
|---------|--------|
| Nodes monitorados | Ilimitado |
| Histórico | 7 dias |
| Alertas | 50 |
| Usuários | 5 |
| Webhooks | Slack, Discord, Email |
| API | Full access |
| Suporte | Email (48h SLA) |

**Preço:**
- Mensal: $2/node
- Anual: $20/node (17% desconto)

**Ideal para:**
- Startups early-stage
- Pequenas empresas
- Freelancers com clientes
- Agências

**Conversão esperada:** 10% → Business

---

### Business ($4/node/mes)

**Objetivo:** Receita principal, empresas estabelecidas

| Feature | Limite |
|---------|--------|
| Nodes monitorados | Ilimitado |
| Histórico | 90 dias |
| Alertas | Ilimitado |
| Usuários | 25 |
| Webhooks | Todos + Custom |
| API | Full + Webhooks |
| Web Dashboard | Incluído |
| SSO | SAML, OAuth |
| Audit Log | 1 ano |
| Suporte | Priority (24h SLA) |

**Preço:**
- Mensal: $4/node
- Anual: $40/node (17% desconto)

**Ideal para:**
- Scale-ups
- Empresas mid-market
- Times de DevOps
- MSPs (Managed Service Providers)

**Conversão esperada:** 5% → Enterprise

---

### Enterprise (Custom)

**Objetivo:** Grandes contratos, credibilidade

| Feature | Limite |
|---------|--------|
| Nodes monitorados | Ilimitado |
| Histórico | Ilimitado |
| Alertas | Ilimitado |
| Usuários | Ilimitado |
| RBAC | Granular |
| On-prem | 100% |
| Air-gapped | Suportado |
| SLA | 99.9% garantido |
| Suporte | 24/7 + TAM dedicado |
| Integrações | Custom |

**Preço:**
- A partir de $10k/ano
- Baseado em volume e necessidades

**Ideal para:**
- Empresas Fortune 500
- Governo
- Financeiro/Healthcare
- Qualquer um com >500 nodes

---

## Modelo de Receita

### Projecao Conservadora

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    PROJEÇÃO DE RECEITA (ANO 1-3)                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   ANO 1 (Lançamento)                                                       │
│   ├── Mês 6:  1.000 users, 50 Pro ($1k MRR)                               │
│   ├── Mês 12: 10.000 users, 200 Pro, 20 Business ($5k MRR)                │
│   └── ARR Final: $60k                                                      │
│                                                                             │
│   ANO 2 (Crescimento)                                                      │
│   ├── Users: 50.000                                                        │
│   ├── Pro: 800 ($6.4k MRR)                                                 │
│   ├── Business: 100 ($8k MRR, média 20 nodes)                              │
│   ├── Enterprise: 5 ($4k MRR)                                              │
│   └── ARR Final: $220k                                                     │
│                                                                             │
│   ANO 3 (Escala)                                                           │
│   ├── Users: 200.000                                                       │
│   ├── Pro: 2.500 ($20k MRR)                                                │
│   ├── Business: 400 ($32k MRR)                                             │
│   ├── Enterprise: 20 ($16k MRR)                                            │
│   └── ARR Final: $816k                                                     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Mix de Receita Ideal

```
       ANO 1          ANO 2          ANO 3
    ┌─────────┐    ┌─────────┐    ┌─────────┐
    │         │    │  ENT    │    │  ENT    │
    │         │    │  20%    │    │  25%    │
    │  PRO    │    ├─────────┤    ├─────────┤
    │  80%    │    │  BUS    │    │  BUS    │
    │         │    │  50%    │    │  50%    │
    │         │    ├─────────┤    ├─────────┤
    │         │    │  PRO    │    │  PRO    │
    └─────────┘    │  30%    │    │  25%    │
                   └─────────┘    └─────────┘
```

---

## Metricas-Chave

### SaaS Metrics

| Métrica | Ano 1 | Ano 2 | Ano 3 |
|---------|-------|-------|-------|
| **MRR** | $5k | $18k | $68k |
| **ARR** | $60k | $220k | $816k |
| **ARPU** | $25 | $35 | $50 |
| **Churn** | 5% | 3% | 2% |
| **LTV** | $300 | $700 | $1,500 |
| **CAC** | $50 | $100 | $150 |
| **LTV:CAC** | 6:1 | 7:1 | 10:1 |

### Conversion Funnel

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    FUNIL DE CONVERSÃO                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Visitantes Website         100,000                                       │
│         │                                                                   │
│         ▼ (20%)                                                             │
│   Downloads                   20,000                                       │
│         │                                                                   │
│         ▼ (50%)                                                             │
│   Instalações Ativas         10,000                                        │
│         │                                                                   │
│         ▼ (3%)                                                              │
│   Trial/Pro Start               300                                        │
│         │                                                                   │
│         ▼ (70%)                                                             │
│   Conversão Paga               210                                         │
│         │                                                                   │
│         ▼ (15%)                                                             │
│   Upgrade Business              32                                         │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Canais de Receita

### 1. Self-Service (70% da receita)

**Fluxo:**
1. Usuario baixa versão gratuita
2. Usa por semanas/meses
3. Atinge limite (11 nodes ou precisa de features)
4. Upgrade via website (Stripe)

**Vantagens:**
- Zero CAC
- Escala infinita
- Automático

### 2. Sales-Assisted (20% da receita)

**Fluxo:**
1. Lead entra via formulário ou trial
2. SDR qualifica
3. Demo com AE
4. POC de 30 dias
5. Contrato anual

**Quando usar:**
- > 100 nodes
- Requisitos enterprise
- Integrações custom

### 3. Partnerships (10% da receita)

**Tipos:**
- **MSPs:** Revenda para clientes
- **Cloud Providers:** Marketplace
- **Consultories:** Implementação

**Modelo:**
- Comissão de 20-30%
- Co-marketing

---

## Pricing Psychology

### Por Que $2/node

1. **Preço psicológico:** < café diário
2. **Comparação:** 87% menos que Datadog
3. **Escalabilidade:** Sobe com uso
4. **Simplicidade:** Fácil de calcular

### Por Que Anual

1. **Cash flow:** Dinheiro upfront
2. **Churn:** Menor em contratos longos
3. **Desconto:** 17% (2 meses grátis)
4. **Commitment:** Usuário mais engajado

---

## Upsell Strategy

### Community → Pro

**Gatilhos:**
- 11º node adicionado
- Feature de alerta necessária
- Histórico > 24h desejado

**Tática:**
- Popup gentil no limite
- Email com benefits
- Onboarding personalizado

### Pro → Business

**Gatilhos:**
- Time cresce (> 5 usuários)
- Precisa de SSO
- Compliance requer audit log
- Volume > 50 nodes (desconto)

**Tática:**
- CSM proativo
- ROI calculator
- Case studies similares

### Business → Enterprise

**Gatilhos:**
- Expansão global
- Requisitos air-gapped
- SLA necessário
- Volume > 500 nodes

**Tática:**
- Executive sponsorship
- TAM dedicado
- Custom roadmap

---

## Proximos Passos

- [Go-to-Market](./go-to-market.md) - Plano de lançamento
- [Marketing](./marketing.md) - Conteúdo e comunidade
