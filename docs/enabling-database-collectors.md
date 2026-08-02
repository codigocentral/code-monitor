# Enabling the database collectors

The server ships collecting the host core — system, containers, network,
systemd, memory — without any database access. The Postgres and MariaDB tabs
stay empty until you grant the agent read-only access to each database. This is
a separate, more privileged step on purpose: it touches the authentication
configuration of production databases, so it is done deliberately, after the
core agent is proven to be up and light.

This guide covers the whole path. The three scripts referenced live in
[`scripts/`](../scripts): `install-remote.sh`, `enable-postgres.sh` and
`enable-mariadb.sh`. Each is idempotent — safe to re-run.

## What the agent reads, and what it does not

The database collectors read **statistics and metadata only**, never table
data:

- Postgres: the `pg_monitor` role — `pg_stat_database`, `pg_settings`,
  `pg_stat_statements` if present. Enough for sizes, connections, cache-hit
  ratios, the configuration source of each setting, and signs of life.
- MariaDB/MySQL: `PROCESS`, `REPLICATION CLIENT`, `SELECT` — process list,
  status counters, schema sizes.

Neither role can write. `SELECT` on MariaDB is what lets it read
`information_schema` for schema sizes; it does not imply any change.

## Prerequisite: the core agent

The database steps assume `monitor-server` is already installed as a systemd
service, with `/etc/code-monitor/config.toml` holding the access token. Install
it first:

```bash
# From your workstation, stage the static binary on the host, then:
ssh -t admin@HOST 'sudo bash /tmp/install-remote.sh <host-vpn-ip>'
```

The binary is statically linked (musl), so one build runs on any Linux
distribution regardless of its glibc — Debian, Ubuntu, whatever the host is.

## Step 1 — Discover the topology

Before granting anything, find out what actually runs on the host. This is
read-only:

```bash
pg_lsclusters                                   # Postgres clusters + ports
ss -ltn | grep -E ':(5432|5433|3306)'           # what is listening
docker ps --format '{{.Names}}' | grep -Ei 'maria|mysql'   # databases in Docker
```

A single host often runs more than one Postgres cluster, on different ports
(`5432`, `5433`). Each is a separate instance and needs enabling separately.

## Step 2 — Postgres

```bash
ssh -t admin@HOST 'sudo bash /tmp/enable-postgres.sh'
```

For each online cluster the script:

1. creates a `code-monitor` role with `pg_monitor` (idempotent);
2. rewrites the agent config to collect from each cluster over the Unix socket;
3. restarts the service, preserving the existing token.

### Why the Unix socket and peer auth

The stock Debian/Ubuntu Postgres enables **peer authentication** for local
socket connections: the OS user `code-monitor` maps to the database role of the
same name, with no password. Nothing is set in `postgresql.conf`, no password
lives in any file, and no credential crosses the network. That is why the
script does not touch `pg_hba.conf` — it only needs to create the role.

### One gotcha: the socket port

Over a Unix socket, Postgres names the socket file `.s.PGSQL.<port>`. A cluster
on `5433` reached by socket must still carry its port, or it silently connects
to `5432` instead. The agent handles this correctly, and the config the script
writes always includes the port. It is worth knowing if you ever hand-edit the
config: `host` and `socket_path` both point at `/var/run/postgresql`, and
`port` selects the instance.

## Step 3 — MariaDB / MySQL in Docker

```bash
ssh -t admin@HOST 'sudo bash /tmp/enable-mariadb.sh [container-name]'
```

MariaDB has no peer auth over TCP, so a password is unavoidable. The script:

1. reads the root password from the **container's own environment**
   (`MYSQL_ROOT_PASSWORD`), so no secret has to be typed or passed in;
2. generates a fresh random password for the `monitor` user;
3. creates that user read-only, scoped to the container's bridge subnet;
4. appends a MariaDB block to the config, leaving the token and Postgres
   clusters untouched;
5. restarts the service.

The agent reaches the container by its bridge IP (for example `172.18.0.2`).
If the container is recreated its IP can change — re-run the script, which
rediscovers it.

## Step 4 — Verify

From the host:

```bash
journalctl -u code-monitor-server -n20 | grep -iE 'postgres|maria'
```

From the client, over the VPN, the Postgres and MariaDB tabs should now list
the databases. Two things worth checking, because they are the reason these
collectors exist:

- **Setting source.** On the Postgres tab, a value whose origin is
  `postgresql.auto.conf` is highlighted. `ALTER SYSTEM` writes there and it
  overrides `postgresql.conf`, so this is the value that is actually in effect
  — and the one that misleads anyone debugging from the main config file.
- **Signs of life.** A database with zero transactions and zero connections is
  marked idle: a candidate for removal, holding buffer pool, disk and backup
  for nothing. The tool flags it; whether to drop it is a person's call.

## Firewall

`install-remote.sh` opens the gRPC port (`50051`) on `ufw` for the VPN subnet
only. If the client cannot connect but the service is active and listening,
the firewall is the first place to look — a host with a public interface must
never expose this port wider than the VPN.

## Hosts outside the VPN: a persistent tunnel

A host with no VPN address — a mail server reached only over the public
internet, say — should not expose the gRPC port at all. Install with the bind
address set to loopback:

```bash
ssh -t admin@HOST 'sudo bash /tmp/install-remote.sh 127.0.0.1'
```

The agent then listens only on the host's own loopback, and the client reaches
it through an SSH tunnel. Rather than a hand-run `ssh -L`, keep it up with a
dedicated key and a user service.

Generate a key with no passphrase and authorize it on the host **restricted to
this one forward** — it grants no shell and no other forwarding:

```bash
ssh-keygen -t ed25519 -N '' -f ~/.ssh/code-monitor-tunnel

# On the host, append to ~/.ssh/authorized_keys (one line):
restrict,command="/bin/false",port-forwarding,permitopen="127.0.0.1:50051" ssh-ed25519 AAAA... code-monitor-tunnel
```

Both parts matter: `restrict` drops PTY and every forwarding, then
`port-forwarding` with `permitopen` re-enables just the one destination.
`restrict` alone does **not** stop command execution — `ssh host 'cmd'` would
still run — so `command="/bin/false"` is what actually denies a shell. With
`ssh -N` the forced command is never invoked; the forward still works.

A systemd **user** service keeps it alive and reconnects:

```ini
# ~/.config/systemd/user/code-monitor-tunnel.service
[Unit]
Description=Code Monitor SSH tunnel
After=network-online.target

[Service]
ExecStart=/usr/bin/ssh -N -o BatchMode=yes -o IdentitiesOnly=yes \
  -o ExitOnForwardFailure=yes -o ServerAliveInterval=30 -o ServerAliveCountMax=3 \
  -i %h/.ssh/code-monitor-tunnel -L 50057:127.0.0.1:50051 admin@HOST
Restart=always
RestartSec=10

[Install]
WantedBy=default.target
```

```bash
systemctl --user daemon-reload
systemctl --user enable --now code-monitor-tunnel.service
```

Point the server's client-config entry at `127.0.0.1:50057`. By default a user
service stops when you log out of every session; to keep the tunnel up across
logout and reboot, enable lingering once:

```bash
sudo loginctl enable-linger "$USER"
```

## Rollback

```bash
# Postgres: drop the role on each cluster
sudo -u postgres psql -p PORT -c 'DROP ROLE "code-monitor";'

# MariaDB: drop the monitor user inside the container
docker exec CONTAINER mariadb -uroot -p"$ROOT_PW" \
  -e "DROP USER 'monitor'@'SUBNET';"

# Then remove the cluster blocks from /etc/code-monitor/config.toml and
# restart. To remove the agent entirely, see install-remote.sh.
```
