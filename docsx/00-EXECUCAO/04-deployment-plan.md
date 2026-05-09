# Deployment Plan — Code Monitor nos servidores reais

> Passo a passo de instalação e configuração nos servidores **A6 / A7 / A8**.
> Pré-requisito: ter completado os **Épicos 1-4** do backlog.
> **Sequência sugerida:** A7 → A6 → A8 (do menor risco pro maior).

---

## 0. Pré-requisitos globais

- [ ] Binário `monitor-server` compilado (release, x86_64 linux)
- [ ] Binário `monitor-client` compilado pra workstation do Diogo (Windows)
- [ ] Script `install.sh` testado em VM Ubuntu 22.04
- [ ] Certificados TLS gerados (CA + 3 server certs)
- [ ] Discord webhook URL pronta

---

## 1. Conceito de deploy

### Não-objetivos (deixar claro)

❌ NÃO usar Docker pra rodar o agent — precisa de acesso direto a `/proc`, `/var/run/docker.sock` e ao banco postgres local.
❌ NÃO criar dependência circular: agent **não escreve** no postgres do A6 (que é o que monitora).
❌ NÃO expor porta 50051 na internet — todo tráfego pela VPN `10.10.0.x`.

### Objetivos

✅ Rodar como systemd service nativo
✅ User dedicado `code-monitor` com privs mínimos
✅ Logs em `journalctl -u code-monitor-server`
✅ Self-restart em caso de crash
✅ TLS habilitado mesmo entre IPs internos

---

## 2. A7 (10.10.0.1) — primeiro deploy

A7 é o mais saudável (load 0.39, RAM folgada). Risco mais baixo. Bom pra validar o procedimento.

### 2.1 Instalação

```bash
# SSH como admin5
ssh admin5@10.10.0.1

# Baixar/copiar binário e install.sh
sudo curl -L -o /tmp/monitor-server \
  https://github.com/diogo/code-monitor/releases/download/v0.2.0/monitor-server-linux-x86_64
sudo curl -L -o /tmp/install.sh \
  https://raw.githubusercontent.com/diogo/code-monitor/main/install.sh
sudo chmod +x /tmp/install.sh /tmp/monitor-server

# Rodar install
sudo /tmp/install.sh \
  --binary /tmp/monitor-server \
  --bind 10.10.0.1 \
  --port 50051 \
  --enable-tls
```

O script deve:
1. Criar user `code-monitor` (system, no login)
2. Criar diretórios:
   - `/etc/code-monitor/` (config + certs) — owner `code-monitor:code-monitor`, mode 0750
   - `/var/lib/code-monitor/` (sqlite) — mode 0750
   - `/var/log/code-monitor/` (overflow logs)
3. Mover binário pra `/usr/local/bin/monitor-server`
4. Gerar config inicial em `/etc/code-monitor/config.toml`
5. Gerar certs CA + server em `/etc/code-monitor/certs/`
6. Criar systemd unit `/etc/systemd/system/code-monitor-server.service`
7. `systemctl enable --now code-monitor-server`

### 2.2 systemd unit (template)

```ini
[Unit]
Description=Code Monitor Server
After=network-online.target docker.service
Wants=network-online.target

[Service]
Type=simple
User=code-monitor
Group=code-monitor
SupplementaryGroups=docker

ExecStart=/usr/local/bin/monitor-server \
  --address 10.10.0.1 \
  --port 50051 \
  --health-port 8080 \
  --config /etc/code-monitor/config.toml

Restart=on-failure
RestartSec=5
TimeoutStopSec=30

# Hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/code-monitor /var/log/code-monitor
ReadOnlyPaths=/etc/code-monitor

# Recursos
MemoryMax=200M
CPUQuota=20%
TasksMax=64

# Logs
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

### 2.3 Validação

```bash
# Status
systemctl status code-monitor-server

# Logs
journalctl -u code-monitor-server -f

# Health check
curl http://10.10.0.1:8080/health
# {"status":"healthy","version":"0.2.0",...}

# Token (anote pra config do cliente)
sudo -u code-monitor monitor-server show-token --config /etc/code-monitor/config.toml
```

### 2.4 Adicionar no cliente

```bash
# No workstation do Diogo
monitor-client add \
  --name "A7 Ferramentas" \
  --address 10.10.0.1 \
  --port 50051 \
  --token "<token-de-cima>"

monitor-client
# Tab "A7" deve aparecer com status connected
```

---

## 3. A6 (10.10.0.6) — depois do A7 ok

A6 é mais delicado: postgres ativo, swap em uso, gitlab perto do cap. **Atenção extra à pegada do agent.**

### 3.1 Diferencial: usuário postgres pro coletor

**Antes** de instalar o agent:

```bash
ssh admin5@10.10.0.6
sudo -u postgres psql -p 5433 <<EOF
CREATE USER "code-monitor" WITH LOGIN;
GRANT pg_monitor TO "code-monitor";
EOF

# Configurar peer auth em pg_hba.conf
echo "local all code-monitor peer" | sudo tee -a /etc/postgresql/18/dev/pg_hba.conf
sudo systemctl reload postgresql@18-dev
```

### 3.2 Instalação

```bash
sudo /tmp/install.sh \
  --binary /tmp/monitor-server \
  --bind 10.10.0.6 \
  --port 50051 \
  --enable-tls \
  --enable-postgres \
  --postgres-cluster "dev:/var/run/postgresql:5433"
```

### 3.3 Config específico A6

`/etc/code-monitor/config.toml` — chaves principais:

```toml
[performance]
max_history_in_memory = 60   # 5min
gc_interval_seconds = 300

[collectors.system]
enabled = true

[collectors.docker]
enabled = true

[[collectors.postgres]]
name = "dev"
host = "/var/run/postgresql"   # socket
port = 5433
user = "code-monitor"
application_name = "code-monitor-agent"
collect_pg_stat_statements = true
top_queries = 5
collect_interval_seconds = 60   # mais lento que system

[collectors.systemd]
enabled = true
units = ["postgresql@18-dev.service", "docker.service"]
```

### 3.4 Validação

```bash
# Confirmar que conexão postgres funciona
journalctl -u code-monitor-server | grep -i postgres
# Esperar: "Postgres collector started for cluster 'dev' (10 databases discovered)"

# Cliente ver:
# Tab Postgres → deve listar paraiso, codemail, x3rpt0, gitlab, sonar, etc.
```

### 3.5 Pegada esperada

Validar com:
```bash
systemctl status code-monitor-server
# deve mostrar Memory: <120M, CPU: <2%
```

Se ultrapassar, ajustar `MemoryMax=` e `CPUQuota=` na unit + investigar coletor.

---

## 4. A8 (10.10.0.8) — produção, último

A8 é o mais crítico (produção). Já tem RAM folgada e load saudável.

### 4.1 Pré-requisitos

```bash
ssh admin5@10.10.0.8

# Postgres dual cluster
for port in 5432 5433; do
  sudo -u postgres psql -p $port <<EOF
CREATE USER "code-monitor" WITH LOGIN;
GRANT pg_monitor TO "code-monitor";
EOF
  echo "local all code-monitor peer" | sudo tee -a /etc/postgresql/18/$([ $port = 5432 ] && echo main || echo prod)/pg_hba.conf
  sudo systemctl reload postgresql@18-$([ $port = 5432 ] && echo main || echo prod)
done

# MariaDB (mautic_db) — usuário read-only
docker exec mautic_db mysql -uroot -p"$ROOT_PASS" <<EOF
CREATE USER 'monitor'@'172.17.0.1' IDENTIFIED BY '<senha-forte>';
GRANT PROCESS, REPLICATION CLIENT, SELECT ON *.* TO 'monitor'@'172.17.0.1';
FLUSH PRIVILEGES;
EOF
# Anotar senha em /etc/code-monitor/secrets.env (chmod 600)
```

### 4.2 Instalação

```bash
sudo /tmp/install.sh \
  --binary /tmp/monitor-server \
  --bind 10.10.0.8 \
  --port 50051 \
  --enable-tls \
  --enable-postgres \
  --postgres-cluster "main:/var/run/postgresql:5432" \
  --postgres-cluster "prod:/var/run/postgresql:5433" \
  --enable-mariadb \
  --mariadb-host "172.17.0.1:3306"   # bridge docker
```

### 4.3 Validação

- Tab "Containers" no cliente deve mostrar 41 containers
- Tab "Postgres" mostra `main` e `prod` clusters
- Tab "MariaDB" mostra mautic schema (4.3 GB)
- Health: `curl http://10.10.0.8:8080/health`

### 4.4 Configurar alertas reais

Após 24h de coleta sem incidentes, adicionar regras no `client-config.toml` do Diogo (lista em `02-aplicacao-infra.md` seção 4).

---

## 5. Backup e rollback

### Backup antes de instalar

```bash
# Em cada host:
sudo tar czf /root/backups/pre-codemonitor-$(date +%Y%m%d).tgz \
  /etc/postgresql /etc/systemd/system /var/spool/cron 2>/dev/null
```

### Rollback (se precisar desinstalar)

```bash
sudo systemctl disable --now code-monitor-server
sudo rm /etc/systemd/system/code-monitor-server.service
sudo rm -rf /etc/code-monitor /var/lib/code-monitor /var/log/code-monitor
sudo userdel code-monitor
sudo rm /usr/local/bin/monitor-server

# Reverter pg_hba.conf (procurar linha "local all code-monitor peer" e remover)
# Reload postgres
```

---

## 6. Troubleshooting esperado

### "Failed to connect to docker socket"

Causa: user `code-monitor` não está no group `docker`.
Fix:
```bash
sudo usermod -aG docker code-monitor
sudo systemctl restart code-monitor-server
```

### "Postgres connection refused"

Causa: socket Unix path errado ou peer auth não configurada.
Fix:
```bash
sudo -u code-monitor psql -p 5433 -d postgres -c "SELECT 1"
# Se falhar: rever pg_hba.conf
```

### "Agent consume too much memory (>200M)"

Causa: provavelmente `top_queries` em pg_stat_statements muito alto, ou histórico em memória inflado.
Fix:
```toml
[performance]
max_history_in_memory = 30  # reduzir
[[collectors.postgres]]
top_queries = 3   # reduzir
```

### "Discord notifications não chegam"

Debug:
```bash
journalctl -u code-monitor-server | grep -i discord
# Esperar "Notification sent to Discord channel '<name>'"
# Se erro 429: rate limit — aumentar cooldown
# Se erro 401: webhook URL inválido
```

---

## 7. Checklist final pós-deploy

- [ ] 3 agents rodando (A6, A7, A8) com `systemctl status` verde
- [ ] 3 hosts visíveis no TUI do cliente
- [ ] Tab Containers funcionando (≥80 containers somados)
- [ ] Tab Postgres mostrando 3 clusters (1 dev + 2 prod)
- [ ] Tab MariaDB mostrando mautic_db
- [ ] 1 alerta de teste disparado e recebido no Discord
- [ ] Pegada do agent <150MB RSS, <2% CPU em cada host
- [ ] Logs sem ERROR persistente em 24h
- [ ] Documentar token de cada host em local seguro (1Password ou similar)

---

## 8. Operação no dia-a-dia

### Atualizar agent

```bash
# Em cada host
sudo curl -L -o /tmp/monitor-server.new \
  https://github.com/.../monitor-server-linux-x86_64
sudo install -m 755 /tmp/monitor-server.new /usr/local/bin/monitor-server
sudo systemctl restart code-monitor-server
```

### Rotação de token

```bash
sudo -u code-monitor monitor-server new-token --config /etc/code-monitor/config.toml
sudo systemctl restart code-monitor-server

# Atualizar no cliente:
monitor-client set-token --id <uuid> --token "<novo-token>"
```

### Métricas Prometheus (se quiser alimentar Grafana do A7)

```yaml
# prometheus.yml
- job_name: 'code-monitor'
  static_configs:
    - targets: ['10.10.0.6:8080', '10.10.0.1:8080', '10.10.0.8:8080']
```

⚠️ Pré-requisito: EP6 (expor métricas reais em /metrics, hoje só tem build_info+uptime)
