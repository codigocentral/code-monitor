# Criando um Repositório APT para seu Sistema

Este guia explica como criar um repositório APT para que seu sistema possa ser instalado diretamente via `apt install` no Linux.

## Índice

1. [Criar Pacotes .deb](#1-criar-pacotes-deb)
2. [Criar Scripts de Instalação](#2-criar-scripts-de-instalação)
3. [Criar Arquivo de Serviço Systemd](#3-criar-arquivo-de-serviço-systemd)
4. [Instalação Interativa com Debconf](#4-instalação-interativa-com-debconf)
5. [Criar o Repositório APT](#5-criar-o-repositório-apt)
6. [Configurar no Servidor Web](#6-configurar-no-servidor-web-nginx)
7. [Como Usuários Instalam](#7-como-usuários-instalam)

---

## 1. Criar Pacotes .deb

Primeiro, você precisa empacotar seu projeto Rust como `.deb`. Use o `cargo-deb`:

```bash
cargo install cargo-deb
```

Adicione configuração no `Cargo.toml` do server e client:

```toml
[package.metadata.deb]
maintainer = "Seu Nome <email@exemplo.com>"
copyright = "2025, Seu Nome"
license-file = ["LICENSE", "0"]
extended-description = "Sistema de monitoramento de código"
depends = "$auto"
section = "utility"
priority = "optional"
assets = [
    ["target/release/monitor-server", "usr/bin/", "755"],
    ["server.service", "lib/systemd/system/", "644"],
]
maintainer-scripts = "debian/"
systemd-units = { unit-name = "monitor-server", enable = false }
```

---

## 2. Criar Scripts de Instalação

Crie a pasta `debian/` com os scripts:

### `debian/postinst` (executado após instalação)

```bash
#!/bin/bash
set -e

case "$1" in
    configure)
        # Criar usuário do sistema se não existir
        if ! getent passwd monitor > /dev/null; then
            useradd --system --no-create-home --shell /usr/sbin/nologin monitor
        fi
        
        # Criar diretórios necessários
        mkdir -p /etc/monitor-server
        mkdir -p /var/log/monitor-server
        chown monitor:monitor /var/log/monitor-server
        
        # Habilitar e iniciar o serviço automaticamente
        systemctl daemon-reload
        systemctl enable monitor-server.service
        systemctl start monitor-server.service
        ;;
esac

exit 0
```

### `debian/prerm` (executado antes de remover)

```bash
#!/bin/bash
set -e

case "$1" in
    remove|upgrade)
        systemctl stop monitor-server.service || true
        systemctl disable monitor-server.service || true
        ;;
esac

exit 0
```

### `debian/postrm` (executado após remover)

```bash
#!/bin/bash
set -e

case "$1" in
    purge)
        # Remover configurações e logs
        rm -rf /etc/monitor-server
        rm -rf /var/log/monitor-server
        # Remover usuário
        userdel monitor 2>/dev/null || true
        ;;
esac

exit 0
```

---

## 3. Criar Arquivo de Serviço Systemd

### `server.service`

```ini
[Unit]
Description=Code Monitor Server
After=network.target

[Service]
Type=simple
User=monitor
Group=monitor
ExecStart=/usr/bin/monitor-server
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

---

## 4. Instalação Interativa com Debconf

Para perguntar ao usuário durante a instalação, use **debconf**:

### `debian/templates`

```
Template: monitor-server/autostart
Type: boolean
Default: true
Description: Iniciar o serviço automaticamente?
 Deseja que o monitor-server seja iniciado automaticamente após a instalação?

Template: monitor-server/enable-boot
Type: boolean
Default: true
Description: Habilitar no boot?
 Deseja que o monitor-server inicie automaticamente quando o sistema ligar?
```

### `debian/config` (script de configuração)

```bash
#!/bin/bash
set -e
. /usr/share/debconf/confmodule

db_input high monitor-server/autostart || true
db_input high monitor-server/enable-boot || true
db_go || true

exit 0
```

### `debian/postinst` (versão interativa)

```bash
#!/bin/bash
set -e
. /usr/share/debconf/confmodule

case "$1" in
    configure)
        # Criar usuário
        if ! getent passwd monitor > /dev/null; then
            useradd --system --no-create-home --shell /usr/sbin/nologin monitor
        fi
        
        mkdir -p /etc/monitor-server
        mkdir -p /var/log/monitor-server
        chown monitor:monitor /var/log/monitor-server
        
        systemctl daemon-reload
        
        # Verificar respostas do usuário
        db_get monitor-server/enable-boot
        if [ "$RET" = "true" ]; then
            systemctl enable monitor-server.service
        fi
        
        db_get monitor-server/autostart
        if [ "$RET" = "true" ]; then
            systemctl start monitor-server.service
        fi
        ;;
esac

exit 0
```

---

## 5. Criar o Repositório APT

### Estrutura do repositório

```
repo/
├── pool/
│   └── main/
│       └── m/
│           └── monitor-server/
│               └── monitor-server_1.0.0_amd64.deb
├── dists/
│   └── stable/
│       ├── Release
│       ├── Release.gpg
│       ├── InRelease
│       └── main/
│           └── binary-amd64/
│               ├── Packages
│               ├── Packages.gz
│               └── Release
```

### Script para gerar o repositório

```bash
#!/bin/bash
REPO_DIR="/var/www/apt-repo"
DIST="stable"
COMPONENT="main"
ARCH="amd64"

cd $REPO_DIR

# Gerar Packages
dpkg-scanpackages --arch $ARCH pool/ > dists/$DIST/$COMPONENT/binary-$ARCH/Packages
gzip -k -f dists/$DIST/$COMPONENT/binary-$ARCH/Packages

# Gerar Release
apt-ftparchive release dists/$DIST > dists/$DIST/Release

# Assinar (necessário chave GPG)
gpg --default-key "sua-chave@email.com" -abs -o dists/$DIST/Release.gpg dists/$DIST/Release
gpg --default-key "sua-chave@email.com" --clearsign -o dists/$DIST/InRelease dists/$DIST/Release
```

---

## 6. Configurar no Servidor Web (Nginx)

```nginx
server {
    listen 80;
    server_name apt.seudominio.com;
    
    root /var/www/apt-repo;
    
    location / {
        autoindex on;
    }
}
```

---

## 7. Como Usuários Instalam

Os usuários adicionam seu repositório:

```bash
# Adicionar chave GPG
curl -fsSL https://apt.seudominio.com/KEY.gpg | sudo gpg --dearmor -o /usr/share/keyrings/monitor-archive-keyring.gpg

# Adicionar repositório
echo "deb [signed-by=/usr/share/keyrings/monitor-archive-keyring.gpg] https://apt.seudominio.com stable main" | sudo tee /etc/apt/sources.list.d/monitor.list

# Instalar
sudo apt update
sudo apt install monitor-server
```

---

## Resumo das Opções de Instalação

| Opção | Comportamento |
|-------|---------------|
| **Automático** | postinst faz `systemctl enable` e `start` direto |
| **Interativo** | Usa debconf para perguntar ao usuário |
| **Manual** | Não faz nada, usuário controla manualmente |

---

## Ferramentas Úteis

- **cargo-deb**: Gera pacotes .deb a partir de projetos Rust
- **dpkg-scanpackages**: Gera arquivo Packages do repositório
- **apt-ftparchive**: Gera arquivos Release
- **gpg**: Assina o repositório para segurança

---

## Referências

- [Debian Policy Manual](https://www.debian.org/doc/debian-policy/)
- [cargo-deb Documentation](https://github.com/kornelski/cargo-deb)
- [Debconf Specification](https://www.debian.org/doc/debian-policy/ch-binary.html#prompting-in-maintainer-scripts)
