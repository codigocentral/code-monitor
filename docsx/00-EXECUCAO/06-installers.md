# Instaladores e Atualizacao

## Objetivo

Instalar e atualizar Code Monitor sem compilar nos servidores.

O fluxo oficial e:

1. Criar uma tag `v*`.
2. GitHub Actions gera binarios para Linux, macOS e Windows.
3. Servidores baixam os assets do GitHub Release.
4. O servico local e reiniciado pelo instalador.

## Linux

```bash
curl -sSL https://github.com/codigocentral/code-monitor/releases/latest/download/install.sh | sudo bash
```

Instala:

- `monitor-server` em `/usr/local/bin`;
- config em `/etc/code-monitor/config.toml`;
- dados em `/var/lib/code-monitor`;
- logs em `/var/log/code-monitor`;
- servico `systemd` chamado `code-monitor-server`.

Atualizacao:

```bash
sudo code-monitor-update
```

## macOS

```bash
curl -sSL https://github.com/codigocentral/code-monitor/releases/latest/download/install.sh | sudo bash
```

Instala:

- binarios em `/usr/local/bin`;
- config em `/usr/local/etc/code-monitor/config.toml`;
- logs em `/usr/local/var/log/code-monitor`;
- LaunchDaemon `com.codemonitor.server`.

## Windows

Execute em PowerShell como administrador:

```powershell
iwr https://github.com/codigocentral/code-monitor/releases/latest/download/install.ps1 -UseB | iex
```

Instala:

- binarios em `C:\Program Files\CodeMonitor`;
- config em `C:\ProgramData\CodeMonitor`;
- tarefa agendada `CodeMonitorServer` para iniciar no boot.

Atualizacao:

```powershell
powershell -ExecutionPolicy Bypass -File "C:\Program Files\CodeMonitor\code-monitor-update.ps1"
```

## Componentes

Por padrao o instalador instala o `server`.

Para instalar apenas o cliente:

```bash
sudo CODE_MONITOR_COMPONENT=client ./install.sh
```

```powershell
.\install.ps1 -Component client
```

Para instalar ambos:

```bash
sudo CODE_MONITOR_COMPONENT=both ./install.sh
```

```powershell
.\install.ps1 -Component both
```

