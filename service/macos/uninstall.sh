#!/bin/bash
set -e

PLIST_DIR="$HOME/Library/LaunchAgents"
PLIST_FILE="$PLIST_DIR/com.codemonitor.server.plist"

echo "==> Uninstalling Code Monitor server"

if [ -f "$PLIST_FILE" ]; then
    launchctl unload "$PLIST_FILE" 2>/dev/null || true
    rm "$PLIST_FILE"
fi

sudo rm -f /usr/local/bin/monitor-server /usr/local/bin/monitor-client

echo "==> Removed launch agent and binaries"
echo "    Config kept at: /usr/local/etc/code-monitor (delete manually if desired)"
