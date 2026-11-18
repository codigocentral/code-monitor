# Simple System Monitor - No UI Dependencies

This is a simplified version that should work on Windows without the complex UI dependencies that require Windows SDK.

## Quick Start for Windows

### Option 1: Install Windows Build Tools
```powershell
# Install Visual Studio Build Tools
winget install Microsoft.VisualStudio.2022.BuildTools

# Or install just the C++ build tools
winget install Microsoft.VisualStudio.2022.Community
```

### Option 2: Use Linux (Recommended for Development)
Since you have a Linux server available, I recommend developing and testing on Linux:
```bash
# Install Rust on Linux
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Build on Linux
cargo build --release
```

### Option 3: Simplified Build (Windows)
Let me create a version without UI dependencies:
```bash
# Build just the server (no UI needed)
cd server
cargo build

# Build just the shared library
cd ../shared  
cargo build
```

## Testing the System

### On Linux (Recommended)
1. Copy the project to your Linux server
2. Build and test there
3. Deploy binaries to Windows clients

### Manual Testing
```bash
# Test server connectivity
curl http://192.168.0.31:50051/health

# Or use netcat
nc -zv 192.168.0.31 50051
```

## Deployment Strategy

1. **Development**: Use Linux server for development
2. **Server**: Deploy to Linux server (192.168.0.31)
3. **Client**: Build Windows client separately or use cross-compilation

Would you like me to create a simplified version without UI dependencies for easier Windows building?