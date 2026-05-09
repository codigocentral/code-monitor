# Code Monitor installer for Windows.

[CmdletBinding()]
param(
    [string]$Version = $env:CODE_MONITOR_VERSION,
    [ValidateSet("server", "client", "both")]
    [string]$Component = $(if ($env:CODE_MONITOR_COMPONENT) { $env:CODE_MONITOR_COMPONENT } else { "server" }),
    [string]$Repo = $(if ($env:CODE_MONITOR_REPO) { $env:CODE_MONITOR_REPO } else { "codigocentral/code-monitor" }),
    [switch]$NoStart
)

$ErrorActionPreference = "Stop"

function Assert-Admin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Run this installer from an elevated PowerShell session."
    }
}

function Get-LatestVersion {
    param([string]$Repository)
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases/latest" -Headers @{ "User-Agent" = "code-monitor-installer" }
    return $release.tag_name
}

Assert-Admin

if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = Get-LatestVersion -Repository $Repo
}

$installDir = Join-Path $env:ProgramFiles "CodeMonitor"
$configDir = Join-Path $env:ProgramData "CodeMonitor"
$logDir = Join-Path $configDir "logs"
$archiveName = "monitor-windows-x86_64.zip"
$downloadUrl = "https://github.com/$Repo/releases/download/$Version/$archiveName"
$tmpDir = Join-Path $env:TEMP ("code-monitor-" + [guid]::NewGuid().ToString("N"))

Write-Host "Installing Code Monitor $Version from $Repo"
Write-Host "Component: $Component"

New-Item -ItemType Directory -Force -Path $installDir, $configDir, $logDir | Out-Null
New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null

$zipPath = Join-Path $tmpDir $archiveName
Invoke-WebRequest -Uri $downloadUrl -OutFile $zipPath
Expand-Archive -Path $zipPath -DestinationPath $tmpDir -Force

if ($Component -eq "server" -or $Component -eq "both") {
    Copy-Item -Path (Join-Path $tmpDir "monitor-server.exe") -Destination (Join-Path $installDir "monitor-server.exe") -Force
}
if ($Component -eq "client" -or $Component -eq "both") {
    Copy-Item -Path (Join-Path $tmpDir "monitor-client.exe") -Destination (Join-Path $installDir "monitor-client.exe") -Force
}

$machinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
if ($machinePath -notlike "*$installDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$machinePath;$installDir", "Machine")
}

$configFile = Join-Path $configDir "config.toml"
if (($Component -eq "server" -or $Component -eq "both") -and -not (Test-Path $configFile)) {
@"
update_interval_seconds = 5
max_clients = 100
enable_authentication = true
log_level = "info"
"@ | Set-Content -Path $configFile -Encoding UTF8
}

$updateScript = Join-Path $installDir "code-monitor-update.ps1"
@"
`$ErrorActionPreference = "Stop"
`$script = Invoke-WebRequest -Uri "https://github.com/$Repo/releases/latest/download/install.ps1" -UseBasicParsing
Invoke-Expression `$script.Content
"@ | Set-Content -Path $updateScript -Encoding UTF8

if ($Component -eq "server" -or $Component -eq "both") {
    $taskName = "CodeMonitorServer"
    $exe = Join-Path $installDir "monitor-server.exe"
    $action = New-ScheduledTaskAction -Execute $exe -Argument "--config `"$configFile`""
    $trigger = New-ScheduledTaskTrigger -AtStartup
    $principal = New-ScheduledTaskPrincipal -UserId "SYSTEM" -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
    Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
    if (-not $NoStart) {
        Start-ScheduledTask -TaskName $taskName
    }
}

Remove-Item -LiteralPath $tmpDir -Recurse -Force

Write-Host ""
Write-Host "Code Monitor installed."
Write-Host "Binaries: $installDir"
Write-Host "Config:   $configDir"
Write-Host ""
Write-Host "Open a new terminal to use monitor-server or monitor-client from PATH."
Write-Host "Update later with: powershell -ExecutionPolicy Bypass -File `"$updateScript`""
