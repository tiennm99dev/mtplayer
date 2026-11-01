<#
.SYNOPSIS
  Applies a device identity to a running spike instance at runtime: Magisk's
  resetprop in RAM, then a framework restart so apps read the new Build.* values.

.DESCRIPTION
  Nothing in Google's images changes: the property values live in the guest's
  RAM only and are gone after a reboot or a cold boot (a Quick Boot snapshot
  taken afterwards keeps them, because it keeps RAM).

  Steps:
  1. adb root, push <repo>/.tmp/tools/magisk/magisk (spike/fetch-magisk.ps1) to
     /data/local/tmp/magisk and chmod 755.
  2. resetprop every property the identity maps to: ro.product.<field> and
     ro.product.<partition>.<field> for system, vendor, product, odm and
     system_ext; ro.build.fingerprint and each ro.<partition>.build.fingerprint;
     ro.serialno and ro.boot.serialno; plus any raw "props" entries.
  3. Unless -NoRestart: delete sys.boot_completed, run 'stop; start' as root and
     time how long the framework takes to set sys.boot_completed=1 again.
  Writes .tmp/evidence/identity/<Name>-apply-<time>.json (values read back and
  the restart seconds) and returns that object.

  The identity file (see spike/identity.sample.json) holds any of: model, brand,
  device, manufacturer, name, fingerprint, serialno, deviceName (the persisted
  Settings "Device name", written with 'settings put global device_name'), and
  "props" {name: value}.

.EXAMPLE
  pwsh spike/apply-identity.ps1 spike1 spike/identity.sample.json
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory, Position = 0)][ValidatePattern('^[A-Za-z0-9._-]+$')][string]$Name,
    [Parameter(Mandatory, Position = 1)][string]$IdentityFile,
    [switch]$NoRestart,
    [int]$TimeoutSec = 180
)

. (Join-Path $PSScriptRoot 'spike-env.ps1')

$state = Get-InstanceState $Name
if (-not $state -or -not (Get-Process -Id $state.pid -ErrorAction SilentlyContinue)) { throw "$Name is not running" }
$serial = $state.serial
$id = Get-Content -Raw -LiteralPath $IdentityFile | ConvertFrom-Json -AsHashtable
$magisk = Join-Path $Tmp 'tools\magisk\magisk'
if (-not (Test-Path -LiteralPath $magisk)) { throw "no magisk binary at $magisk; run spike/fetch-magisk.ps1" }

# Identity field -> every property that carries it.
$props = [ordered]@{}
$partitions = 'system', 'vendor', 'product', 'odm', 'system_ext'
foreach ($f in 'model', 'brand', 'device', 'manufacturer', 'name') {
    if (-not $id.ContainsKey($f)) { continue }
    $props["ro.product.$f"] = $id[$f]
    foreach ($p in $partitions) { $props["ro.product.$p.$f"] = $id[$f] }
}
if ($id.ContainsKey('fingerprint')) {
    $props['ro.build.fingerprint'] = $id.fingerprint
    foreach ($p in $partitions + 'bootimage') { $props["ro.$p.build.fingerprint"] = $id.fingerprint }
}
if ($id.ContainsKey('serialno')) { $props['ro.serialno'] = $id.serialno; $props['ro.boot.serialno'] = $id.serialno }
if ($id.ContainsKey('props')) { foreach ($k in $id.props.Keys) { $props[$k] = [string]$id.props[$k] } }
foreach ($k in $props.Keys) {
    if ($k -notmatch '^[A-Za-z0-9._-]+$' -or $props[$k] -match "['`"\\\r\n]") { throw "unsafe property $k or value" }
}

Invoke-Adb -s $serial root | Out-Null
Start-Sleep -Seconds 2
Invoke-Adb -s $serial wait-for-device
Invoke-Adb -s $serial push $magisk /data/local/tmp/magisk | Out-Null
$rp = '/data/local/tmp/magisk resetprop'
$cmds = @('chmod 755 /data/local/tmp/magisk')
foreach ($k in $props.Keys) { $cmds += "$rp $k '$($props[$k])'" }
if ($id.ContainsKey('deviceName') -and $id.deviceName -match "['`"\\\r\n]") { throw 'unsafe deviceName' }
$applyStart = Get-Date
Invoke-Adb -s $serial shell ($cmds -join '; ') | Write-Host
$applySec = [math]::Round(((Get-Date) - $applyStart).TotalSeconds, 1)

$restartSec = $null
if (-not $NoRestart) {
    # sys.boot_completed stays 1 across 'stop; start' unless it is removed first.
    $restartStart = Get-Date
    Invoke-Adb -s $serial shell "$rp --delete sys.boot_completed; stop; start" | Write-Host
    $restartSec = Wait-BootCompleted $serial $TimeoutSec $restartStart
    Write-Host "restart  framework back (sys.boot_completed=1) in $restartSec s"
}

# Settings > About shows the persisted global device_name (set from Build.MODEL on
# first boot), not Build.MODEL, so it is set separately. It lives in /data. It is
# written after the restart: SettingsProvider persists with a delay, so a value
# put just before 'stop' is lost.
if ($id.ContainsKey('deviceName')) {
    Invoke-Adb -s $serial shell "settings put global device_name '$($id.deviceName)'" | Write-Host
}

$readBack = [ordered]@{}
foreach ($line in (Invoke-Adb -s $serial shell (($props.Keys | ForEach-Object { "echo $_=`$(getprop $_)" }) -join '; '))) {
    if ($line -match '^([^=]+)=(.*)$') { $readBack[$Matches[1]] = $Matches[2] }
}
$mismatch = @($props.Keys | Where-Object { $readBack[$_] -ne $props[$_] })
if ($id.ContainsKey('deviceName')) {
    $readBack['settings.global.device_name'] = (Invoke-Adb -s $serial shell settings get global device_name | Out-String).Trim()
    if ($readBack['settings.global.device_name'] -ne $id.deviceName) { $mismatch += 'settings.global.device_name' }
}
$result = [ordered]@{
    name = $Name; serial = $serial; identityFile = (Resolve-Path -LiteralPath $IdentityFile).Path
    applied = $props.Count; mismatched = $mismatch; resetpropSeconds = $applySec
    frameworkRestartSeconds = $restartSec; readBack = $readBack
}
$dir = Join-Path $EvidenceDir 'identity'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$result | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $dir "$Name-apply-$(Get-Date -Format yyyyMMdd-HHmmss).json")
if ($mismatch.Count) { Write-Warning "not applied: $($mismatch -join ', ')" }
$result
