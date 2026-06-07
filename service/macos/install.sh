#!/bin/bash
set -e

INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="/usr/local/etc/code-monitor"
LOG_DIR="/usr/local/var/log/code-monitor"
PLIST_DIR="$HOME/Library/LaunchAgents"

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

echo "==> Installing Code Monitor server (macOS / launchd)"

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

# Create directories
sudo mkdir -p "$INSTALL_DIR" "$CONFIG_DIR" "$LOG_DIR"

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

# Install LaunchAgent
mkdir -p "$PLIST_DIR"
cp "$(dirname "$0")/com.codemonitor.server.plist" "$PLIST_DIR/com.codemonitor.server.plist"

# Load service
launchctl unload "$PLIST_DIR/com.codemonitor.server.plist" 2>/dev/null || true
launchctl load "$PLIST_DIR/com.codemonitor.server.plist"

echo "==> Code Monitor server installed and started"
echo "    Config:  $CONFIG_DIR/config.toml"
echo "    Logs:    $LOG_DIR"
echo "    Health:  curl http://localhost:8080/health"
echo "    Control: launchctl list | grep com.codemonitor.server"
