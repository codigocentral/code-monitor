#!/usr/bin/env bash
# Code Monitor installer for Linux and macOS.

set -euo pipefail

GITHUB_REPO="${CODE_MONITOR_REPO:-codigocentral/code-monitor}"
VERSION="${CODE_MONITOR_VERSION:-latest}"
COMPONENT="${CODE_MONITOR_COMPONENT:-server}"
START_SERVICE="${CODE_MONITOR_START_SERVICE:-true}"

usage() {
    cat <<EOF
Code Monitor installer

Usage:
  curl -sSL https://github.com/${GITHUB_REPO}/releases/latest/download/install.sh | sudo bash
  ./install.sh [--version v0.1.0] [--component server|client|both] [--no-start]

Environment:
  CODE_MONITOR_REPO       GitHub repo, default: ${GITHUB_REPO}
  CODE_MONITOR_VERSION    Release tag or latest, default: latest
  CODE_MONITOR_COMPONENT  server, client, or both, default: server
  CODE_MONITOR_START_SERVICE true/false, default: true
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version)
            VERSION="$2"
            shift 2
            ;;
        --component)
            COMPONENT="$2"
            shift 2
            ;;
        --no-start)
            START_SERVICE="false"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage
            exit 1
            ;;
    esac
done

case "$COMPONENT" in
    server|client|both) ;;
    *)
        echo "Invalid component: $COMPONENT" >&2
        exit 1
        ;;
esac

if [ "${EUID:-$(id -u)}" -ne 0 ]; then
    echo "Please run as root (use sudo)." >&2
    exit 1
fi

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "$1 is required but was not found." >&2
        exit 1
    fi
}

require_cmd curl
require_cmd tar

detect_archive_name() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)
            case "$arch" in
                x86_64|amd64) echo "linux-x86_64" ;;
                aarch64|arm64) echo "linux-aarch64" ;;
                *) echo "Unsupported Linux architecture: $arch" >&2; exit 1 ;;
            esac
            ;;
        Darwin)
            case "$arch" in
                x86_64|amd64) echo "macos-x86_64" ;;
                arm64|aarch64) echo "macos-aarch64" ;;
                *) echo "Unsupported macOS architecture: $arch" >&2; exit 1 ;;
            esac
            ;;
        *)
            echo "Unsupported OS: $os. Use install.ps1 on Windows." >&2
            exit 1
            ;;
    esac
}

detect_version() {
    if [ "$VERSION" != "latest" ]; then
        echo "$VERSION"
        return
    fi

    local tag
    tag="$(curl -fsSL "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)"
    if [ -z "$tag" ]; then
        echo "Failed to detect latest release for ${GITHUB_REPO}." >&2
        exit 1
    fi
    echo "$tag"
}

install_paths() {
    case "$(uname -s)" in
        Linux)
            BIN_DIR="/usr/local/bin"
            CONFIG_DIR="/etc/code-monitor"
            DATA_DIR="/var/lib/code-monitor"
            LOG_DIR="/var/log/code-monitor"
            ;;
        Darwin)
            BIN_DIR="/usr/local/bin"
            CONFIG_DIR="/usr/local/etc/code-monitor"
            DATA_DIR="/usr/local/var/lib/code-monitor"
            LOG_DIR="/usr/local/var/log/code-monitor"
            ;;
    esac
}

create_user_linux() {
    if [ "$(uname -s)" != "Linux" ]; then
        return
    fi
    if ! id code-monitor >/dev/null 2>&1; then
        useradd --system --no-create-home --shell /usr/sbin/nologin code-monitor
    fi
}

download_release() {
    local tag archive tmpdir url
    tag="$(detect_version)"
    archive="$(detect_archive_name)"
    tmpdir="$(mktemp -d)"
    url="https://github.com/${GITHUB_REPO}/releases/download/${tag}/monitor-${archive}.tar.gz"

    echo "Downloading Code Monitor ${tag} (${archive})..."
    curl -fL "$url" -o "${tmpdir}/monitor.tar.gz"
    tar -xzf "${tmpdir}/monitor.tar.gz" -C "$tmpdir"
    echo "$tmpdir"
}

install_binaries() {
    local tmpdir="$1"
    mkdir -p "$BIN_DIR"

    if [ "$COMPONENT" = "server" ] || [ "$COMPONENT" = "both" ]; then
        install -m 755 "${tmpdir}/monitor-server" "${BIN_DIR}/monitor-server"
    fi
    if [ "$COMPONENT" = "client" ] || [ "$COMPONENT" = "both" ]; then
        install -m 755 "${tmpdir}/monitor-client" "${BIN_DIR}/monitor-client"
    fi
}

create_server_config() {
    if [ "$COMPONENT" = "client" ]; then
        return
    fi

    mkdir -p "$CONFIG_DIR" "$DATA_DIR" "$LOG_DIR"
    if [ "$(uname -s)" = "Linux" ]; then
        chown code-monitor:code-monitor "$DATA_DIR" "$LOG_DIR"
    fi

    local config_file="${CONFIG_DIR}/config.toml"
    if [ ! -f "$config_file" ]; then
        cat > "$config_file" <<EOF
update_interval_seconds = 5
max_clients = 100
enable_authentication = true
log_level = "info"
EOF
        chmod 640 "$config_file"
    fi
}

install_update_helper() {
    cat > "${BIN_DIR}/code-monitor-update" <<EOF
#!/usr/bin/env bash
set -euo pipefail
curl -sSL https://github.com/${GITHUB_REPO}/releases/latest/download/install.sh | sudo CODE_MONITOR_COMPONENT="${COMPONENT}" bash
EOF
    chmod 755 "${BIN_DIR}/code-monitor-update"
}

install_systemd_service() {
    if [ "$(uname -s)" != "Linux" ] || [ "$COMPONENT" = "client" ]; then
        return
    fi

    cat > /etc/systemd/system/code-monitor-server.service <<EOF
[Unit]
Description=Code Monitor Server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=code-monitor
Group=code-monitor
ExecStart=${BIN_DIR}/monitor-server --config ${CONFIG_DIR}/config.toml
Restart=always
RestartSec=5
StandardOutput=append:${LOG_DIR}/server.log
StandardError=append:${LOG_DIR}/server.log
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=${DATA_DIR} ${LOG_DIR} ${CONFIG_DIR}

[Install]
WantedBy=multi-user.target
EOF

    systemctl daemon-reload
    systemctl enable code-monitor-server.service
    if [ "$START_SERVICE" = "true" ]; then
        systemctl restart code-monitor-server.service
    fi
}

install_launchd_service() {
    if [ "$(uname -s)" != "Darwin" ] || [ "$COMPONENT" = "client" ]; then
        return
    fi

    mkdir -p "$LOG_DIR"
    cat > /Library/LaunchDaemons/com.codemonitor.server.plist <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.codemonitor.server</string>
  <key>ProgramArguments</key>
  <array>
    <string>${BIN_DIR}/monitor-server</string>
    <string>--config</string>
    <string>${CONFIG_DIR}/config.toml</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>${LOG_DIR}/server.log</string>
  <key>StandardErrorPath</key>
  <string>${LOG_DIR}/server.log</string>
</dict>
</plist>
EOF
    chmod 644 /Library/LaunchDaemons/com.codemonitor.server.plist
    chown root:wheel /Library/LaunchDaemons/com.codemonitor.server.plist

    if [ "$START_SERVICE" = "true" ]; then
        launchctl bootout system /Library/LaunchDaemons/com.codemonitor.server.plist >/dev/null 2>&1 || true
        launchctl bootstrap system /Library/LaunchDaemons/com.codemonitor.server.plist
    fi
}

main() {
    install_paths
    create_user_linux
    tmpdir="$(download_release)"
    install_binaries "$tmpdir"
    create_server_config
    install_update_helper
    install_systemd_service
    install_launchd_service
    rm -rf "$tmpdir"

    echo
    echo "Code Monitor installed."
    echo "Binaries: ${BIN_DIR}"
    echo "Config:   ${CONFIG_DIR}"
    if [ "$COMPONENT" != "client" ]; then
        echo
        echo "Show server token:"
        echo "  monitor-server --config ${CONFIG_DIR}/config.toml show-token"
    fi
    echo
    echo "Update later with:"
    echo "  code-monitor-update"
}

main
