# Funcionalidades Atuais

## Estado do Produto (v1.0)

### Implementado

| Categoria | Feature | Status | Notas |
|-----------|---------|--------|-------|
| **Arquitetura** | Cliente-servidor gRPC | ✅ | Porta 50051 |
| **Arquitetura** | Workspace Cargo | ✅ | shared, server, client |
| **Arquitetura** | Protocol Buffers | ✅ | Compilação via build.rs |
| **Servidor** | Coleta de CPU | ✅ | Uso %, cores |
| **Servidor** | Coleta de memória | ✅ | Total, usado, disponível |
| **Servidor** | Coleta de disco | ✅ | Por mount point |
| **Servidor** | Lista de processos | ✅ | PID, nome, CPU, RAM |
| **Servidor** | Lista de serviços | ✅ | Status, PID, recursos |
| **Servidor** | Info de rede | ✅ | Interfaces, IPs, tráfego |
| **Servidor** | Autenticação token | ✅ | Base64, header gRPC |
| **Servidor** | Configuração TOML | ✅ | config.toml |
| **Cliente** | Dashboard TUI | ✅ | 4 tabs, sparklines |
| **Cliente** | Multi-servidor | ✅ | Lista com status |
| **Cliente** | Navegação teclado | ✅ | Vim-style + arrows |
| **Cliente** | Wizard add server | ✅ | Interativo |
| **Cliente** | Filtro processos | ✅ | Tecla `/` |
| **Cliente** | Auto-reconnect | ✅ | Configurável |
| **Cliente** | Configuração TOML | ✅ | client-config.toml |
| **Cross-platform** | Linux | ✅ | Testado |
| **Cross-platform** | Windows | ✅ | Testado |

---

## Dashboard TUI

### Layout

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  Code Monitor v1.0                              prod-web-01    [?] Help     │
├────────────────┬────────────────────────────────────────────────────────────┤
│                │  ┌─ Overview ─┬─ Services ─┬─ Processes ─┬─ Network ─┐     │
│  SERVERS       │  │                                                    │     │
│  ──────────    │  │  SYSTEM INFORMATION                                │     │
│                │  │  ─────────────────────                             │     │
│  🟢 prod-web-01│  │  Hostname: prod-web-01                             │     │
│  🟢 prod-db-01 │  │  OS: Ubuntu 22.04 LTS                              │     │
│  🟡 staging    │  │  Kernel: 5.15.0-91-generic                         │     │
│  🔴 dev-server │  │  Uptime: 45 days, 3 hours                          │     │
│                │  │                                                    │     │
│  [a] Add       │  │  CPU USAGE                        MEMORY           │     │
│  [t] Token     │  │  ▁▂▃▄▅▆▇█▇▆▅▄▃▂▁▂▃▄▅  45%       ████████░░ 78%    │     │
│  [Del] Remove  │  │                                   12.5GB / 16GB    │     │
│                │  │                                                    │     │
│                │  │  DISK USAGE                                        │     │
│                │  │  /     ████████████░░░░░░░░  58%  (116GB/200GB)    │     │
│                │  │  /home ██████░░░░░░░░░░░░░░  28%  (140GB/500GB)    │     │
│                │  │  /var  ███████████████████░  92%  (46GB/50GB)      │     │
│                │  │                                                    │     │
│                │  │  TOP PROCESSES                                     │     │
│                │  │  nginx      12%  1.2GB                             │     │
│                │  │  postgres   34%  4.5GB                             │     │
│                │  │  redis       2%  0.5GB                             │     │
│                │  │                                                    │     │
│                │  └────────────────────────────────────────────────────┘     │
├────────────────┴────────────────────────────────────────────────────────────┤
│  [Tab] Switch tabs  [↑↓] Navigate  [Enter] Select  [q] Quit  [?] Help       │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Tabs Disponíveis

#### 1. Overview
- Informações do sistema (hostname, OS, kernel, uptime)
- CPU usage com sparkline (últimos 60 samples)
- Memória com progress bar
- Disco por mount point
- Top 5 processos por CPU

#### 2. Services
- Lista de serviços do sistema
- Status (running, stopped)
- PID e recursos
- Tempo de execução
- Ordenação por coluna

#### 3. Processes
- Lista completa de processos
- Filtro por nome (`/`)
- CPU e memória por processo
- Linha de comando
- Ordenação por CPU/RAM

#### 4. Network
- Interfaces de rede
- Endereços IP e MAC
- Status (up/down)
- Tráfego (bytes sent/received)
- Pacotes sent/received

---

## Atalhos de Teclado

### Navegação Global

| Tecla | Ação |
|-------|------|
| `Tab` | Próxima tab |
| `Shift+Tab` | Tab anterior |
| `1-4` | Ir para tab específica |
| `?` | Mostrar ajuda |
| `q` / `Esc` | Sair |

### Lista de Servidores

| Tecla | Ação |
|-------|------|
| `↑` / `k` | Servidor anterior |
| `↓` / `j` | Próximo servidor |
| `Enter` | Conectar/desconectar |
| `a` | Adicionar servidor |
| `t` | Editar token |
| `Delete` | Remover servidor |
| `C` | Conectar todos |
| `D` | Desconectar todos |

### Tabs Específicas

| Tecla | Ação |
|-------|------|
| `/` | Filtrar processos |
| `x` | Limpar filtro |
| `d` | Toggle painel de detalhes |
| `s` | Abrir settings |
| `r` | Refresh servidor atual |
| `R` | Refresh todos |

---

## Metricas Coletadas

### SystemInfo

```rust
pub struct SystemInfo {
    pub hostname: String,
    pub os_name: String,
    pub os_version: String,
    pub kernel_version: String,
    pub uptime_seconds: u64,
    pub cpu_count: u32,
    pub cpu_usage_percent: f64,
    pub memory_total_bytes: u64,
    pub memory_used_bytes: u64,
    pub memory_available_bytes: u64,
    pub disks: Vec<DiskInfo>,
}
```

### ProcessInfo

```rust
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub user: String,
    pub cpu_usage_percent: f64,
    pub memory_bytes: u64,
    pub memory_percent: f64,
    pub command_line: String,
    pub start_time: u64,
    pub status: ProcessStatus,
}
```

### ServiceInfo

```rust
pub struct ServiceInfo {
    pub name: String,
    pub display_name: String,
    pub status: ServiceStatus,
    pub pid: Option<u32>,
    pub cpu_usage_percent: f64,
    pub memory_bytes: u64,
    pub user: String,
    pub uptime_seconds: Option<u64>,
}
```

### NetworkInfo

```rust
pub struct NetworkInterface {
    pub name: String,
    pub ip_addresses: Vec<String>,
    pub mac_address: String,
    pub is_up: bool,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
}
```

---

## Limitacoes Atuais

### Funcionalidades Ausentes

| Feature | Impacto | Prioridade |
|---------|---------|------------|
| Histórico persistente | Alto | P0 |
| Sistema de alertas | Alto | P0 |
| TLS/SSL | Alto | P0 |
| Dashboard web | Médio | P1 |
| Métricas de GPU | Baixo | P2 |
| Métricas de Docker | Médio | P1 |
| Suporte Kubernetes | Médio | P2 |
| API REST | Médio | P1 |

### Problemas Conhecidos

| Issue | Workaround |
|-------|------------|
| Sem TLS | Usar em rede confiável |
| Token em plaintext | Rotacionar regularmente |
| Sem histórico | Dados perdidos ao desconectar |
| Sem alertas | Monitorar manualmente |

---

## Comparativo com Concorrentes

### O Que Já Temos de Vantagem

| Feature | Code Monitor | Glances | Netdata | Datadog |
|---------|--------------|---------|---------|---------|
| Multi-servidor | ✅ | ✅ | ✅ | ✅ |
| TUI nativa | ✅ | ✅ | ❌ | ❌ |
| Baixo consumo CPU | ✅ (1.5%) | ❌ (15%) | ❌ (5%) | N/A |
| Baixo consumo RAM | ✅ (50MB) | ❌ (450MB) | ❌ (500MB) | N/A |
| Setup simples | ✅ | ✅ | ✅ | ✅ |
| 100% On-prem | ✅ | ✅ | ⚠️ | ❌ |
| Binário único | ✅ | ❌ | ❌ | ❌ |
| Zero dependências | ✅ | ❌ | ❌ | ❌ |

### O Que Precisamos Adicionar

| Feature | Glances | Netdata | Datadog | Nós |
|---------|---------|---------|---------|-----|
| Alertas | ❌ | ✅ | ✅ | 🔜 |
| Histórico | ❌ | ✅ | ✅ | 🔜 |
| Web UI | ✅ | ✅ | ✅ | 🔜 |
| API REST | ✅ | ✅ | ✅ | 🔜 |
| TLS | ❌ | ✅ | ✅ | 🔜 |

---

## Proximos Passos

- [Diferenciais](./diferenciais.md) - Por que somos melhores
- [Roadmap](../04-ROADMAP/visao-12-meses.md) - O que vamos construir
