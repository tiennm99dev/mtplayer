<#
.SYNOPSIS
  Measures cold boot (no snapshot load) of an already initialized spike AVD.

.DESCRIPTION
  Runs -Runs times: record host state, start the instance with -no-snapshot
  (no Quick Boot load and no save), wait for sys.boot_completed=1 (timed from
  launch), stop it. No save matters: a snapshot taken seconds after
  boot_completed restores into a guest stuck at 'Phone is starting' with adb
  offline, which would break the next Quick Boot.
  Writes each run and the median to .tmp/evidence/<Name>-coldboot-runs.txt and
  the isolated adb server's device list from the first run to
  .tmp/evidence/<Name>-adb-devices.txt. The AVD must have booted once already,
  so its first-boot work (userdata creation, ART) is not counted.
#>
[CmdletBinding()]
param(
    [Parameter(Position = 0)][ValidatePattern('^[A-Za-z0-9._-]+$')][string]$Name = 'spike0',
    [Parameter(Position = 1)][int]$ConsolePort = 5600,
    [int]$Runs = 3
)

. (Join-Path $PSScriptRoot 'spike-env.ps1')

if (-not (Test-Path -LiteralPath (Join-Path $AvdDir "$Name.avd\userdata-qemu.img"))) {
    throw "$Name has never booted; boot it once (spike/run-emulator.ps1 $Name) before measuring cold boot"
}
$out = Join-Path $EvidenceDir "$Name-coldboot-runs.txt"
$times = @()
for ($i = 1; $i -le $Runs; $i++) {
    $host_ = & (Join-Path $PSScriptRoot 'record-host-state.ps1') -Label "cold boot run $i" | Select-Object -Last 1
    $serial = & (Join-Path $PSScriptRoot 'start-instance.ps1') $Name $ConsolePort -ExtraArgs '-no-snapshot' | Select-Object -Last 1
    try {
        $secs = Wait-BootCompleted $serial 300 ([datetime](Get-InstanceState $Name).started)
        $times += $secs
        Add-Content -LiteralPath $out -Value "$((Get-Date).ToString('o')) run $i cold boot ${secs} s (host state: $(Split-Path -Leaf $host_))"
        Write-Host "run $i   cold boot $secs s"
        if ($i -eq 1) { Invoke-Adb devices -l 2>&1 | Set-Content -LiteralPath (Join-Path $EvidenceDir "$Name-adb-devices.txt") }
    } finally {
        & (Join-Path $PSScriptRoot 'stop-instance.ps1') $Name -KeepAdb:($i -lt $Runs)
    }
}
$sorted = $times | Sort-Object
$median = if ($sorted.Count % 2) { $sorted[[int][math]::Floor($sorted.Count / 2)] } else { ($sorted[$sorted.Count / 2 - 1] + $sorted[$sorted.Count / 2]) / 2 }
Add-Content -LiteralPath $out -Value "median of $($times.Count) runs: $median s (runs: $($times -join ', '))"
Write-Host "median   $median s"