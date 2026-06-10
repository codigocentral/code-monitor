# API gRPC — Code Monitor

Documentação do serviço gRPC exposto pelo `monitor-server`.

- **Package proto:** `monitoring`
- **Serviço:** `monitoring.MonitorService`
- **Porta padrão:** `50051`
- **Definição:** [`shared/src/proto/monitoring.proto`](../../shared/src/proto/monitoring.proto)
- **Compressão:** gzip habilitado nos dois sentidos (negociado automaticamente)

## Autenticação

Quando `enable_authentication = true` (padrão), toda chamada precisa apresentar
o access token do servidor em um dos dois metadados aceitos:

| Metadado          | Formato                  |
|-------------------|--------------------------|
| `x-access-token`  | `<token>`                |
| `authorization`   | `Bearer <token>`         |

O token é exibido com `monitor-server show-token` e regenerado com
`monitor-server new-token`. Chamadas sem token válido recebem
`UNAUTHENTICATED`.

## RPCs

| RPC                  | Request            | Response               | Tipo            |
|----------------------|--------------------|------------------------|-----------------|
| `GetSystemInfo`      | `google.protobuf.Empty` | `SystemInfoResponse`  | Unary           |
| `GetProcesses`       | `ProcessesRequest` | `ProcessesResponse`    | Unary           |
| `GetServices`        | `google.protobuf.Empty` | `ServicesResponse`    | Unary           |
| `GetNetworkInfo`     | `google.protobuf.Empty` | `NetworkInfoResponse` | Unary           |
| `GetContainers`      | `ContainersRequest`| `ContainersResponse`   | Unary           |
| `GetPostgresInfo`    | `google.protobuf.Empty` | `PostgresInfoResponse`| Unary           |
| `GetMariaDBInfo`     | `google.protobuf.Empty` | `MariaDbInfoResponse` | Unary           |
| `GetSystemdInfo`     | `google.protobuf.Empty` | `SystemdInfoResponse` | Unary           |
| `StreamSystemUpdates`| `UpdatesRequest`   | `stream SystemUpdate`  | Server streaming |

### GetSystemInfo

Retorna hostname, SO, kernel, uptime, CPU (contagem e uso), memória
(total/usada/disponível) e discos (`DiskInfo[]`), com timestamp da última
coleta em background.

### GetProcesses

`ProcessesRequest`:

- `limit` (`uint32`) — máximo de processos retornados (0 = sem limite),
  ordenados por uso de CPU decrescente.
- `filter` (`string`) — busca case-insensitive em nome e linha de comando.
  **Máximo de 256 caracteres**; acima disso a chamada é rejeitada com
  `INVALID_ARGUMENT`.

### StreamSystemUpdates

`UpdatesRequest.update_interval_seconds` define o intervalo entre amostras
(mínimo 1s). O servidor envia `SystemUpdate` continuamente até o cliente
encerrar o stream.

Conexões de streaming contam para o limite `max_clients` da configuração do
servidor; ao exceder, a chamada é rejeitada com `RESOURCE_EXHAUSTED`.

## Códigos de erro

| Código               | Quando ocorre                                            |
|----------------------|----------------------------------------------------------|
| `UNAUTHENTICATED`    | Token ausente, malformado ou inválido                    |
| `INVALID_ARGUMENT`   | Parâmetro inválido (ex.: `filter` > 256 caracteres)      |
| `RESOURCE_EXHAUSTED` | Limite `max_clients` de streams simultâneos atingido     |
| `INTERNAL`           | Falha interna de coleta (detalhes no log do servidor)    |

Todas as requisições têm timeout de 30 segundos no servidor.

## Exemplos com grpcurl

Com o servidor sem TLS (desenvolvimento), usando o proto local:

```bash
# Informações do sistema
grpcurl -plaintext \
  -import-path shared/src/proto -proto monitoring.proto \
  -H "x-access-token: SEU_TOKEN" \
  localhost:50051 monitoring.MonitorService/GetSystemInfo

# Top 10 processos filtrados por "postgres"
grpcurl -plaintext \
  -import-path shared/src/proto -proto monitoring.proto \
  -H "authorization: Bearer SEU_TOKEN" \
  -d '{"limit": 10, "filter": "postgres"}' \
  localhost:50051 monitoring.MonitorService/GetProcesses

# Stream de atualizações a cada 5s
grpcurl -plaintext \
  -import-path shared/src/proto -proto monitoring.proto \
  -H "x-access-token: SEU_TOKEN" \
  -d '{"update_interval_seconds": 5}' \
  localhost:50051 monitoring.MonitorService/StreamSystemUpdates
```

Com TLS habilitado, troque `-plaintext` por `-cacert certs/ca.crt` (e
`-cert`/`-key` se o servidor exigir mTLS).

## TLS / mTLS

O servidor habilita TLS quando a seção `[tls]` do `config.toml` aponta para
`cert_path`/`key_path` válidos; se `ca_path` também for definido, certificados
de cliente passam a ser exigidos (mTLS). Use `./generate-certs.sh` para gerar
certificados de desenvolvimento. Detalhes operacionais no guia de deploy
(`docsx/00-EXECUCAO/04-deployment-plan.md`).
