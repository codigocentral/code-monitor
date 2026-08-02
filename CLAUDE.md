# Code Monitor — guia de sessão

## Como abrir a sessão

Este projeto vive em **3 repositórios irmãos**. Abra sempre a partir daqui, anexando os outros dois:

```bash
cd code-monitor
claude --add-dir ../code-monitor-ops ../code-monitor-clientes
```

Se a sessão já estiver aberta sem eles, use `/add-dir ../code-monitor-ops` e `/add-dir ../code-monitor-clientes`.

Não abra a sessão na pasta-mãe `codigocentral/` — são mais de 120 projetos, o contexto fica poluído e a fronteira entre público e privado se perde.

## O que ler ao entrar

Leia sob demanda, não tudo de uma vez:

| Arquivo | Quando |
|---|---|
| `AGENTS.md` | **sempre** — arquitetura, stack, build, layout dos crates, convenções |
| `PLANO-MESTRE-OPEN-SOURCE.md` | decisão de produto, fases, o que é grátis vs. pago |
| `PLANO-DE-ATIVIDADES.md` | épicos e backlog planejado |
| `docsx/` | estratégia (mercado, posicionamento, roadmap, go-to-market) |
| `CHANGELOG.md` | o que já entrou |

Antes de mexer em coletor, ler a issue correspondente no GitLab (ver abaixo) — cada uma traz o diagnóstico e o critério de aceitação.

## Os 3 repositórios

Todos privados em `gitlab.didaticos.com/codigocentral/`.

| Repo | Papel | Regra |
|---|---|---|
| `code-monitor` (aqui) | O produto. Crates `shared`, `server`, `client`. **É o que vai a público no GitHub.** | Só código, docs genéricos e exemplos |
| `code-monitor-ops` | Operação da infra própria: inventário, playbooks, configs reais, rollout | Nunca recebe código-fonte do produto |
| `code-monitor-clientes` | Dados de cliente e piloto: contatos, configs específicas, histórico | Nunca sai daqui, em hipótese alguma |

## Público vs. interno — como funciona

Este repositório é **privado no GitLab** e é a fonte da versão **pública no GitHub**. As duas coisas não são a mesma árvore: a publicação é uma **cópia filtrada**, feita por `scripts/publish-to-github.sh`, que remove tudo listado em `.publishignore`.

Ou seja: material interno **pode** ser versionado aqui — ele só não atravessa para o GitHub.

**Nunca use `git push --mirror` nem adicione um remote do GitHub para dar push direto.** Espelho leva tudo, inclusive o histórico e os arquivos internos. O único caminho de publicação é o script.

O que hoje fica de fora (ver `.publishignore` para a lista corrente):

- este `CLAUDE.md` e o próprio mecanismo de publicação
- `docsx/` — estratégia comercial, e `00-EXECUCAO/` tem os IPs da VPN e o usuário SSH da frota
- `PLANO-MESTRE-OPEN-SOURCE.md`, `PLANO-DE-ATIVIDADES.md`
- `necessidade.md` — documento de origem, contém credencial em texto plano

Ao criar arquivo novo com conteúdo interno, **acrescente o caminho ao `.publishignore` no mesmo commit**. O script tem uma varredura que aborta a publicação se encontrar IP da VPN, hostname da frota, chave privada ou credencial com valor real — mas ela é a última linha de defesa, não a primeira.

Para issues, commits e comentários de código a regra continua valendo por escrito, porque nenhum script os filtra: o problema técnico genérico fica aqui, a evidência com dados da frota vai para `../code-monitor-ops`.

### Publicar

```bash
scripts/publish-to-github.sh           # simula e mostra o que iria
scripts/publish-to-github.sh --push    # publica
```

## Backlog

As issues estão em **`codigocentral/code-monitor` no GitLab** (`gitlab.didaticos.com`), não no GitHub. Hoje são 12 abertas (#1–#12), rotuladas `auditoria-frota-2026-07`, cobrindo coletores (Postgres, Docker, memória, systemd, TLS, rede), alertas preditivos e rollout.

Ao criar issue nova, escolha o repo pelo conteúdo:

- comportamento do produto, bug de coletor, feature de TUI → **aqui**
- instalação, rollout, inventário, procedimento operacional → **`code-monitor-ops`**
- pedido ou problema de um cliente nomeado → **`code-monitor-clientes`**

**Atenção ao migrar issues para o GitHub:** as issues atuais contêm a seção "Evidência real" com dados da frota. Ao abrir a versão pública, leve apenas Problema / Proposta / Critério de aceitação e deixe a evidência no `code-monitor-ops`.

## Convenções

- Código, comentários, nomes e documentação do produto: **em inglês** (`necessidade.md` é a exceção histórica, é o documento de origem).
- Documentos de estratégia e planejamento: português, como já estão.
- Comandos de build, execução e teste: ver `AGENTS.md` — não reinventar.
- `main` é a branch padrão. `backup/pre-public-history-20260508` guarda o histórico anterior à limpeza para publicação; não fazer merge dela.
