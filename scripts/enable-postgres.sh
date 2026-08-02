#!/usr/bin/env bash
#
# Grant monitor-server read-only access to the Postgres clusters on a host.
#
# Runs on the target host as root (via sudo). It discovers every running
# cluster, creates a code-monitor role with pg_monitor (statistics only, never
# table data), and rewrites the agent config so it collects from each one over
# the Unix socket with peer authentication — no password, nothing on the wire.
#
#   sudo bash enable-postgres.sh
#
# Assumptions, all true of a stock Debian/Ubuntu Postgres:
#   - peer auth is enabled for local socket connections (the default)
#   - the OS service user is code-monitor, created by install-remote.sh
#   - the agent config lives at /etc/code-monitor/config.toml
#
# If a newer /tmp/monitor-server is staged, it is installed first. Idempotent:
# re-running re-grants (harmless) and regenerates the config from the clusters
# currently present, preserving the existing access token.

set -euo pipefail

SERVICE_USER="code-monitor"
CONF="/etc/code-monitor/config.toml"
BIN_DST="/usr/local/bin/monitor-server"
BIN_SRC="/tmp/monitor-server"
SOCKET_DIR="/var/run/postgresql"

if [ "$(id -u)" -ne 0 ]; then
    echo "error: must run as root (use sudo)" >&2
    exit 1
fi
if [ ! -f "$CONF" ]; then
    echo "error: $CONF not found; run install-remote.sh first" >&2
    exit 1
fi

# Optional binary refresh, so a collector fix ships with the same command
if [ -f "$BIN_SRC" ]; then
    echo "==> upgrading binary"
    install -m 0755 "$BIN_SRC" "$BIN_DST"
    rm -f "$BIN_SRC"
fi

echo "==> discovering clusters"
# pg_lsclusters columns: version cluster port status owner datadir logfile
mapfile -t CLUSTERS < <(pg_lsclusters --no-header 2>/dev/null | awk '$4=="online"{print $2":"$3}')
if [ "${#CLUSTERS[@]}" -eq 0 ]; then
    echo "    no online clusters found; nothing to do"
    exit 0
fi
for c in "${CLUSTERS[@]}"; do echo "    ${c%%:*} on port ${c##*:}"; done

echo "==> granting read-only role on each cluster"
for c in "${CLUSTERS[@]}"; do
    port="${c##*:}"
    # Idempotent: create the role only if absent, then ensure the grant
    sudo -u postgres psql -p "$port" -v ON_ERROR_STOP=1 <<SQL
DO \$\$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '$SERVICE_USER') THEN
        CREATE ROLE "$SERVICE_USER" LOGIN;
    END IF;
END
\$\$;
GRANT pg_monitor TO "$SERVICE_USER";
SQL
    echo "    port $port: role ready"
done

echo "==> rewriting agent config"
# Preserve the token; a fresh one would lock out the client
TOKEN="$(grep '^access_token' "$CONF" | head -1 | sed 's/.*=\s*"\(.*\)"/\1/')"
if [ -z "$TOKEN" ]; then
    echo "error: could not read existing access_token from $CONF" >&2
    exit 1
fi

TMP_CONF="$(mktemp)"
{
    echo "# Managed by install-remote.sh + enable-postgres.sh."
    echo "update_interval_seconds = 5"
    echo "max_clients = 100"
    echo "enable_authentication = true"
    echo "log_level = \"info\""
    echo "access_token = \"$TOKEN\""
    echo "systemd_units = []"
    echo ""
    for c in "${CLUSTERS[@]}"; do
        name="${c%%:*}"
        port="${c##*:}"
        echo "[[postgres_clusters]]"
        echo "name = \"$name\""
        echo "host = \"$SOCKET_DIR\""
        echo "port = $port"
        echo "database = \"postgres\""
        echo "user = \"$SERVICE_USER\""
        echo "socket_path = \"$SOCKET_DIR\""
        echo "enabled = true"
        echo ""
    done
} > "$TMP_CONF"
install -o "$SERVICE_USER" -g "$SERVICE_USER" -m 0640 "$TMP_CONF" "$CONF"
rm -f "$TMP_CONF"

echo "==> restart"
systemctl restart code-monitor-server
sleep 2
systemctl is-active --quiet code-monitor-server \
    && echo "    active" \
    || { echo "error: service failed; run: journalctl -u code-monitor-server -n40" >&2; exit 1; }

echo "==> done: ${#CLUSTERS[@]} cluster(s) enabled"
echo "    verify with: journalctl -u code-monitor-server -n20 | grep -i postgres"
