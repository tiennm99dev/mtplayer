<#
.SYNOPSIS
  Records read-only host state that can skew boot or benchmark numbers.

.DESCRIPTION
  Writes .tmp/evidence/host-state-<yyyyMMdd-HHmmss>.txt: hypervisor present,
  running WSL distros (none are started), Docker Desktop, MuMuPlayer and
  Overwolf processes. Starts, stops or changes nothing. Returns the file path.
#>
[CmdletBinding()]
param([string]$Label = '')

. (Join-Path $PSScriptRoot 'spike-env.ps1')

New-Item -ItemType Directory -Force -Path $EvidenceDir | Out-Null
$now = Get-Date
$file = Join-Path $EvidenceDir ("host-state-{0}.txt" -f $now.ToString('yyyyMMdd-HHmmss'))
$wsl = if (Get-Command wsl.exe -ErrorAction SilentlyContinue) {
    # wsl.exe writes UTF-16; --running lists only distros already up and starts nothing.
    $prev = [Console]::OutputEncoding; [Console]::OutputEncoding = [Text.Encoding]::Unicode
    try { ((& wsl.exe --list --running 2>&1) | Out-String).Trim() } finally { [Console]::OutputEncoding = $prev }
} else { 'wsl.exe not found' }
function Get-Names([string[]]$Patterns) {
    $p = Get-Process -ErrorAction SilentlyContinue | Where-Object { $n = $_.ProcessName; $Patterns | Where-Object { $n -like $_ } }
    if ($p) { ($p | Group-Object ProcessName | ForEach-Object { "$($_.Name) x$($_.Count)" }) -join ', ' } else { 'none' }
}
@(
    "time: $($now.ToString('o')) $Label"
    "HypervisorPresent: $((Get-CimInstance Win32_ComputerSystem).HypervisorPresent)"
    "WSL running: $wsl"
    "Docker: $(Get-Names 'Docker Desktop', 'com.docker.*', 'dockerd')"
    "MuMu: $(Get-Names 'MuMu*')"
    "Overwolf: $(Get-Names 'Overwolf*')"
) | Set-Content -LiteralPath $file
Get-Content -LiteralPath $file | Write-Host
$file