#!/usr/bin/env bash
#
# Grant monitor-server read-only access to a MariaDB/MySQL running in a Docker
# container.
#
# Runs on the target host as root (via sudo). Unlike Postgres there is no peer
# auth here: the agent reaches the container over TCP on the Docker bridge, so
# a user with a password is required. The script generates that password
# itself — it is never typed and never leaves the host — and reads the root
# password straight from the container's own environment, so no secret passes
# through the operator.
#
#   sudo bash enable-mariadb.sh [container-name]   # default: mautic_db
#
# Idempotent: re-running resets the monitor user's password and rewrites only
# the mariadb block of the config, leaving the token and Postgres clusters
# untouched.

set -euo pipefail

CONTAINER="${1:-mautic_db}"
SERVICE_USER="code-monitor"
CONF="/etc/code-monitor/config.toml"
MONITOR_USER="monitor"
BEGIN="# >>> mariadb (managed by enable-mariadb.sh)"
END="# <<< mariadb (managed by enable-mariadb.sh)"

if [ "$(id -u)" -ne 0 ]; then
    echo "error: must run as root (use sudo)" >&2
    exit 1
fi
if [ ! -f "$CONF" ]; then
    echo "error: $CONF not found; run install-remote.sh first" >&2
    exit 1
fi
if ! docker inspect "$CONTAINER" >/dev/null 2>&1; then
    echo "error: container '$CONTAINER' not found" >&2
    exit 1
fi

# Pick the mysql client the image actually ships (mariadb vs mysql)
CLIENT="mariadb"
docker exec "$CONTAINER" sh -c 'command -v mariadb' >/dev/null 2>&1 || CLIENT="mysql"

echo "==> reading container facts"
ROOT_PW="$(docker inspect "$CONTAINER" --format '{{range .Config.Env}}{{println .}}{{end}}' \
    | sed -n 's/^\(MYSQL_ROOT_PASSWORD\|MARIADB_ROOT_PASSWORD\)=//p' | head -1)"
if [ -z "$ROOT_PW" ]; then
    echo "error: no root password in the container environment; cannot provision" >&2
    exit 1
fi
CONTAINER_IP="$(docker inspect "$CONTAINER" \
    --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' | head -1)"
if [ -z "$CONTAINER_IP" ]; then
    echo "error: could not determine container IP" >&2
    exit 1
fi
# The agent connects from the bridge gateway, so scope the grant to that subnet
SUBNET="$(echo "$CONTAINER_IP" | cut -d. -f1,2).%"
echo "    container $CONTAINER at $CONTAINER_IP, granting to '$MONITOR_USER'@'$SUBNET'"

echo "==> provisioning read-only user"
MON_PW="$(openssl rand -base64 24 2>/dev/null || head -c18 /dev/urandom | base64)"
docker exec -i "$CONTAINER" "$CLIENT" -uroot -p"$ROOT_PW" <<SQL
CREATE USER IF NOT EXISTS '$MONITOR_USER'@'$SUBNET' IDENTIFIED BY '$MON_PW';
ALTER USER '$MONITOR_USER'@'$SUBNET' IDENTIFIED BY '$MON_PW';
GRANT PROCESS, REPLICATION CLIENT, SELECT ON *.* TO '$MONITOR_USER'@'$SUBNET';
FLUSH PRIVILEGES;
SQL
echo "    user ready"

echo "==> updating agent config"
# Drop any previous managed block, then append a fresh one. This leaves the
# token and the Postgres clusters exactly as they are.
if grep -qF "$BEGIN" "$CONF"; then
    sed -i "/^${BEGIN}$/,/^${END}$/d" "$CONF"
fi
TMP="$(mktemp)"
cp "$CONF" "$TMP"
cat >> "$TMP" <<EOF
$BEGIN
[[mariadb_clusters]]
name = "$CONTAINER"
host = "$CONTAINER_IP"
port = 3306
user = "$MONITOR_USER"
password = "$MON_PW"
enabled = true
$END
EOF
install -o "$SERVICE_USER" -g "$SERVICE_USER" -m 0640 "$TMP" "$CONF"
rm -f "$TMP"

echo "==> restart"
systemctl restart code-monitor-server
sleep 2
systemctl is-active --quiet code-monitor-server \
    && echo "    active" \
    || { echo "error: service failed; run: journalctl -u code-monitor-server -n40" >&2; exit 1; }

echo "==> done: MariaDB '$CONTAINER' enabled"
echo "    verify with: journalctl -u code-monitor-server -n20 | grep -i maria"
