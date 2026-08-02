#!/usr/bin/env bash
#
# Publica o conteúdo público do Code Monitor no repositório do GitHub.
#
# Este repositório (GitLab, privado) contém material que não pode ir a público:
# guias de sessão, infraestrutura real, estratégia comercial e o documento de
# requisitos original. A publicação é uma CÓPIA FILTRADA da árvore atual.
#
# NÃO use `git push --mirror` nem adicione um remote do GitHub para dar push
# direto: espelho leva tudo, inclusive o histórico e os arquivos internos.
#
# Uso:
#   scripts/publish-to-github.sh           # prepara e mostra o que iria (não publica)
#   scripts/publish-to-github.sh --push    # prepara e publica
#
# Variáveis de ambiente:
#   GITHUB_REMOTE    destino (default: git@github.com:codigocentral/code-monitor.git)
#   PUBLISH_BRANCH   branch de destino (default: main)

set -euo pipefail

GITHUB_REMOTE=${GITHUB_REMOTE:-git@github.com:codigocentral/code-monitor.git}
PUBLISH_BRANCH=${PUBLISH_BRANCH:-main}

DO_PUSH=false
if [[ ${1:-} == "--push" ]]; then
    DO_PUSH=true
elif [[ -n ${1:-} ]]; then
    echo "uso: $0 [--push]" >&2
    exit 2
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT"

if [[ ! -f .publishignore ]]; then
    echo "erro: .publishignore não encontrado na raiz do repositório." >&2
    exit 1
fi

if [[ -n $(git status --porcelain) ]]; then
    echo "erro: árvore de trabalho suja — commite ou guarde as mudanças antes de publicar." >&2
    exit 1
fi

STAGE=$(mktemp -d)
WORK=$(mktemp -d)
trap 'rm -rf "$STAGE" "$WORK"' EXIT

# ---------------------------------------------------------------------------
# 1. Extrai apenas os arquivos rastreados do HEAD (sem .git, sem lixo local)
# ---------------------------------------------------------------------------
git archive HEAD | tar -x -C "$STAGE"

# ---------------------------------------------------------------------------
# 2. Remove o que o .publishignore marcar como interno
# ---------------------------------------------------------------------------
removed=0
while IFS= read -r line || [[ -n $line ]]; do
    pattern=${line%%#*}
    pattern=$(printf '%s' "$pattern" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')
    [[ -z $pattern ]] && continue

    while IFS= read -r hit; do
        rm -rf "$hit"
        echo "  excluído: ${hit#"$STAGE"/}"
        removed=$((removed + 1))
    done < <(find "$STAGE" -path "$STAGE/$pattern" -prune 2>/dev/null)
done < .publishignore

echo "$removed caminho(s) removido(s) da publicação."

# ---------------------------------------------------------------------------
# 3. Guarda de segurança — aborta se sobrou dado sensível
#
# Roda sobre o resultado JÁ filtrado. Se disparar, corrija o arquivo apontado
# ou acrescente o caminho ao .publishignore. Não há flag para ignorar: se
# houvesse, um dia alguém a usaria com pressa.
#
# Isto NÃO é um scanner de segredos de propósito geral — para isso use gitleaks
# ou o secret detection do GitLab. Aqui o alvo é estreito de propósito: os
# marcadores da infraestrutura real, que são o que este repo não pode vazar.
# ---------------------------------------------------------------------------
abort_scan() {
    echo >&2
    echo "ABORTADO: conteúdo sensível encontrado no material que seria publicado." >&2
    echo >&2
    printf '%s\n' "${1//$STAGE\//}" >&2
    echo >&2
    echo "Corrija o arquivo ou acrescente o caminho ao .publishignore." >&2
    exit 1
}

# 3a. Marcadores de infraestrutura real — vale para qualquer arquivo.
INFRA='10\.10\.0\.[0-9]|alemanha[0-9]|BEGIN [A-Z ]*PRIVATE KEY'
hits=$(grep -rInE "$INFRA" "$STAGE" 2>/dev/null || true)
if [[ -n $hits ]]; then
    abort_scan "$hits"
fi

# 3b. Credencial com valor real em documentação e configuração.
#
#     Fica fora do código fonte porque `password: None` e `Some("secret")` em
#     teste unitário são legítimos e inundariam a saída de falso positivo.
#
#     O valor precisa parecer um segredo de verdade: 16+ caracteres com dígito
#     e maiúscula — a assinatura de base64/hex. Isso deixa passar placeholders
#     em kebab-case ("token-from-server", "your-token-here") sem precisar de uma
#     lista de exceções que alguém teria de manter para sempre.
CRED='(senha|password|passwd|token|secret|api_?key)[[:space:]]*[=:][[:space:]]*"?[[:alnum:]]'
#     O teste roda só sobre o valor: o prefixo `arquivo:linha:` do grep tem
#     dígitos e maiúsculas próprios e faria tudo casar.
hits=$(grep -rInE "$CRED" \
        --include='*.md' --include='*.toml' --include='*.yml' --include='*.yaml' \
        --include='*.sh' --include='*.ps1' --include='*.env*' \
        "$STAGE" 2>/dev/null \
        | awk '{
              line = $0
              sub(/^[^:]*:[0-9]+:/, "", line)
              if (match(line, /[=:][ \t]*"?[A-Za-z0-9+\/=_-]{16,}/)) {
                  val = substr(line, RSTART, RLENGTH)
                  if (val ~ /[A-Z]/ && val ~ /[0-9]/) print $0
              }
          }' || true)
if [[ -n $hits ]]; then
    abort_scan "$hits"
fi

echo "Varredura de segurança: limpa."

# ---------------------------------------------------------------------------
# 4. Sincroniza com o repositório público
# ---------------------------------------------------------------------------
if ! git clone --quiet --depth 1 --branch "$PUBLISH_BRANCH" "$GITHUB_REMOTE" "$WORK" 2>/dev/null; then
    echo "aviso: não foi possível clonar $GITHUB_REMOTE ($PUBLISH_BRANCH) — iniciando repositório novo."
    git init --quiet --initial-branch="$PUBLISH_BRANCH" "$WORK"
    git -C "$WORK" remote add origin "$GITHUB_REMOTE"
fi

find "$WORK" -mindepth 1 -maxdepth 1 -not -name .git -exec rm -rf {} +
cp -a "$STAGE"/. "$WORK"/

git -C "$WORK" add -A

if git -C "$WORK" diff --cached --quiet; then
    echo "Nada mudou desde a última publicação."
    exit 0
fi

echo
echo "Mudanças a publicar em $GITHUB_REMOTE ($PUBLISH_BRANCH):"
git -C "$WORK" diff --cached --stat

SRC=$(git rev-parse --short HEAD)
git -C "$WORK" commit --quiet -m "Sync from internal repository ($SRC)"

if [[ $DO_PUSH != true ]]; then
    echo
    echo "Simulação concluída — nada foi enviado."
    echo "Rode novamente com --push para publicar."
    exit 0
fi

git -C "$WORK" push origin "$PUBLISH_BRANCH"
echo "Publicado."
