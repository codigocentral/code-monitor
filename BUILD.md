# Build and Deployment Guide

## Quick Start

### 1. Build the Project
```bash
# Build everything
cargo build --release

# Or build components individually
cd shared && cargo build --release
cd server && cargo build --release  
cd client && cargo build --release
```

### 2. Server Setup
```bash
# Start the monitoring server
./target/release/monitor-server --address 0.0.0.0 --port 50051

# Server will automatically generate authentication keys
# Check logs for key generation confirmation
```

### 3. Client Setup
```bash
# Generate client authentication keys
./target/release/monitor-client generate-keys

# Add the server you want to monitor
./target/release/monitor-client add \
    --name "My Server" \
    --address 192.168.1.100 \
    --port 50051

# Start monitoring
./target/release/monitor-client monitor
```

## Testing Your Setup

### Test Server Connectivity
```bash
# Test if server is running
curl http://192.168.1.100:50051/health

# Or use telnet
telnet 192.168.1.100 50051
```

### Test Client Connection
```bash
# List configured servers
./target/release/monitor-client list

# Test connection to specific server
./target/release/monitor-client monitor --server <uuid>
```

## Configuration

### Server Configuration
Create `server-config.toml`:
```toml
update_interval_seconds = 5
max_clients = 100
enable_authentication = true
log_level = "info"
```

### Client Configuration  
Create `client-config.toml`:
```toml
update_interval_seconds = 5
auto_reconnect = true
reconnect_delay_seconds = 5

[[servers]]
id = "your-server-uuid"
name = "Production Server"
address = "192.168.1.100"
port = 50051
description = "Main production server"
```

## Production Deployment

### 1. Server Deployment
```bash
# Copy server binary to target machine
scp target/release/monitor-server user@192.168.1.100:/usr/local/bin/

# SSH to server and start it
ssh user@192.168.1.100
nohup monitor-server --address 0.0.0.0 --port 50051 &
```

### 2. Client Setup
```bash
# Install client on monitoring machine
cp target/release/monitor-client /usr/local/bin/

# Setup systemd service (optional)
sudo systemctl enable monitor-client
sudo systemctl start monitor-client
```

### 3. Firewall Configuration
```bash
# Allow monitoring port on server
sudo ufw allow 50051/tcp

# Or for firewalld
sudo firewall-cmd --add-port=50051/tcp --permanent
sudo firewall-cmd --reload
```

## Troubleshooting

### Build Issues
```bash
# Update Rust
rustup update

# Clean build
cargo clean
cargo build --release

# Check dependencies
cargo tree
```

### Runtime Issues
```bash
# Check server logs
./monitor-server --log-level debug

# Test network connectivity
ping 192.168.1.100
nmap -p 50051 192.168.1.100

# Check authentication
# Ensure key files exist and have correct permissions
ls -la keys/
chmod 600 keys/client_private.key
```

### Performance Tuning
```bash
# Increase update intervals to reduce load
./monitor-client monitor --interval 30

# Monitor server resources
top
htop
iostat
```

## Security Considerations

### Key Management
- Keep private keys secure (600 permissions)
- Rotate keys regularly
- Use separate keys for different environments

### Network Security
- Use TLS in production
- Restrict access with firewall rules
- Monitor authentication logs

### Access Control
- Implement proper user permissions
- Use VPN for remote access
- Regular security audits