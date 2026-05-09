#!/bin/bash
# Code Monitor Server Installer
# Downloads and installs monitor-server with systemd integration

set -e

# Configuration
GITHUB_REPO="diogo/code-monitor"
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="/etc/code-monitor"
DATA_DIR="/var/lib/code-monitor"
LOG_DIR="/var/log/code-monitor"
USER_NAME="code-monitor"
SERVICE_NAME="code-monitor-server"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Detect architecture
detect_arch() {
    local arch=$(uname -m)
    case "$arch" in
        x86_64)
            echo "linux-x86_64"
            ;;
        aarch64|arm64)
            echo "linux-aarch64"
            ;;
        *)
            echo "Unsupported architecture: $arch" >&2
            exit 1
            ;;
    esac
}

# Detect latest release version
detect_version() {
    echo "Detecting latest version..."
    local version=$(curl -s "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$version" ]; then
        echo -e "${RED}Failed to detect latest version${NC}"
        exit 1
    fi
    echo "$version"
}

# Download binary
download_binary() {
    local version=$1
    local arch=$2
    local url="https://github.com/${GITHUB_REPO}/releases/download/${version}/monitor-server-${arch}.tar.gz"
    local tmpdir=$(mktemp -d)
    
    echo "Downloading monitor-server ${version} for ${arch}..."
    if ! curl -sL "$url" -o "${tmpdir}/monitor-server.tar.gz"; then
        echo -e "${RED}Failed to download binary${NC}"
        rm -rf "$tmpdir"
        exit 1
    fi
    
    echo "Extracting..."
    tar -xzf "${tmpdir}/monitor-server.tar.gz" -C "$tmpdir"
    
    if [ ! -f "${tmpdir}/monitor-server" ]; then
        echo -e "${RED}Binary not found in archive${NC}"
        rm -rf "$tmpdir"
        exit 1
    fi
    
    echo "Installing to ${INSTALL_DIR}..."
    install -m 755 "${tmpdir}/monitor-server" "${INSTALL_DIR}/monitor-server"
    rm -rf "$tmpdir"
}

# Create user and directories
setup_environment() {
    echo "Setting up environment..."
    
    # Create user if not exists
    if ! id "$USER_NAME" &>/dev/null; then
        useradd --system --no-create-home --shell /usr/sbin/nologin "$USER_NAME"
        echo -e "${GREEN}Created user: ${USER_NAME}${NC}"
    fi
    
    # Create directories
    mkdir -p "$CONFIG_DIR" "$DATA_DIR" "$LOG_DIR"
    chown "$USER_NAME:$USER_NAME" "$DATA_DIR" "$LOG_DIR"
    
    echo -e "${GREEN}Created directories:${NC}"
    echo "  - $CONFIG_DIR"
    echo "  - $DATA_DIR"
    echo "  - $LOG_DIR"
}

# Generate certificates if needed
generate_certs() {
    local cert_dir="${CONFIG_DIR}/certs"
    
    if [ -f "${cert_dir}/server.crt" ] && [ -f "${cert_dir}/server.key" ]; then
        echo -e "${YELLOW}Certificates already exist, skipping generation${NC}"
        return
    fi
    
    echo "Generating self-signed certificates..."
    mkdir -p "$cert_dir"
    
    if [ -f "./generate-certs.sh" ]; then
        ./generate-certs.sh "$cert_dir"
    else
        # Inline certificate generation
        openssl genrsa -out "${cert_dir}/server.key" 4096 2>/dev/null
        openssl req -new -x509 -key "${cert_dir}/server.key" \
            -out "${cert_dir}/server.crt" -days 365 \
            -subj "/CN=code-monitor" 2>/dev/null
        chmod 600 "${cert_dir}/server.key"
        chmod 644 "${cert_dir}/server.crt"
    fi
    
    chown -R "$USER_NAME:$USER_NAME" "$cert_dir"
    echo -e "${GREEN}Certificates generated in ${cert_dir}${NC}"
}

# Create default config
create_config() {
    local config_file="${CONFIG_DIR}/config.toml"
    
    if [ -f "$config_file" ]; then
        echo -e "${YELLOW}Config already exists, skipping${NC}"
        return
    fi
    
    cat > "$config_file" << 'EOF'
update_interval_seconds = 5
max_clients = 100
enable_authentication = true
log_level = "info"

[tls]
cert_path = "/etc/code-monitor/certs/server.crt"
key_path = "/etc/code-monitor/certs/server.key"
EOF
    
    chown "$USER_NAME:$USER_NAME" "$config_file"
    chmod 640 "$config_file"
    echo -e "${GREEN}Created default config: ${config_file}${NC}"
}

# Create systemd service
create_systemd_service() {
    local service_file="/etc/systemd/system/${SERVICE_NAME}.service"
    
    cat > "$service_file" << EOF
[Unit]
Description=Code Monitor Server
After=network.target

[Service]
Type=simple
User=${USER_NAME}
Group=${USER_NAME}
ExecStart=${INSTALL_DIR}/monitor-server --config ${CONFIG_DIR}/config.toml
Restart=always
RestartSec=5
StandardOutput=append:${LOG_DIR}/server.log
StandardError=append:${LOG_DIR}/server.log

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=${DATA_DIR} ${LOG_DIR}

[Install]
WantedBy=multi-user.target
EOF
    
    systemctl daemon-reload
    echo -e "${GREEN}Created systemd service: ${SERVICE_NAME}${NC}"
}

# Main installation
main() {
    echo "=================================="
    echo "  Code Monitor Server Installer"
    echo "=================================="
    echo ""
    
    # Check root
    if [ "$EUID" -ne 0 ]; then
        echo -e "${RED}Please run as root (use sudo)${NC}"
        exit 1
    fi
    
    # Check dependencies
    if ! command -v curl &> /dev/null; then
        echo -e "${RED}curl is required but not installed${NC}"
        exit 1
    fi
    
    local arch=$(detect_arch)
    local version=$(detect_version)
    
    echo "Architecture: $arch"
    echo "Version: $version"
    echo ""
    
    download_binary "$version" "$arch"
    setup_environment
    generate_certs
    create_config
    create_systemd_service
    
    echo ""
    echo -e "${GREEN}==================================${NC}"
    echo -e "${GREEN}  Installation Complete!${NC}"
    echo -e "${GREEN}==================================${NC}"
    echo ""
    echo "Show access token:"
    echo "  monitor-server show-token --config ${CONFIG_DIR}/config.toml"
    echo ""
    echo "Start the service:"
    echo "  systemctl start ${SERVICE_NAME}"
    echo "  systemctl enable ${SERVICE_NAME}"
    echo ""
    echo "View logs:"
    echo "  journalctl -u ${SERVICE_NAME} -f"
    echo ""
}

main "$@"
