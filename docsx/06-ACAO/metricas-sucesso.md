# Metricas de Sucesso

## Framework de Métricas

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    PIRAMIDE DE MÉTRICAS                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│                         ┌─────────────┐                                    │
│                         │   NORTE     │  ← Métrica Norte                   │
│                         │   MRR/ARR   │     A única que importa            │
│                         └──────┬──────┘                                    │
│                    ┌───────────┴───────────┐                               │
│                    │      CORE METRICS     │  ← Métricas de negócio        │
│                    │  Customers, Churn     │     Diretamente ligadas       │
│                    │  LTV, CAC, NPS        │     ao Norte                  │
│                    └───────────┬───────────┘                               │
│              ┌─────────────────┴─────────────────┐                         │
│              │         LEADING INDICATORS        │  ← Indicadores          │
│              │   Signups, Activations, Usage     │     antecipados         │
│              │   Engagement, Retention           │                         │
│              └─────────────────┬─────────────────┘                         │
│        ┌───────────────────────┴───────────────────────┐                   │
│        │              VANITY METRICS                   │  ← Bom para       │
│        │    Stars, Downloads, Visitors, Followers      │     marketing,    │
│        │                                               │     não decisões  │
│        └───────────────────────────────────────────────┘                   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Métricas por Fase

### Fase 1: Fundação (M1-3)

**Foco:** Validação técnica e comunidade inicial

| Métrica | Target M1 | Target M2 | Target M3 |
|---------|-----------|-----------|-----------|
| GitHub Stars | 100 | 300 | 500 |
| Downloads | 200 | 500 | 1,000 |
| Beta testers | 20 | 50 | 100 |
| Discord members | 50 | 100 | 200 |
| Issues resolvidos | 70% | 80% | 90% |
| Test coverage | 50% | 60% | 70% |

**Dashboard:**
```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    FASE 1 DASHBOARD                                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   GitHub Stars          Downloads           Beta Testers                   │
│   ┌──────────────┐      ┌──────────────┐    ┌──────────────┐               │
│   │     342      │      │     782      │    │      67      │               │
│   │   ▲ +15%     │      │   ▲ +23%     │    │   ▲ +12%     │               │
│   │   Target:500 │      │  Target:1000 │    │  Target:100  │               │
│   └──────────────┘      └──────────────┘    └──────────────┘               │
│                                                                             │
│   Issues Resolved       Test Coverage       Discord Members                │
│   ┌──────────────┐      ┌──────────────┐    ┌──────────────┐               │
│   │     85%      │      │     62%      │    │     134      │               │
│   │   ▲ Target:90│      │   ▲ Target:70│    │   ▲ Target:200               │
│   └──────────────┘      └──────────────┘    └──────────────┘               │
│                                                                             │
│   NPS (Beta)            Bug Rate            Response Time                  │
│   ┌──────────────┐      ┌──────────────┐    ┌──────────────┐               │
│   │      42      │      │    0.3/day   │    │    4 hours   │               │
│   │   Target: 40 │      │   Target: <1 │    │  Target: <24h│               │
│   └──────────────┘      └──────────────┘    └──────────────┘               │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

### Fase 2: Expansão (M4-6)

**Foco:** Launch, primeiros clientes pagantes, validação de mercado

| Métrica | Target M4 | Target M5 | Target M6 |
|---------|-----------|-----------|-----------|
| Website visitors | 10k | 30k | 50k |
| Signups | 100 | 300 | 500 |
| Trial starts | 30 | 80 | 150 |
| Conversão trial→paid | 8% | 10% | 12% |
| Clientes pagantes | 5 | 25 | 50 |
| MRR | $100 | $500 | $1,000 |
| NPS | 35 | 40 | 45 |

**Funil de Conversão:**
```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    FUNIL DE CONVERSÃO (M6)                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Website Visitors                                                          │
│   ████████████████████████████████████████████████████ 50,000 (100%)       │
│                              │                                              │
│                              ▼ 20% CTR                                      │
│   Downloads                                                                 │
│   ██████████████████████████████████████ 10,000 (20%)                      │
│                              │                                              │
│                              ▼ 50% Activation                               │
│   Active Users                                                             │
│   ████████████████████ 5,000 (10%)                                         │
│                              │                                              │
│                              ▼ 10% Trial                                    │
│   Trial Starts                                                             │
│   ████ 500 (1%)                                                            │
│                              │                                              │
│                              ▼ 10% Conversion                               │
│   Paying Customers                                                         │
│   █ 50 (0.1%)                                                              │
│                                                                             │
│   MRR: $1,000 | ARPU: $20 | CAC: $50                                       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

### Fase 3: Escala (M7-9)

**Foco:** Crescimento sustentável, enterprise pipeline

| Métrica | Target M7 | Target M8 | Target M9 |
|---------|-----------|-----------|-----------|
| Clientes pagantes | 75 | 110 | 150 |
| MRR | $1,500 | $2,500 | $4,000 |
| Enterprise leads | 2 | 5 | 10 |
| Nodes monitorados | 500 | 1,000 | 2,000 |
| Churn mensal | <5% | <4% | <3% |
| NPS | 45 | 48 | 50 |
| LTV | $300 | $400 | $500 |
| LTV:CAC | 5:1 | 6:1 | 7:1 |

---

### Fase 4: Liderança (M10-12)

**Foco:** Enterprise, liderança de nicho

| Métrica | Target M10 | Target M11 | Target M12 |
|---------|------------|------------|------------|
| Clientes pagantes | 170 | 185 | 200 |
| Enterprise clientes | 2 | 3 | 5 |
| MRR | $4,500 | $4,800 | $5,000 |
| ARR | - | - | $60,000 |
| Nodes monitorados | 2,500 | 3,500 | 5,000 |
| NPS | 50 | 52 | 55 |
| Churn anual | - | - | <10% |

---

## Métricas Detalhadas

### Aquisição

| Métrica | Definição | Como Medir |
|---------|-----------|------------|
| **Website Visitors** | Visitantes únicos/mês | Google Analytics |
| **Downloads** | Binários baixados | Download counter |
| **Signups** | Contas criadas | Database |
| **Trial Starts** | Trials iniciados | Database |
| **Conversion Rate** | Signups → Paid | Stripe + DB |
| **CAC** | Custo por cliente | Marketing spend / New customers |

### Engajamento

| Métrica | Definição | Como Medir |
|---------|-----------|------------|
| **DAU/MAU** | Usuários ativos | Database login |
| **Session Duration** | Tempo médio de uso | Telemetry |
| **Nodes Connected** | Servidores monitorados | Database |
| **Feature Usage** | Uso de features | Telemetry |
| **API Calls** | Chamadas de API | Logs |

### Retenção

| Métrica | Definição | Como Medir |
|---------|-----------|------------|
| **Day 1 Retention** | Voltou no dia 1 | Database |
| **Week 1 Retention** | Ativo na semana 1 | Database |
| **Month 1 Retention** | Ativo no mês 1 | Database |
| **Monthly Churn** | Cancelamentos/mês | Stripe |
| **Net Revenue Retention** | Expansão - Churn | Stripe |

### Receita

| Métrica | Definição | Como Medir |
|---------|-----------|------------|
| **MRR** | Monthly Recurring Revenue | Stripe |
| **ARR** | Annual Recurring Revenue | MRR × 12 |
| **ARPU** | Average Revenue Per User | MRR / Customers |
| **LTV** | Lifetime Value | ARPU × Avg lifetime |
| **LTV:CAC** | Ratio valor/custo | LTV / CAC |

### Satisfação

| Métrica | Definição | Como Medir |
|---------|-----------|------------|
| **NPS** | Net Promoter Score | Survey |
| **CSAT** | Customer Satisfaction | Survey |
| **Support Tickets** | Tickets/cliente | Helpdesk |
| **Resolution Time** | Tempo para resolver | Helpdesk |
| **GitHub Stars** | Estrelas no repo | GitHub |

---

## Dashboards

### Executive Dashboard (Mensal)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    EXECUTIVE DASHBOARD - Janeiro 2026                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                         NORTH STAR: MRR                             │   │
│   │                                                                     │   │
│   │                           $2,450                                    │   │
│   │                          ▲ +23% MoM                                 │   │
│   │                                                                     │   │
│   │   Jan  Feb  Mar  Apr  May  Jun  Jul  Aug  Sep  Oct  Nov  Dec       │   │
│   │    │    │    │    │    │    │    │    │    │    │    │    │        │   │
│   │    ▁    ▂    ▃    ▄    ▅    ▆    ?    ?    ?    ?    ?    ?        │   │
│   │  $100 $500 $1k $1.5k $2k $2.4k                         Target:$5k  │   │
│   │                                                                     │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐   │
│   │  Customers   │  │    Churn     │  │     NPS      │  │   LTV:CAC    │   │
│   │     87       │  │    3.2%      │  │      47      │  │     6.2      │   │
│   │   ▲ +15      │  │   ▼ -0.8%    │  │   ▲ +3       │  │   ▲ +0.4     │   │
│   └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Growth Dashboard (Semanal)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    GROWTH DASHBOARD - Week 23                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   ACQUISITION                           ACTIVATION                         │
│   ┌─────────────────────────────┐      ┌─────────────────────────────┐     │
│   │ Website    12,450 (+8%)     │      │ Signups      142 (+12%)     │     │
│   │ Downloads   1,823 (+15%)    │      │ Activations   89 (63%)      │     │
│   │ Trials        34 (+22%)     │      │ D7 Retention  72%           │     │
│   └─────────────────────────────┘      └─────────────────────────────┘     │
│                                                                             │
│   RETENTION                             REVENUE                            │
│   ┌─────────────────────────────┐      ┌─────────────────────────────┐     │
│   │ Active Users   456          │      │ New MRR      $320           │     │
│   │ Churned          2          │      │ Expansion     $85           │     │
│   │ Net Change     +23          │      │ Churn        -$45           │     │
│   └─────────────────────────────┘      │ Net MRR     +$360           │     │
│                                        └─────────────────────────────┘     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Alertas e Thresholds

### Alertas Críticos (Ação Imediata)

| Métrica | Threshold | Ação |
|---------|-----------|------|
| Churn mensal | > 10% | Investigar causas, outreach |
| NPS | < 20 | Survey, customer calls |
| Bug críticos | > 0 | All hands on deck |
| Uptime | < 99% | Incident response |

### Alertas de Atenção (Investigar)

| Métrica | Threshold | Ação |
|---------|-----------|------|
| Conversion rate | < 5% | A/B test, pricing review |
| Activation rate | < 50% | Onboarding review |
| D7 retention | < 60% | Product feedback |
| CAC | > $150 | Channel optimization |

### Indicadores Positivos (Capitalizar)

| Métrica | Threshold | Ação |
|---------|-----------|------|
| NPS | > 60 | Request testimonials |
| Expansion rate | > 20% | Upsell campaigns |
| Organic growth | > 50% | Double down |
| Referrals | > 30% | Referral program |

---

## Ferramentas de Medição

| Métrica | Ferramenta |
|---------|------------|
| Website analytics | Google Analytics / Plausible |
| Product analytics | PostHog / Mixpanel |
| Revenue | Stripe Dashboard |
| NPS/Surveys | Typeform / SurveyMonkey |
| Support | Intercom / Freshdesk |
| Uptime | Better Uptime / Checkly |
| Error tracking | Sentry |
| Logs | Loki / CloudWatch |

---

## Review Cadence

| Frequência | Métricas | Participantes |
|------------|----------|---------------|
| Diário | Signups, Errors, Churn signals | Founder |
| Semanal | Acquisition, Activation, Revenue | Team |
| Mensal | All core metrics, NPS | Team + Advisors |
| Trimestral | Strategic review, Roadmap | Team + Board |

---

## Próximos Passos

- [Quick Wins](./quick-wins.md) - Ações imediatas
- [Roadmap Fase 1](../04-ROADMAP/fase1-fundacao.md) - Primeiros 3 meses
