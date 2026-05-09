# Cost Insights - Economia Inteligente

## Visao Geral

O Cost Insights é o diferencial que transforma Code Monitor de "mais uma ferramenta de monitoramento" em "ferramenta que se paga sozinha".

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                                                                             │
│   FERRAMENTAS TRADICIONAIS              CODE MONITOR                       │
│   ════════════════════════              ════════════                        │
│                                                                             │
│   "Aqui estão seus dados.               "Você está desperdiçando           │
│    Boa sorte!"                           $2.340/mês. Veja como              │
│                                          economizar."                       │
│                                                                             │
│   📊 Métricas                            📊 Métricas                        │
│   📈 Gráficos                            📈 Gráficos                        │
│   🔔 Alertas                             🔔 Alertas                         │
│                                          💰 Economia identificada           │
│                                          💡 Recomendações acionáveis        │
│                                          📉 ROI comprovado                  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## O Problema

### Desperdício Invisível

Empresas gastam em média **30-40% a mais** do que precisam em infraestrutura porque:

| Problema | Causa | Frequência |
|----------|-------|------------|
| Processos zumbis | Deploys antigos esquecidos | 80% das empresas |
| Over-provisioning | "Melhor sobrar que faltar" | 90% das empresas |
| Memory leaks | Apps que nunca reiniciam | 60% das empresas |
| Serviços não usados | POCs abandonadas | 70% das empresas |
| Logs excessivos | Debug em produção | 50% das empresas |

### Custo Real

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    ANATOMIA DO DESPERDÍCIO                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Empresa com 50 servidores                                                │
│   Gasto mensal: $10.000                                                    │
│                                                                             │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                                                                     │   │
│   │   NECESSÁRIO (70%)              DESPERDÍCIO (30%)                   │   │
│   │   ████████████████████████████  ████████████                        │   │
│   │          $7.000                      $3.000                         │   │
│   │                                                                     │   │
│   │   ├── Aplicações reais              ├── Processos zumbis            │   │
│   │   ├── Bancos de dados               ├── Servidores grandes demais   │   │
│   │   ├── Cache ativo                   ├── Serviços não usados         │   │
│   │   └── Logs necessários              └── Logs debug                  │   │
│   │                                                                     │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│   DESPERDÍCIO ANUAL: $36.000                                               │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## A Solução

### Detecções Automáticas

#### 1. Processos Zumbis

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ ⚠️  PROCESSO SUSPEITO DETECTADO                                            │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Servidor:    prod-web-01                                                 │
│   Processo:    node:8080 (PID 12345)                                       │
│   Rodando há:  45 dias                                                     │
│   CPU média:   0.3%                                                        │
│   RAM:         512MB                                                       │
│   Conexões:    0 (últimos 7 dias)                                          │
│                                                                             │
│   Diagnóstico: Provavelmente um deploy antigo que foi esquecido.           │
│                Não está recebendo tráfego.                                 │
│                                                                             │
│   Economia estimada: $15/mês                                               │
│                                                                             │
│   [Investigar] [Criar alerta] [Ignorar por 30 dias]                        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Lógica de detecção:**
```rust
fn detect_zombie_process(process: &Process, history: &History) -> Option<Waste> {
    // Critérios:
    // 1. Rodando há mais de 7 dias
    // 2. CPU média < 1%
    // 3. Sem conexões de rede (se aplicável)
    // 4. Não é um serviço de sistema conhecido

    if process.uptime_days > 7
        && process.cpu_avg_7d < 1.0
        && process.network_connections == 0
        && !KNOWN_SYSTEM_SERVICES.contains(&process.name)
    {
        Some(Waste::ZombieProcess {
            process: process.clone(),
            estimated_savings: calculate_resource_cost(process),
        })
    } else {
        None
    }
}
```

#### 2. Over-Provisioning

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ 💡 OPORTUNIDADE DE RIGHTSIZING                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Servidor:    prod-api-02                                                 │
│   Tipo atual:  8 vCPU, 32GB RAM ($320/mês)                                 │
│                                                                             │
│   Uso real (últimos 30 dias):                                              │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │ CPU:                                                                │   │
│   │ Média: 12%  ████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░            │   │
│   │ Pico:  28%  ███████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░            │   │
│   │                                                                     │   │
│   │ RAM:                                                                │   │
│   │ Média: 8GB  ████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ (25%)     │   │
│   │ Pico: 12GB  ████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ (38%)     │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│   Recomendação: Migrar para 4 vCPU, 16GB RAM ($160/mês)                    │
│   Economia:     $160/mês ($1.920/ano)                                      │
│   Risco:        Baixo (pico ainda teria 44% de margem)                     │
│                                                                             │
│   [Ver análise completa] [Ignorar] [Agendar migração]                      │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

#### 3. Memory Leaks

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ 🔴 MEMORY LEAK DETECTADO                                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Servidor:    prod-app-01                                                 │
│   Processo:    java:app.jar (PID 5678)                                     │
│                                                                             │
│   Padrão detectado:                                                        │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                                                              ▄▄▄▄   │   │
│   │                                                         ▄▄▄▄▀▀▀▀   │   │
│   │                                                    ▄▄▄▄▀▀▀▀        │   │
│   │                                               ▄▄▄▄▀▀▀▀             │   │
│   │                                          ▄▄▄▄▀▀▀▀                  │   │
│   │                                     ▄▄▄▄▀▀▀▀                       │   │
│   │    Memória              ▄▄▄▄▄▄▄▄▄▄▀▀▀▀                            │   │
│   │    crescendo       ▄▄▄▄▀▀▀▀▀▀▀▀▀                                  │   │
│   │    50MB/dia   ▄▄▄▄▀▀▀▀                                            │   │
│   │          ▄▄▄▄▀▀▀▀                                                  │   │
│   │     ▄▄▄▄▀▀▀▀                                                       │   │
│   │ ▄▄▄▄▀▀                                                             │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│     Dia 1    5       10       15       20       25       30               │
│                                                                             │
│   RAM atual:     6.2GB                                                     │
│   RAM limite:    8GB                                                       │
│   Taxa:          +50MB/dia                                                 │
│   Crash em:      ~36 dias (se não reiniciar)                               │
│                                                                             │
│   [Ver detalhes] [Criar alerta] [Agendar restart]                          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

#### 4. Serviços Não Utilizados

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ ⚠️  SERVIÇO SEM UTILIZAÇÃO                                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Servidor:    prod-db-01                                                  │
│   Serviço:     elasticsearch (porta 9200)                                  │
│   Rodando há:  120 dias                                                    │
│   RAM usada:   2.1GB                                                       │
│   Requisições: 0 (últimos 30 dias)                                         │
│                                                                             │
│   Diagnóstico: Serviço está ativo mas não recebe nenhuma requisição.       │
│                Possivelmente uma POC ou integração abandonada.             │
│                                                                             │
│   Economia estimada: $45/mês (RAM liberada)                                │
│                                                                             │
│   [Investigar] [Desativar] [Ignorar]                                       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Dashboard de Economia

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  Code Monitor    │ Overview │ Services │ Processes │ Network │ 💰 Savings │ │
├──────────────────┴──────────────────────────────────────────────────────────┤
│                                                                             │
│   ECONOMIA POTENCIAL                                                       │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                                                                     │   │
│   │          💰 $2.340/mês                                              │   │
│   │             $28.080/ano                                             │   │
│   │                                                                     │   │
│   │   ┌─────────────────────────────────────────────────────────────┐   │   │
│   │   │ Já economizado (ações tomadas)        $1.250/mês            │   │   │
│   │   │ █████████████████████████░░░░░░░░░░░░ 53%                   │   │   │
│   │   │                                                             │   │   │
│   │   │ Pendente (requer ação)                $1.090/mês            │   │   │
│   │   │ ███████████████████░░░░░░░░░░░░░░░░░░ 47%                   │   │   │
│   │   └─────────────────────────────────────────────────────────────┘   │   │
│   │                                                                     │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│   RECOMENDAÇÕES ATIVAS                                                     │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                                                                     │   │
│   │   🔴 Alta Prioridade (3)                                           │   │
│   │   ├── Memory leak em prod-app-01 (crash em 5 dias)                 │   │
│   │   ├── Elasticsearch não usado em prod-db-01 ($45/mês)              │   │
│   │   └── Servidor prod-web-03 com 8% CPU ($120/mês para rightsizing)  │   │
│   │                                                                     │   │
│   │   🟡 Média Prioridade (7)                                          │   │
│   │   ├── 3 processos zumbis identificados ($35/mês total)             │   │
│   │   ├── 2 servidores over-provisioned ($180/mês total)               │   │
│   │   └── Redis com 0 keys em staging ($25/mês)                        │   │
│   │                                                                     │   │
│   │   🟢 Baixa Prioridade (12)                                         │   │
│   │   └── 12 otimizações menores ($85/mês total)                       │   │
│   │                                                                     │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│   HISTÓRICO DE ECONOMIA                                                    │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │          $1.2k                                                      │   │
│   │     ▄▄▄                                                             │   │
│   │    ████▄▄                                                           │   │
│   │   ███████▄▄       ▄▄                                               │   │
│   │  ██████████▄▄   ▄████                                              │   │
│   │ ████████████████████████                                           │   │
│   │ Jan  Fev  Mar  Abr  Mai  Jun                                       │   │
│   │                                                                     │   │
│   │ Total economizado desde o início: $8.450                           │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Integrações de Custo

### Cloud Providers

Para estimativas precisas de economia:

| Provider | Integração | Dados |
|----------|------------|-------|
| AWS | API (read-only) | Preços EC2, RDS, ElastiCache |
| GCP | API (read-only) | Preços Compute, Cloud SQL |
| Azure | API (read-only) | Preços VMs, Managed DBs |
| DigitalOcean | API (read-only) | Preços Droplets |
| Hetzner | Manual/API | Preços de servidores |

### Configuração

```toml
# config.toml

[cost_insights]
enabled = true
currency = "USD"  # ou "BRL"

# Custo manual (se não usar integração cloud)
[cost_insights.manual]
cost_per_vcpu_hour = 0.05
cost_per_gb_ram_hour = 0.01
cost_per_gb_disk_month = 0.10

# Integração AWS (opcional)
[cost_insights.aws]
enabled = true
access_key_id = "${AWS_ACCESS_KEY_ID}"
secret_access_key = "${AWS_SECRET_ACCESS_KEY}"
region = "us-east-1"
```

---

## ROI do Cost Insights

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    PROPOSTA DE VALOR                                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   "Code Monitor se paga no primeiro mês"                                   │
│                                                                             │
│   Cenário típico: 50 servidores                                            │
│                                                                             │
│   Custo Code Monitor Business:                                             │
│   └── 50 × $4 = $200/mês                                                   │
│                                                                             │
│   Economia média identificada:                                             │
│   ├── 5 processos zumbis          $75/mês                                  │
│   ├── 3 rightsizing               $240/mês                                 │
│   ├── 2 memory leaks (prevenção)  $50/mês                                  │
│   ├── 2 serviços não usados       $90/mês                                  │
│   └── Logs otimizados             $30/mês                                  │
│   TOTAL:                          $485/mês                                 │
│                                                                             │
│   ROI = ($485 - $200) / $200 = 142%                                        │
│   Payback: < 2 semanas                                                     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Roadmap Cost Insights

| Versão | Feature | Prioridade |
|--------|---------|------------|
| v1.1 | Detecção de processos zumbis | P0 |
| v1.2 | Detecção de memory leaks | P0 |
| v1.3 | Sugestão de rightsizing | P1 |
| v1.4 | Detecção de serviços não usados | P1 |
| v2.0 | Dashboard Savings | P0 |
| v2.1 | Histórico de economia | P1 |
| v2.2 | Integração AWS | P2 |
| v2.3 | Integração GCP/Azure | P2 |
| v3.0 | Recomendações com ML | P3 |

---

## Próximos Passos

- [Diferenciais](./diferenciais.md) - Outros diferenciais do produto
- [Modelo de Negócio](../03-ESTRATEGIA/modelo-negocio.md) - Como monetizar
