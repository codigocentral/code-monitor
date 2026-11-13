# Running Code Monitor as a Service

This folder contains scripts and configuration files to run the **Code Monitor server** (`monitor-server`) as a system service on Windows, Linux, and macOS.

> The **client** (`monitor-client`) is a TUI application and is meant to be run interactively from a terminal, not as a background service.

---

## Quick Start

1. Build the project first:
   ```bash
   cargo build --all --release
   ```

2. Pick your platform below and run the install script.

---

## Windows

Two options are provided. The Task Scheduler option does not require any third-party tools.

### Option A: Task Scheduler (recommended, no extra tools)

Open PowerShell **as Administrator** and run:

```powershell
.\service\windows\install-task.ps1
```

This creates a scheduled task named `CodeMonitorServer` that:
- Starts automatically at boot
- Runs as `SYSTEM`
- Restarts up to 3 times if it crashes

Check status:
```powershell
Get-ScheduledTask -TaskName CodeMonitorServer
```

### Option B: Windows Service via NSSM

If you prefer a true Windows Service, install [NSSM](https://nssm.cc/download) first and make sure `nssm.exe` is in your PATH.

Open PowerShell **as Administrator** and run:

```powershell
.\service\windows\install-nssm.ps1
```

Check status:
```powershell
Get-Service -Name CodeMonitorServer
```

### Uninstall (Windows)

```powershell
.\service\windows\uninstall.ps1
```

---

## Linux (systemd)

Run the install script with `sudo`:

```bash
sudo ./service/linux/install.sh
```

This will:
- Create a `code-monitor` system user
- Install binaries to `/usr/local/bin/`
- Create config at `/etc/code-monitor/config.toml`
- Enable and start the systemd service

Check status:
```bash
sudo systemctl status code-monitor-server
```

View logs:
```bash
sudo journalctl -u code-monitor-server -f
```

### Uninstall (Linux)

```bash
sudo ./service/linux/uninstall.sh
```

---

## macOS (launchd)

Run the install script:

```bash
./service/macos/install.sh
```

This will:
- Install binaries to `/usr/local/bin/`
- Create config at `/usr/local/etc/code-monitor/config.toml`
- Install a `LaunchAgent` for the current user
- Start the service immediately

Check status:
```bash
launchctl list | grep com.codemonitor.server
```

View logs:
```bash
tail -f /usr/local/var/log/code-monitor/server.out.log
```

### Uninstall (macOS)

```bash
./service/macos/uninstall.sh
```

---

## Health Check

Once the service is running, verify it on any platform:

```bash
curl http://localhost:8080/health
```

You should see a JSON response like:
```json
{"status":"healthy","version":"0.1.0","timestamp":"..."}
```

---

## Connecting the Client

After the server is running, configure and run the client from a terminal:

```bash
# Show the server's access token
monitor-server show-token

# Create client-config.toml and add your server, then run:
monitor-client dashboard
```
