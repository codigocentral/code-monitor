# Visao Geral do Produto

## O Que E o Code Monitor

Code Monitor é uma **ferramenta de monitoramento de sistemas multi-servidor** construída em Rust, que funciona como um "htop distribuído" - permitindo monitorar múltiplos servidores remotos a partir de uma única interface de terminal.

### Proposta de Valor

> "Monitoramento de servidores que não custa mais que sua infraestrutura.
> Leve, rápido, sem surpresas."

---

## Arquitetura

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           ARQUITETURA CODE MONITOR                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   SEU COMPUTADOR (Workstation)                                              │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                      monitor-client                                 │   │
│   │                                                                     │   │
│   │   ┌─────────────────┐    ┌─────────────────┐    ┌──────────────┐   │   │
│   │   │   TUI Dashboard │    │   Gerenciador   │    │   Histórico  │   │   │
│   │   │   (tui-rs)      │    │   de Conexões   │    │   (SQLite)   │   │   │
│   │   └─────────────────┘    └─────────────────┘    └──────────────┘   │   │
│   │                                                                     │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                                    │                                        │
│                                    │ gRPC/HTTP2 (Port 50051)                │
│                                    │                                        │
│   ┌────────────────────────────────┼────────────────────────────────────┐   │
│   │                                │                                    │   │
│   │            SEUS SERVIDORES (Linux/Windows)                          │   │
│   │                                │                                    │   │
│   │   ┌──────────────┐   ┌──────────────┐   ┌──────────────┐           │   │
│   │   │monitor-server│   │monitor-server│   │monitor-server│           │   │
│   │   │              │   │              │   │              │           │   │
│   │   │ prod-web-01  │   │ prod-db-01   │   │ staging-01   │           │   │
│   │   └──────────────┘   └──────────────┘   └──────────────┘           │   │
│   │         │                  │                   │                    │   │
│   │         └──────────────────┴───────────────────┘                    │   │
│   │                            │                                        │   │
│   │                    Coleta de Métricas                               │   │
│   │                    (sysinfo crate)                                  │   │
│   │                            │                                        │   │
│   │         ┌──────────────────┴───────────────────┐                    │   │
│   │         │                                      │                    │   │
│   │   ┌─────┴─────┐  ┌──────────┐  ┌──────────┐  ┌┴─────────┐          │   │
│   │   │    CPU    │  │  Memory  │  │   Disk   │  │ Network  │          │   │
│   │   │  Cores    │  │  RAM     │  │  Usage   │  │ Traffic  │          │   │
│   │   │  Usage    │  │  Swap    │  │  I/O     │  │ Latency  │          │   │
│   │   └───────────┘  └──────────┘  └──────────┘  └──────────┘          │   │
│   │                                                                     │   │
│   │   ┌───────────┐  ┌──────────┐  ┌──────────┐                        │   │
│   │   │ Processes │  │ Services │  │  System  │                        │   │
│   │   │  PID      │  │  Status  │  │  Uptime  │                        │   │
│   │   │  CPU/RAM  │  │  PID     │  │  Kernel  │                        │   │
│   │   └───────────┘  └──────────┘  └──────────┘                        │   │
│   │                                                                     │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Stack Tecnologico

| Componente | Tecnologia | Justificativa |
|------------|------------|---------------|
| **Linguagem** | Rust (Edition 2021) | Performance, segurança de memória, zero-cost abstractions |
| **Async Runtime** | Tokio | Padrão da indústria para async Rust |
| **RPC Framework** | gRPC (tonic 0.10) | Alta performance, streaming bidirecional |
| **Protocol Buffers** | prost 0.12 | Serialização eficiente, tipagem forte |
| **TUI Framework** | tui-rs 0.19 + crossterm | Cross-platform, rico em widgets |
| **System Info** | sysinfo 0.29 | Cross-platform, APIs unificadas |
| **Auth** | Ed25519 + Tokens | Simples, seguro, offline-first |
| **Config** | TOML + serde | Human-readable, type-safe |
| **Logging** | tracing 0.1 | Estruturado, async-friendly |
| **CLI** | clap 4.0 | Derive macros, shell completion |

---

## Estrutura do Projeto

```
code-monitor/
├── Cargo.toml              # Workspace configuration
├── Cargo.lock
│
├── shared/                  # Biblioteca compartilhada
│   ├── Cargo.toml
│   ├── build.rs            # Compila Protocol Buffers
│   ├── src/
│   │   ├── lib.rs          # Tipos: SystemInfo, ProcessInfo, etc.
│   │   └── proto/
│   │       └── monitoring.proto  # Definição gRPC
│
├── server/                  # Agente de monitoramento
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs         # Entry point, CLI
│   │   ├── config.rs       # Configuração TOML
│   │   ├── monitor.rs      # Coleta de métricas
│   │   ├── service.rs      # Implementação gRPC
│   │   └── auth.rs         # Autenticação
│
├── client/                  # Dashboard TUI
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs         # Entry point
│   │   ├── cli.rs          # Comandos CLI
│   │   ├── config.rs       # Gerenciamento de servidores
│   │   ├── client.rs       # Cliente gRPC
│   │   ├── dashboard.rs    # TUI principal (~2200 linhas)
│   │   ├── ui.rs           # Componentes UI
│   │   └── auth.rs         # Auth client-side
│
├── config.toml             # Config do servidor
├── client-config.toml      # Config do cliente
│
├── docs/                   # Documentação usuário
├── docsx/                  # Documentação estratégica (esta)
└── README.md               # Getting started
```

---

## Comunicacao gRPC

### Servico Definido

```protobuf
// shared/src/proto/monitoring.proto

service MonitorService {
    // Snapshot único
    rpc GetSystemInfo(Empty) returns (SystemInfo);
    rpc GetProcesses(ProcessFilter) returns (ProcessList);
    rpc GetServices(Empty) returns (ServiceList);
    rpc GetNetworkInfo(Empty) returns (NetworkInfo);

    // Streaming em tempo real
    rpc StreamSystemUpdates(StreamRequest) returns (stream SystemUpdate);

    // Autenticação
    rpc Authenticate(AuthRequest) returns (AuthResponse);
}
```

### Fluxo de Dados

```
┌──────────────┐                              ┌──────────────┐
│    Client    │                              │    Server    │
└──────────────┘                              └──────────────┘
       │                                              │
       │  1. Authenticate(token)                      │
       │─────────────────────────────────────────────►│
       │                                              │
       │  2. AuthResponse(success)                    │
       │◄─────────────────────────────────────────────│
       │                                              │
       │  3. StreamSystemUpdates(interval=5s)         │
       │─────────────────────────────────────────────►│
       │                                              │
       │  4. SystemUpdate (CPU, RAM, Disk, Net)       │
       │◄─────────────────────────────────────────────│
       │                                              │
       │  5. SystemUpdate (every 5 seconds)           │
       │◄─────────────────────────────────────────────│
       │                                              │
       │  ... continues streaming ...                  │
       │                                              │
```

---

## Configuracao

### Servidor (config.toml)

```toml
# Intervalo de coleta de métricas
update_interval_seconds = 5

# Limite de clientes simultâneos
max_clients = 100

# Autenticação
enable_authentication = true
access_token = "auto-generated-base64-token"

# Logging
log_level = "info"  # debug, info, warn, error
```

### Cliente (client-config.toml)

```toml
# Configurações gerais
update_interval_seconds = 5
auto_reconnect = true
reconnect_delay_seconds = 5

# Servidores monitorados
[[servers]]
id = "550e8400-e29b-41d4-a716-446655440000"
name = "Production Web"
address = "192.168.1.100"
port = 50051
description = "Servidor web de produção"
access_token = "token-do-servidor"

[[servers]]
id = "550e8400-e29b-41d4-a716-446655440001"
name = "Production DB"
address = "192.168.1.101"
port = 50051
description = "Servidor de banco de dados"
access_token = "outro-token"
```

---

## Uso Basico

### Servidor

```bash
# Iniciar servidor
./monitor-server --address 0.0.0.0 --port 50051

# Ver token de acesso
./monitor-server show-token

# Gerar novo token
./monitor-server new-token

# Desabilitar autenticação (desenvolvimento)
./monitor-server disable-auth
```

### Cliente

```bash
# Abrir dashboard (padrão)
./monitor-client

# Conectar a um servidor rapidamente
./monitor-client connect 192.168.1.100

# Adicionar servidor manualmente
./monitor-client add \
    --name "Meu Servidor" \
    --address 192.168.1.100 \
    --port 50051 \
    --token "token-aqui"

# Listar servidores configurados
./monitor-client list

# Atualizar token
./monitor-client set-token --id <uuid> --token "novo-token"
```

---

## Por Que Rust?

| Aspecto | Benefício |
|---------|-----------|
| **Performance** | Compilado para código nativo, zero overhead |
| **Segurança de memória** | Sem null pointers, sem buffer overflows |
| **Concorrência** | async/await nativo, sem data races |
| **Binário único** | Sem runtime, sem dependências |
| **Cross-platform** | Mesmo código para Linux e Windows |
| **Ecossistema** | Cargo, crates.io, comunidade ativa |

### Comparativo de Recursos

| Ferramenta | Linguagem | CPU Idle | RAM Usage | Startup |
|------------|-----------|----------|-----------|---------|
| htop | C | ~0.5% | ~5MB | Instant |
| **Code Monitor** | **Rust** | **~1.5%** | **~50MB** | **~0.1s** |
| Glances | Python | ~15% | ~450MB | ~3s |
| Netdata | C | ~5% | ~500MB | ~2s |

---

## Proximos Passos

- [Funcionalidades Atuais](./funcionalidades-atuais.md) - O que já temos
- [Diferenciais](./diferenciais.md) - Por que somos melhores
