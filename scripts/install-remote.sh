#!/usr/bin/env bash
#
# Install monitor-server on a host as a hardened systemd service.
#
# Runs on the target host as root (via sudo). It expects the static binary to
# already be at /tmp/monitor-server, copied there beforehand.
#
#   sudo bash install-remote.sh <bind-ip> [port]
#
# The bind IP must be the host's VPN address. Binding to 0.0.0.0 would expose
# the metrics of the whole fleet on a public interface — the exact problem the
# network collector was built to catch — so this script refuses it.
#
# Idempotent: re-running upgrades the binary and restarts the service without
# disturbing an existing token, so the client keeps working across upgrades.

set -euo pipefail

BIND_IP="${1:?usage: install-remote.sh <bind-ip> [port]}"
PORT="${2:-50051}"
# The VPN subnet allowed to reach the gRPC port through the firewall
VPN_CIDR="${3:-10.0.0.0/24}"

SERVICE_USER="code-monitor"
BIN_SRC="/tmp/monitor-server"
BIN_DST="/usr/local/bin/monitor-server"
CONF_DIR="/etc/code-monitor"
CONF="$CONF_DIR/config.toml"
DATA_DIR="/var/lib/code-monitor"
UNIT="/etc/systemd/system/code-monitor-server.service"
TOKEN_OUT="/tmp/cm-token.txt"

if [ "$(id -u)" -ne 0 ]; then
    echo "error: must run as root (use sudo)" >&2
    exit 1
fi

case "$BIND_IP" in
    0.0.0.0|::|"")
        echo "error: refusing to bind to all interfaces; pass the host's VPN IP" >&2
        exit 1
        ;;
esac

if [ ! -f "$BIN_SRC" ]; then
    echo "error: $BIN_SRC not found; copy the binary there first" >&2
    exit 1
fi

echo "==> service user"
if ! id "$SERVICE_USER" >/dev/null 2>&1; then
    useradd --system --no-create-home --shell /usr/sbin/nologin "$SERVICE_USER"
    echo "    created $SERVICE_USER"
else
    echo "    $SERVICE_USER already exists"
fi
# Docker metrics need the socket; the group is the least-privilege way in
if getent group docker >/dev/null 2>&1; then
    usermod -aG docker "$SERVICE_USER"
    echo "    added to docker group"
fi

echo "==> binary"
install -m 0755 "$BIN_SRC" "$BIN_DST"
"$BIN_DST" --help >/dev/null 2>&1 && echo "    $BIN_DST runs" || {
    echo "error: installed binary does not execute on this host" >&2
    exit 1
}

echo "==> directories"
install -d -o "$SERVICE_USER" -g "$SERVICE_USER" -m 0750 "$CONF_DIR" "$DATA_DIR"

echo "==> config"
if [ -f "$CONF" ] && grep -q '^access_token' "$CONF"; then
    # Keep the existing token so the client is not locked out on upgrade
    TOKEN="$(grep '^access_token' "$CONF" | head -1 | sed 's/.*=\s*"\(.*\)"/\1/')"
    echo "    keeping existing token"
else
    TOKEN="$(openssl rand -base64 24 2>/dev/null || head -c18 /dev/urandom | base64)"
    cat > "$CONF" <<EOF
# Managed by install-remote.sh. Bound to the VPN interface only.
update_interval_seconds = 5
max_clients = 100
enable_authentication = true
log_level = "info"
access_token = "$TOKEN"
systemd_units = []
EOF
    echo "    generated new token"
fi
chown "$SERVICE_USER:$SERVICE_USER" "$CONF"
chmod 0640 "$CONF"

echo "==> systemd unit"
cat > "$UNIT" <<EOF
[Unit]
Description=Code Monitor Server
After=network-online.target docker.service
Wants=network-online.target

[Service]
Type=simple
User=$SERVICE_USER
Group=$SERVICE_USER
SupplementaryGroups=docker

# Health server is off: its default port 8080 collides with other services on
# these hosts, and the gRPC endpoint is what the client uses. Re-enable with a
# free --health-port if Prometheus scraping is wanted later.
ExecStart=$BIN_DST --address $BIND_IP --port $PORT --no-health --config $CONF

Restart=on-failure
RestartSec=5
TimeoutStopSec=30

# Hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=$DATA_DIR
ReadOnlyPaths=$CONF_DIR

# Footprint ceiling; the README promises a light agent
MemoryMax=200M
CPUQuota=20%
TasksMax=64

StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

echo "==> firewall"
# Open the gRPC port to the VPN subnet only. A host with a public interface
# must never expose it wider than that.
if command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | grep -q "Status: active"; then
    ufw allow from "$VPN_CIDR" to any port "$PORT" proto tcp >/dev/null 2>&1 \
        && echo "    ufw: allowed $PORT from $VPN_CIDR" \
        || echo "    ufw: rule may already exist"
else
    echo "    no active ufw; check other firewalls if the client cannot connect"
fi

echo "==> start"
systemctl daemon-reload
systemctl enable --now code-monitor-server >/dev/null 2>&1
sleep 2
systemctl is-active --quiet code-monitor-server \
    && echo "    active" \
    || { echo "error: service failed to start; run: journalctl -u code-monitor-server -n40" >&2; exit 1; }

# Hand the token back to the operator who invoked sudo, readable without root,
# so the client side can be wired up without a second privileged step.
if [ -n "${SUDO_USER:-}" ]; then
    printf '%s\n' "$TOKEN" > "$TOKEN_OUT"
    chown "$SUDO_USER" "$TOKEN_OUT"
    chmod 600 "$TOKEN_OUT"
    echo "==> token written to $TOKEN_OUT (owner $SUDO_USER)"
fi

echo "==> done: bound to $BIND_IP:$PORT (health server disabled)"
rm -f "$BIN_SRC"
