# Uninstalls Code Monitor server from Windows

$ErrorActionPreference = "Stop"

# Remove scheduled task if exists
$taskName = "CodeMonitorServer"
$task = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
if ($task) {
    Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
    Write-Host "Removed scheduled task: $taskName" -ForegroundColor Green
}

# Remove NSSM service if exists
$serviceName = "CodeMonitorServer"
$service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
if ($service) {
    $nssm = Get-Command nssm.exe -ErrorAction SilentlyContinue
    if (-not $nssm) {
        $nssm = Get-ChildItem -Path "C:\Program Files\nssm*", "C:\nssm*", "$env:LOCALAPPDATA\nssm*" -Filter "nssm.exe" -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($nssm) { $nssm = $nssm.FullName }
    } else {
        $nssm = $nssm.Source
    }

    if ($nssm) {
        Stop-Service -Name $serviceName -Force -ErrorAction SilentlyContinue
        & $nssm remove $serviceName confirm
        Write-Host "Removed service: $serviceName" -ForegroundColor Green
    } else {
        Write-Host "Service exists but nssm.exe not found. Remove manually." -ForegroundColor Yellow
    }
}

$installDir = "$env:LOCALAPPDATA\CodeMonitor"
if (Test-Path $installDir) {
    Write-Host "Install directory kept at: $installDir" -ForegroundColor Yellow
    Write-Host "Delete manually if desired." -ForegroundColor Yellow
}
