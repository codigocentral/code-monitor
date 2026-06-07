# Installs Code Monitor server as a scheduled task that runs at startup
# No third-party tools required - uses Windows Task Scheduler

$ErrorActionPreference = "Stop"

$installDir = "$env:LOCALAPPDATA\CodeMonitor"
$serverExe = "$installDir\monitor-server.exe"
$clientExe = "$installDir\monitor-client.exe"
$serverConfig = "$installDir\config.toml"
$clientConfig = "$installDir\client-config.toml"
$logDir = "$installDir\logs"

Write-Host "Installing Code Monitor to $installDir ..." -ForegroundColor Cyan

# Create directories
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

# Find built binaries
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$candidates = @(
    "$repoRoot\target\release\monitor-server.exe"
    "$repoRoot\target\debug\monitor-server.exe"
)

$sourceServer = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $sourceServer) {
    Write-Host "No monitor-server.exe found. Build first with: cargo build --all --release" -ForegroundColor Red
    exit 1
}

$sourceClient = $sourceServer -replace "monitor-server", "monitor-client"

Copy-Item $sourceServer $serverExe -Force
Copy-Item $sourceClient $clientExe -Force

# Create default server config if missing
if (-not (Test-Path $serverConfig)) {
    @"
update_interval_seconds = 5
max_clients = 100
enable_authentication = true
log_level = "info"
"@ | Out-File -FilePath $serverConfig -Encoding utf8
}

# Create scheduled task
$taskName = "CodeMonitorServer"
$action = New-ScheduledTaskAction -Execute $serverExe -Argument "--config `"$serverConfig`"" -WorkingDirectory $installDir
$trigger = New-ScheduledTaskTrigger -AtStartup
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
$principal = New-ScheduledTaskPrincipal -UserId "SYSTEM" -LogonType ServiceAccount -RunLevel Highest

Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Settings $settings -Principal $principal -Force | Out-Null

Start-ScheduledTask -TaskName $taskName

Write-Host "Code Monitor server installed and started as task: $taskName" -ForegroundColor Green
Write-Host "Logs: $logDir" -ForegroundColor Green
Write-Host "Config: $serverConfig" -ForegroundColor Green
Write-Host "Health check: http://localhost:8080/health" -ForegroundColor Green
