# Installs Code Monitor server as a Windows Service using NSSM
# Download NSSM from: https://nssm.cc/download

$ErrorActionPreference = "Stop"

$installDir = "$env:LOCALAPPDATA\CodeMonitor"
$serverExe = "$installDir\monitor-server.exe"
$clientExe = "$installDir\monitor-client.exe"
$serverConfig = "$installDir\config.toml"
$logDir = "$installDir\logs"

$nssm = Get-Command nssm.exe -ErrorAction SilentlyContinue
if (-not $nssm) {
    # Try common locations
    $nssm = Get-ChildItem -Path "C:\Program Files\nssm*", "C:\nssm*", "$env:LOCALAPPDATA\nssm*" -Filter "nssm.exe" -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $nssm) {
        Write-Host "nssm.exe not found. Please download from https://nssm.cc/download and add to PATH." -ForegroundColor Red
        exit 1
    }
    $nssm = $nssm.FullName
} else {
    $nssm = $nssm.Source
}

Write-Host "Using NSSM: $nssm" -ForegroundColor Cyan
Write-Host "Installing Code Monitor to $installDir ..." -ForegroundColor Cyan

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

if (-not (Test-Path $serverConfig)) {
    @"
update_interval_seconds = 5
max_clients = 100
enable_authentication = true
log_level = "info"
"@ | Out-File -FilePath $serverConfig -Encoding utf8
}

$serviceName = "CodeMonitorServer"

# Remove existing service
& $nssm stop $serviceName 2>$null
& $nssm remove $serviceName confirm 2>$null

# Install new service
& $nssm install $serviceName $serverExe "--config `"$serverConfig`""
& $nssm set $serviceName AppDirectory $installDir
& $nssm set $serviceName AppStdout "$logDir\server.out.log"
& $nssm set $serviceName AppStderr "$logDir\server.err.log"
& $nssm set $serviceName Start SERVICE_AUTO_START
& $nssm start $serviceName

Write-Host "Code Monitor server installed and started as service: $serviceName" -ForegroundColor Green
Write-Host "Logs: $logDir" -ForegroundColor Green
Write-Host "Config: $serverConfig" -ForegroundColor Green
Write-Host "Health check: http://localhost:8080/health" -ForegroundColor Green
