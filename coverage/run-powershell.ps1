param([Parameter(Mandatory)][string]$OutputDirectory, [switch]$Baseline)
$ErrorActionPreference = 'Stop'
Import-Module /Volumes/LaCie/GitLab/coverage-unitarios-2026-10-17/cache/session-13/powershell/modules/Pester/5.7.1/Pester.psd1
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$config = New-PesterConfiguration
$config.Run.Path = if ($Baseline) { './tests/powershell/Baseline.Tests.ps1' } else { './tests/powershell/Installers.Tests.ps1' }
$config.Run.PassThru = $true
$config.CodeCoverage.Enabled = $true
$config.CodeCoverage.Path = @('./install.ps1','./service/windows/install-nssm.ps1','./service/windows/install-task.ps1','./service/windows/uninstall.ps1')
$config.CodeCoverage.OutputFormat = 'Cobertura'
$config.CodeCoverage.OutputPath = Join-Path $OutputDirectory 'coverage.xml'
$config.CodeCoverage.CoveragePercentTarget = 100
$config.TestResult.Enabled = $true
$config.TestResult.OutputFormat = 'JUnitXml'
$config.TestResult.OutputPath = Join-Path $OutputDirectory 'unit.xml'
$config.Output.Verbosity = 'Detailed'
$result = Invoke-Pester -Configuration $config
$result | Select-Object Result,TotalCount,PassedCount,FailedCount,SkippedCount,NotRunCount,CodeCoverage | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $OutputDirectory 'summary.json')
if ($result.FailedCount -gt 0 -or $result.SkippedCount -gt 0 -or $result.NotRunCount -gt 0) { exit 1 }
if ($result.CodeCoverage.CommandsExecutedCount -ne $result.CodeCoverage.CommandsAnalyzedCount) { exit 1 }
