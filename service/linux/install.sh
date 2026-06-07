#!/bin/bash
set -e

INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="/etc/code-monitor"
DATA_DIR="/var/lib/code-monitor"
SERVICE_USER="code-monitor"

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

echo "==> Installing Code Monitor server (Linux / systemd)"

# Detect binary path
if [ -f "$REPO_ROOT/target/release/monitor-server" ]; then
    SERVER_BIN="$REPO_ROOT/target/release/monitor-server"
    CLIENT_BIN="$REPO_ROOT/target/release/monitor-client"
elif [ -f "$REPO_ROOT/target/debug/monitor-server" ]; then
    SERVER_BIN="$REPO_ROOT/target/debug/monitor-server"
    CLIENT_BIN="$REPO_ROOT/target/debug/monitor-client"
else
    echo "No monitor-server binary found. Build first with: cargo build --all --release"
    exit 1
fi

# Create user
if ! id -u "$SERVICE_USER" >/dev/null 2>&1; then
    sudo useradd --system --no-create-home --shell /usr/sbin/nologin "$SERVICE_USER"
fi

# Create directories
sudo mkdir -p "$CONFIG_DIR" "$DATA_DIR"

# Install binaries
sudo cp "$SERVER_BIN" "$INSTALL_DIR/monitor-server"
sudo cp "$CLIENT_BIN" "$INSTALL_DIR/monitor-client"
sudo chmod +x "$INSTALL_DIR/monitor-server" "$INSTALL_DIR/monitor-client"

# Create default config
if [ ! -f "$CONFIG_DIR/config.toml" ]; then
    sudo tee "$CONFIG_DIR/config.toml" >/dev/null <<'EOF'
update_interval_seconds = 5
max_clients = 100
enable_authentication = true
log_level = "info"
EOF
fi

sudo chown -R "$SERVICE_USER:$SERVICE_USER" "$CONFIG_DIR" "$DATA_DIR"

# Install systemd service
sudo cp "$(dirname "$0")/code-monitor-server.service" /etc/systemd/system/code-monitor-server.service
sudo systemctl daemon-reload
sudo systemctl enable code-monitor-server.service
sudo systemctl start code-monitor-server.service

echo "==> Code Monitor server installed and started"
echo "    Config:  $CONFIG_DIR/config.toml"
echo "    Logs:    sudo journalctl -u code-monitor-server -f"
echo "    Health:  curl http://localhost:8080/health"
