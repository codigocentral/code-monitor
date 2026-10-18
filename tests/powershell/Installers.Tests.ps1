BeforeAll {
    $global:UnitRepoRoot = (Resolve-Path "$PSScriptRoot/../..").Path
    $env:LOCALAPPDATA = '/Volumes/LaCie/GitLab/coverage-unitarios-2026-10-17/cache/session-13/tmp/ps-fixture'
    function Invoke-UnitNssm { $global:UnitNssmCalls.Add(($args -join ' ')) }
    # Windows cmdlets have no implementation on macOS. These fail-closed
    # declarations allow Pester to install doubles with the real signatures.
    function Expand-Archive { [CmdletBinding()]param($Path,$DestinationPath,[switch]$Force) throw 'unmocked archive extraction' }
    function Get-ScheduledTask { [CmdletBinding()]param($TaskName) throw 'unmocked task query' }
    function Stop-ScheduledTask { [CmdletBinding()]param($TaskName) throw 'unmocked task stop' }
    function Start-ScheduledTask { [CmdletBinding()]param($TaskName) throw 'unmocked task start' }
    function Unregister-ScheduledTask { [CmdletBinding(SupportsShouldProcess)]param($TaskName) throw 'unmocked task removal' }
    function Register-ScheduledTask { [CmdletBinding()]param($TaskName,$Action,$Trigger,$Settings,$Principal,[switch]$Force) throw 'unmocked registration' }
    function New-ScheduledTaskAction { [CmdletBinding()]param($Execute,$Argument,$WorkingDirectory) throw 'unmocked action' }
    function New-ScheduledTaskTrigger { [CmdletBinding()]param([switch]$AtStartup) throw 'unmocked trigger' }
    function New-ScheduledTaskSettingsSet { [CmdletBinding()]param([switch]$AllowStartIfOnBatteries,[switch]$DontStopIfGoingOnBatteries,[switch]$StartWhenAvailable,$RestartCount,$RestartInterval) throw 'unmocked settings' }
    function New-ScheduledTaskPrincipal { [CmdletBinding()]param($UserId,$LogonType,$RunLevel) throw 'unmocked principal' }
    function Get-Service { [CmdletBinding()]param($Name) throw 'unmocked service query' }
    function Stop-Service { [CmdletBinding()]param($Name,[switch]$Force) throw 'unmocked service stop' }
}

Describe 'Release installer with platform, HTTP and filesystem doubles' {
    BeforeAll {
        . "$global:UnitRepoRoot/install.ps1"
        Add-Type @'
public static class CodeMonitorUnitIdentity {
    public static object GetCurrent() { return "fixture-identity"; }
}
public class CodeMonitorUnitPrincipal {
    public static bool Administrator = true;
    public object Identity;
    public CodeMonitorUnitPrincipal(object identity) { Identity = identity; }
    public bool IsInRole(object role) { return Administrator; }
}
public static class CodeMonitorUnitEnvironment {
    public static string MachinePath = "unit-path";
    public static string GetEnvironmentVariable(string name, string scope) { return MachinePath; }
    public static void SetEnvironmentVariable(string name, string value, string scope) { MachinePath = value; }
}
'@
    }
    BeforeEach {
        [CodeMonitorUnitPrincipal]::Administrator = $true
        $global:UnitConfigExists = $false
        $global:UnitMachinePath = 'unit-path'
        $env:ProgramFiles = "$env:LOCALAPPDATA/program-files"
        $env:ProgramData = "$env:LOCALAPPDATA/program-data"
        $env:TEMP = "$env:LOCALAPPDATA/temp"
        $env:CODE_MONITOR_COMPONENT = $null
        $env:CODE_MONITOR_VERSION = $null
        $env:CODE_MONITOR_REPO = $null
        Mock Get-AdministratorIdentity { 'fixture-identity' }
        Mock New-AdministratorPrincipal { [CodeMonitorUnitPrincipal]::new($Identity) }
        Mock Get-MachinePath { $global:UnitMachinePath }
        Mock Set-MachinePath {}
        Mock Write-Host {}
        Mock New-Item {}
        Mock Copy-Item {}
        Mock Set-Content {} -RemoveParameterType Encoding
        Mock Remove-Item {}
        Mock Test-Path { $global:UnitConfigExists }
        Mock Invoke-RestMethod { @{ tag_name = 'v0.unit' } }
        Mock Invoke-WebRequest {}
        Mock Expand-Archive {}
        Mock New-ScheduledTaskAction { @{ Execute = $Execute } }
        Mock New-ScheduledTaskTrigger { @{ AtStartup = $true } }
        Mock New-ScheduledTaskPrincipal { @{ UserId = $UserId } }
        Mock New-ScheduledTaskSettingsSet { @{ RestartCount = $RestartCount } }
        Mock Register-ScheduledTask {}
        Mock Start-ScheduledTask {}
    }
    It 'checks the administrator role through provider doubles' {
        Assert-Admin
        [CodeMonitorUnitPrincipal]::Administrator = $false
        { Assert-Admin } | Should -Throw '*elevated PowerShell*'
    }
    It 'installs default server from latest release, registers and starts its task' {
        Install-CodeMonitor
        Should -Invoke Invoke-RestMethod -Times 1 -ParameterFilter { $Uri -eq 'https://api.github.com/repos/codigocentral/code-monitor/releases/latest' }
        Should -Invoke Invoke-WebRequest -Times 1 -ParameterFilter { $Uri -like '*/v0.unit/monitor-windows-x86_64.zip' }
        Should -Invoke Copy-Item -Exactly -Times 1
        Should -Invoke Register-ScheduledTask -Exactly -Times 1
        Should -Invoke Start-ScheduledTask -Exactly -Times 1
        Should -Invoke Set-MachinePath -Exactly -Times 1
        Should -Invoke Remove-Item -Exactly -Times 1
    }
    It 'installs both explicit-version components without starting and preserves existing configuration and PATH' {
        $global:UnitConfigExists = $true
        $global:UnitMachinePath = 'unit;' + (Join-Path $env:ProgramFiles 'CodeMonitor')
        Install-CodeMonitor -Version 'v1.fixture' -Component both -Repo unit/repository -NoStart
        Should -Invoke Invoke-RestMethod -Exactly -Times 0
        Should -Invoke Copy-Item -Exactly -Times 2
        Should -Invoke Set-MachinePath -Exactly -Times 0
        Should -Invoke Start-ScheduledTask -Exactly -Times 0
        Should -Invoke Set-Content -Exactly -Times 1
    }
    It 'installs client alone without creating server configuration or task' {
        Install-CodeMonitor -Component client -Version v1.fixture
        Should -Invoke Copy-Item -Exactly -Times 1 -ParameterFilter { $Path -like '*monitor-client.exe' }
        Should -Invoke Register-ScheduledTask -Exactly -Times 0
        Should -Invoke Set-Content -Exactly -Times 1
    }
    It 'uses configured component, repository and version defaults' {
        $env:CODE_MONITOR_COMPONENT = 'both'
        $env:CODE_MONITOR_VERSION = 'v2.fixture'
        $env:CODE_MONITOR_REPO = 'fixture/repository'
        Install-CodeMonitor
        Should -Invoke Invoke-WebRequest -Times 1 -ParameterFilter { $Uri -eq 'https://github.com/fixture/repository/releases/download/v2.fixture/monitor-windows-x86_64.zip' }
        Should -Invoke Copy-Item -Exactly -Times 2
    }
    It 'entrypoint propagates early platform failure before provisioning' {
        # Provider doubles remain active through the public entrypoint. The
        # filesystem barrier proves failure propagates before provisioning.
        Mock New-Item { throw 'unit abort before provisioning' }
        { & "$global:UnitRepoRoot/install.ps1" -Version v1.fixture } | Should -Throw '*unit abort before provisioning*'
        Should -Invoke Copy-Item -Exactly -Times 0
        Should -Invoke Register-ScheduledTask -Exactly -Times 0
    }
    It 'propagates failed release download before copying or registering services' {
        Mock Invoke-WebRequest { throw 'fixture download failure' }
        { Install-CodeMonitor -Version v1.fixture } | Should -Throw '*fixture download failure*'
        Should -Invoke Copy-Item -Exactly -Times 0
        Should -Invoke Register-ScheduledTask -Exactly -Times 0
    }
}

Describe 'Windows service installers with command and filesystem doubles' {
    BeforeEach {
        $global:LASTEXITCODE = 0
        $global:UnitNssmOnPath = $true
        $global:UnitNssmFallback = $false
        $global:UnitBinaryExists = $true
        $global:UnitConfigExists = $false
        $global:UnitTaskExists = $false
        $global:UnitServiceExists = $false
        $global:UnitNssmCalls = [System.Collections.Generic.List[string]]::new()
        Mock Write-Host {}
        Mock New-Item {}
        Mock Copy-Item {}
        Mock Out-File {} -RemoveParameterType Encoding
        Mock Test-Path {
            if ($Path -like '*monitor-server.exe') { return $global:UnitBinaryExists }
            return $global:UnitConfigExists
        }
        Mock Get-Command { if ($global:UnitNssmOnPath) { @{ Source='Invoke-UnitNssm' } } } -ParameterFilter { $Name -eq 'nssm.exe' }
        Mock Get-ChildItem { if ($global:UnitNssmFallback) { @{ FullName='Invoke-UnitNssm' } } } -ParameterFilter { $Filter -eq 'nssm.exe' }
        Mock Get-ScheduledTask { if ($global:UnitTaskExists) { @{ TaskName=$TaskName } } }
        Mock Get-Service { if ($global:UnitServiceExists) { @{ Name=$Name } } }
        Mock Stop-Service {}
        Mock Stop-ScheduledTask {}
        Mock Start-ScheduledTask {}
        Mock Unregister-ScheduledTask {}
        Mock Register-ScheduledTask {}
        Mock New-ScheduledTaskAction { @{ Kind='action'; Execute=$Execute } }
        Mock New-ScheduledTaskTrigger { @{ Kind='trigger' } }
        Mock New-ScheduledTaskSettingsSet { @{ Kind='settings' } }
        Mock New-ScheduledTaskPrincipal { @{ Kind='principal' } }
    }
    It 'creates a scheduled task with SYSTEM principal and restart policy' {
        & "$global:UnitRepoRoot/service/windows/install-task.ps1"
        $LASTEXITCODE | Should -Be 0
        Should -Invoke Copy-Item -Exactly -Times 2
        Should -Invoke Out-File -Exactly -Times 1
        Should -Invoke New-ScheduledTaskPrincipal -Times 1 -ParameterFilter { $UserId -eq 'SYSTEM' -and $RunLevel -eq 'Highest' }
        Should -Invoke New-ScheduledTaskSettingsSet -Times 1 -ParameterFilter { $RestartCount -eq 3 -and $RestartInterval.TotalMinutes -eq 1 }
        Should -Invoke Start-ScheduledTask -Exactly -Times 1
    }
    It 'preserves an existing task configuration' {
        $global:UnitConfigExists = $true
        & "$global:UnitRepoRoot/service/windows/install-task.ps1"
        Should -Invoke Out-File -Exactly -Times 0
    }
    It 'stops before registration when no binary was built' {
        $global:UnitBinaryExists = $false
        & "$global:UnitRepoRoot/service/windows/install-task.ps1"
        $LASTEXITCODE | Should -Be 1
        Should -Invoke Register-ScheduledTask -Exactly -Times 0
    }
    It 'installs NSSM service with captured arguments and output logs' {
        & "$global:UnitRepoRoot/service/windows/install-nssm.ps1"
        $LASTEXITCODE | Should -Be 0
        $global:UnitNssmCalls.Count | Should -Be 8
        $global:UnitNssmCalls[0] | Should -Be 'stop CodeMonitorServer'
        $global:UnitNssmCalls[2] | Should -Match '^install CodeMonitorServer .*monitor-server.exe --config '
        $global:UnitNssmCalls[-1] | Should -Be 'start CodeMonitorServer'
    }
    It 'discovers NSSM from fallback location and preserves config' {
        $global:UnitNssmOnPath = $false
        $global:UnitNssmFallback = $true
        $global:UnitConfigExists = $true
        & "$global:UnitRepoRoot/service/windows/install-nssm.ps1"
        $LASTEXITCODE | Should -Be 0
        Should -Invoke Out-File -Exactly -Times 0
        $global:UnitNssmCalls.Count | Should -Be 8
    }
    It 'rejects missing NSSM without installing files' {
        $global:UnitNssmOnPath = $false
        & "$global:UnitRepoRoot/service/windows/install-nssm.ps1"
        $LASTEXITCODE | Should -Be 1
        Should -Invoke Copy-Item -Exactly -Times 0
    }
    It 'rejects absent binary in NSSM mode' {
        $global:UnitBinaryExists = $false
        & "$global:UnitRepoRoot/service/windows/install-nssm.ps1"
        $LASTEXITCODE | Should -Be 1
        $global:UnitNssmCalls.Count | Should -Be 0
    }
    It 'leaves absent tasks and services untouched' {
        & "$global:UnitRepoRoot/service/windows/uninstall.ps1"
        Should -Invoke Stop-Service -Exactly -Times 0
        Should -Invoke Stop-ScheduledTask -Exactly -Times 0
    }
    It 'removes present tasks and NSSM service and keeps the install directory' {
        $global:UnitTaskExists = $true
        $global:UnitServiceExists = $true
        $global:UnitConfigExists = $true
        & "$global:UnitRepoRoot/service/windows/uninstall.ps1"
        Should -Invoke Stop-Service -Exactly -Times 1
        Should -Invoke Unregister-ScheduledTask -Exactly -Times 1
        $global:UnitNssmCalls[0] | Should -Be 'remove CodeMonitorServer confirm'
    }
    It 'removes NSSM service found at fallback location' {
        $global:UnitServiceExists = $true
        $global:UnitNssmOnPath = $false
        $global:UnitNssmFallback = $true
        & "$global:UnitRepoRoot/service/windows/uninstall.ps1"
        Should -Invoke Stop-Service -Exactly -Times 1
    }
    It 'reports service requiring manual removal when NSSM is missing' {
        $global:UnitServiceExists = $true
        $global:UnitNssmOnPath = $false
        & "$global:UnitRepoRoot/service/windows/uninstall.ps1"
        Should -Invoke Stop-Service -Exactly -Times 0
        Should -Invoke Write-Host -Times 1 -ParameterFilter { $Object -like '*Remove manually*' }
    }
}

Describe 'Platform provider adapters with fake types' {
    BeforeAll { . "$global:UnitRepoRoot/install.ps1" }
    It 'adapts native identity, principal and environment provider contracts' {
        Get-AdministratorIdentity -IdentityType ([CodeMonitorUnitIdentity]) | Should -Be 'fixture-identity'
        $principal = New-AdministratorPrincipal -Identity 'fixture' -PrincipalType ([CodeMonitorUnitPrincipal])
        $principal.Identity | Should -Be 'fixture'
        [CodeMonitorUnitEnvironment]::MachinePath = 'before'
        Get-MachinePath -EnvironmentType ([CodeMonitorUnitEnvironment]) | Should -Be 'before'
        Set-MachinePath -Value 'after' -EnvironmentType ([CodeMonitorUnitEnvironment])
        [CodeMonitorUnitEnvironment]::MachinePath | Should -Be 'after'
    }
}
