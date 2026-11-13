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
# the host metrics on a public interface — the exact problem the
# network collector was built to catch — so this script refuses it.
#
# Idempotent: re-running upgrades the binary and restarts the service without
# disturbing an existing token, so the client keeps working across upgrades.

set -euo pipefail

# Optional filesystem staging root; empty keeps the normal host paths.
PREFIX="${CODE_MONITOR_PREFIX:-}"

BIND_IP="${1:?usage: install-remote.sh <bind-ip> [port]}"
PORT="${2:-50051}"
# The VPN subnet allowed to reach the gRPC port through the firewall
VPN_CIDR="${3:-10.0.0.0/24}"

SERVICE_USER="code-monitor"
BIN_SRC="${PREFIX}/tmp/monitor-server"
BIN_DST="${PREFIX}/usr/local/bin/monitor-server"
CONF_DIR="${PREFIX}/etc/code-monitor"
CONF="$CONF_DIR/config.toml"
DATA_DIR="${PREFIX}/var/lib/code-monitor"
UNIT="${PREFIX}/etc/systemd/system/code-monitor-server.service"
SNAP_UNIT="${PREFIX}/etc/systemd/system/code-monitor-firewall-snapshot.service"
SNAP_TIMER="${PREFIX}/etc/systemd/system/code-monitor-firewall-snapshot.timer"
TOKEN_OUT="${PREFIX}/tmp/cm-token.txt"

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
# Firewall snapshot: written by root, readable by the agent only
install -d -o root -g "$SERVICE_USER" -m 0750 "$DATA_DIR/firewall"

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
log_level = "warn"
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
# No capabilities: the firewall is read from the files that
# code-monitor-firewall-snapshot.timer (root) saves in /var/lib/code-monitor/firewall.
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

echo "==> firewall snapshot timer"
# Root oneshot that saves iptables-save/ip6tables-save every minute, so the
# agent never needs CAP_NET_ADMIN. Same units as service/linux/.
cat > "$SNAP_UNIT" <<'EOF'
# Saves the packet filter for the code-monitor audit, so the agent itself
# never needs CAP_NET_ADMIN (which would also let it change the rules).
# Triggered every minute by code-monitor-firewall-snapshot.timer; the agent
# treats a snapshot older than 5 minutes as unavailable.
[Unit]
Description=Code Monitor firewall snapshot (iptables-save for the audit)
Documentation=https://github.com/codigocentral/code-monitor
After=network.target

[Service]
Type=oneshot
# root to read the rules; group code-monitor so the files come out
# root:code-monitor and the agent (and nobody else) can read them
User=root
Group=code-monitor
UMask=0027
StateDirectory=code-monitor/firewall
StateDirectoryMode=0750
Environment=PATH=/usr/sbin:/usr/bin:/sbin:/bin
# Each file is written to a temporary name in the same directory and renamed,
# so the agent never reads half a rule set.
ExecStart=/bin/sh -c 'd=/var/lib/code-monitor/firewall; rc=0; \
  for t in iptables ip6tables; do \
    tmp=$$(mktemp "$$d/.$$t.rules.XXXXXX") || exit 1; \
    if "$$t-save" > "$$tmp" && chmod 0640 "$$tmp" && mv -f "$$tmp" "$$d/$$t.rules"; then :; \
    else rm -f "$$tmp"; echo "$$t-save failed" >&2; rc=1; fi; \
  done; exit $$rc'
TimeoutStartSec=30

# Hardening: only the snapshot directory is writable
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/var/lib/code-monitor/firewall
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectKernelLogs=true
ProtectControlGroups=true
ProtectClock=true
ProtectHostname=true
RestrictNamespaces=true
RestrictRealtime=true
RestrictSUIDSGID=true
LockPersonality=true
MemoryDenyWriteExecute=true
RestrictAddressFamilies=AF_UNIX AF_NETLINK AF_INET AF_INET6
CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_RAW
SystemCallArchitectures=native
EOF
cat > "$SNAP_TIMER" <<'EOF'
[Unit]
Description=Code Monitor firewall snapshot every minute
Documentation=https://github.com/codigocentral/code-monitor

[Timer]
OnBootSec=15s
OnUnitActiveSec=1min
AccuracySec=5s
Unit=code-monitor-firewall-snapshot.service

[Install]
WantedBy=timers.target
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
systemctl enable --now code-monitor-firewall-snapshot.timer >/dev/null 2>&1
# First snapshot now, so the audit does not wait for the timer
systemctl start code-monitor-firewall-snapshot.service \
    && echo "    firewall snapshot written" \
    || echo "    warning: firewall snapshot failed; run: journalctl -u code-monitor-firewall-snapshot -n20" >&2
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
