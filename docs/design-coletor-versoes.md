# Design — coletor `versions`

**Estado:** proposta
**Motivação:** hoje não há como responder "o que está desatualizado?" sem entrar host a host e
comparar imagem por imagem na mão. Esse levantamento manual leva horas e envelhece no dia seguinte.

---

## O problema

O `docker` collector já sabe **o que está rodando** (nome, imagem, estado, saúde, memória,
restarts). O que falta é a outra metade: **o que existe lá fora**.

Comparar número de versão não resolve, por dois motivos:

1. A maioria das implantações usa tag flutuante (`latest`, `stable`, `community`). O número não
   muda mesmo quando a imagem muda — um `latest` puxado há seis meses continua se chamando `latest`.
2. Tag fixa não garante que seja a mais recente. `v1.8.2` pode estar imutável e correta no registry,
   e mesmo assim ser cinco versões atrás.

Portanto o coletor precisa responder a **duas perguntas distintas**, e não confundi-las:

| Pergunta | Método | Campo |
|---|---|---|
| A imagem que rodo ainda é o que essa tag aponta? | comparar digest local vs. remoto | `digest_drift` |
| Existe versão mais nova que a minha? | listar tags e ordenar por versão | `newer_version` |

A primeira é barata e exata. A segunda é heurística (exige entender o esquema de tag do projeto)
e deve ser tratada como sugestão, não verdade.

---

## Escopo da primeira versão

Só `digest_drift`. É determinística, cobre o caso mais comum (tag flutuante defasada) e não exige
conhecer a convenção de nomes de cada projeto. `newer_version` fica para uma segunda etapa.

---

## Como funciona

Para cada container em execução:

1. Ler a referência da imagem (já disponível no `docker` collector).
2. Obter o digest local — `inspect_image` → `RepoDigests[0]`, a parte após `@`.
   Imagem construída localmente não tem `RepoDigests`: marcar como `LocalBuild` e não consultar rede.
3. Resolver o registry a partir da referência:
   - sem host e sem `/` → Docker Hub, repositório `library/<nome>`
   - sem host e com `/` → Docker Hub, repositório como veio
   - primeiro segmento contendo `.` ou `:` → é o host do registry
4. Autenticar (anônimo) e fazer **`HEAD`** no manifesto da tag, com `Accept` cobrindo índice OCI,
   lista de manifestos Docker e manifesto simples.
5. Ler o cabeçalho `Docker-Content-Digest` e comparar com o local.

`HEAD` em vez de `GET`: o digest vem no cabeçalho, o corpo não é necessário, e o tráfego cai muito.

### Estados

| Estado | Significado |
|---|---|
| `UpToDate` | digest local == remoto |
| `Drifted` | diferentes — existe imagem nova sob a mesma tag |
| `LocalBuild` | sem digest de registry; construída no host |
| `Unknown` | registry inacessível, sem autenticação, ou tag removida |

`Unknown` **não** é erro: registry privado, rede caída e rate limit são situações normais. O
coletor precisa degradar em silêncio e manter o último resultado conhecido com carimbo de tempo.

---

## Cadência e rate limit

Este coletor é diferente de todos os outros do projeto: **depende de rede externa** e os demais
rodam a cada 5 s. Aqui isso seria destrutivo.

O Docker Hub limita requisições anônimas a manifesto por IP, e `HEAD` conta no limite. Com algumas
dezenas de imagens distintas por host, e vários hosts saindo pelo mesmo IP público, uma cadência
agressiva bate no teto e devolve `429` para todo mundo.

Regras:

- **Intervalo padrão: 24 h**, configurável, com mínimo aceito de 1 h.
- **Deduplicar por referência de imagem** antes de consultar: 6 hosts com o mesmo `node-exporter`
  são **uma** consulta, não seis.
- **Cache com TTL** por referência, persistido, para sobreviver a restart do serviço.
- **Espaçar** as consultas em vez de disparar todas juntas.
- Respeitar `Retry-After` no `429` e não tentar de novo na mesma janela.
- Suportar credencial opcional por registry (aumenta o limite e permite registry privado).

## Configuração

```toml
[collectors.versions]
enabled = true
interval_hours = 24
timeout_seconds = 20
# Referências que o coletor ignora (registry interno, imagem de CI).
skip_patterns = ["registry.interno.exemplo/"]

# Credencial opcional por registry — eleva o rate limit e habilita repositório privado.
[[collectors.versions.registry_auth]]
registry = "registry-1.docker.io"
username_env = "DOCKERHUB_USER"
password_env = "DOCKERHUB_TOKEN"
```

Credencial **só por variável de ambiente**, nunca valor literal no arquivo.

## Saída

Por container, somando ao que o `docker` collector já publica:

```rust
pub struct ImageVersionInfo {
    pub image_ref: String,        // "grafana/grafana:latest"
    pub registry: String,         // "registry-1.docker.io"
    pub repository: String,       // "grafana/grafana"
    pub tag: String,              // "latest"
    pub local_digest: Option<String>,
    pub remote_digest: Option<String>,
    pub status: VersionStatus,    // UpToDate | Drifted | LocalBuild | Unknown
    pub checked_at: Option<SystemTime>,
    pub error: Option<String>,    // motivo do Unknown, para diagnóstico
}
```

### Métricas Prometheus

```
code_monitor_image_drift{container,image,registry,repository,tag}  0|1
code_monitor_image_check_timestamp_seconds{image}                  <epoch>
code_monitor_image_check_errors_total{registry,reason}             <contador>
```

`code_monitor_image_drift` como gauge 0/1 permite alerta direto
(`sum(code_monitor_image_drift) > 0`) e um painel de "quantos serviços estão atrasados".

### TUI

Coluna de situação na aba Docker (`·` em dia, `↑` desatualizada, `?` indeterminado) e um resumo no
topo. Sem aba nova: o dado pertence ao container, ao lado de saúde e memória.

---

## Fora de escopo nesta versão

- **Atualizar qualquer coisa.** O coletor só observa. Ação fica com o operador.
- **`newer_version`** (listar tags e ordenar) — exige lidar com convenção de nome por projeto
  (`v1.2.3`, `1.2.3-alpine`, `2026.1-lta`) e erra fácil. Etapa seguinte.
- **CVE por imagem.** É trabalho de scanner (Trivy/Grype), não deste coletor. Pode virar
  integração depois.

---

## Riscos

| Risco | Mitigação |
|---|---|
| Rate limit do registry | dedup + cache + intervalo de 24 h + credencial opcional |
| Registry privado sem credencial | `Unknown` silencioso; `skip_patterns` para não poluir |
| Consulta de rede travando o ciclo | timeout curto, execução fora do caminho dos outros coletores |
| Digest de índice multi-arch vs. manifesto de plataforma | `Accept` cobre os dois; comparar sempre o que `RepoDigests` registrou |
| Falso "em dia" | deixar explícito na UI que `UpToDate` = "a tag não mudou", **não** "é a versão mais nova" |

---

## Aceitação

1. Em host com imagem sabidamente antiga sob tag flutuante, reporta `Drifted`.
2. Logo após `docker pull`, a mesma imagem reporta `UpToDate`.
3. Imagem construída localmente reporta `LocalBuild` e não gera tráfego de rede.
4. Com a rede bloqueada, reporta `Unknown` e **não** derruba nem atrasa os demais coletores.
5. Seis hosts com a mesma imagem produzem **uma** consulta ao registry por ciclo.
6. A métrica aparece em `/metrics` no formato acima.

> Referência de implementação: `code-monitor-ops/scripts/inventario-versoes.sh` faz exatamente
> essa comparação em shell + Python e já rodou contra a frota. Serve de espelho para os testes.
