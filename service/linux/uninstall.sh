#!/bin/bash
set -e

echo "==> Uninstalling Code Monitor server"

sudo systemctl stop code-monitor-server.service || true
sudo systemctl disable code-monitor-server.service || true
sudo rm -f /etc/systemd/system/code-monitor-server.service
sudo systemctl daemon-reload

sudo rm -f /usr/local/bin/monitor-server /usr/local/bin/monitor-client

echo "==> Removed service and binaries"
echo "    Config kept at: /etc/code-monitor (delete manually if desired)"
