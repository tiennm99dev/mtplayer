<#
.SYNOPSIS
  Full clone of a stopped spike AVD (its userdata and other qcow2 overlays), then
  a first boot with no network that gives the clone new per-device IDs.

.DESCRIPTION
  Copy: <Source>.avd -> <Target>.avd, leaving out snapshots (the clone always
  cold boots first), lock files and per-launch files the emulator rewrites
  (hardware-qemu.ini, emu-launch-params.txt, read-snapshot.txt, tmpAdbCmds).
  The qcow2 overlays name their backing file relatively (userdata-qemu.img,
  cache.img, encryptionkey.img), so a plain copy stays self-contained; this is
  checked with the emulator's qemu-img. config.ini gets the new AvdId and
  display name, and <Target>.ini points to the new folder. The system image is
  shared read-only and is not copied.

  Reset (unless -NoReset): boots the clone with -no-window, -no-snapshot and
  no network: the VirtioWifi feature off (Wi-Fi via netsimd ignores restrict=on)
  and QEMU user-mode networking restrict=on for the radio network. After 15 s it
  checks that ICMP and TCP to 8.8.8.8 and TCP to the host (10.0.2.2) all fail,
  and refuses to go on otherwise. Then as root:
    - settings delete secure android_id (the value shell and system read; the
      SettingsProvider makes a new random one on the next boot),
    - stop (framework down so nothing rewrites the files), then delete
      /data/system/users/0/settings_ssaid.xml (the per-app SSAID key store),
    - stop the emulator and cold boot it again the same way (a guest
      'adb reboot' froze emulator 37.2.13), wait for boot, read the new values.
  It stops the clone at the end unless -KeepRunning (it stays without network
  until it is restarted normally).
  'before' and 'after' are read with read-identity.ps1 -App, so they include the
  spike test app's own ANDROID_ID (its SSAID) when the app is installed; stop the
  source only after 'adb shell sync', since the copy takes the disks, not RAM.
  Writes .tmp/evidence/identity/clone-<Source>-to-<Target>.json.

.EXAMPLE
  pwsh spike/clone-instance.ps1 spike1 spike3 -ConsolePort 5630
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory, Position = 0)][ValidatePattern('^[A-Za-z0-9._-]+$')][string]$Source,
    [Parameter(Mandatory, Position = 1)][ValidatePattern('^[A-Za-z0-9._-]+$')][string]$Target,
    [int]$ConsolePort = 5630,
    [switch]$Force,
    [switch]$NoReset,
    [switch]$KeepRunning,
    [int]$TimeoutSec = 300
)

. (Join-Path $PSScriptRoot 'spike-env.ps1')

$srcDir = Join-Path $AvdDir "$Source.avd"
$dstDir = Join-Path $AvdDir "$Target.avd"
$dstIni = Join-Path $AvdDir "$Target.ini"
if (-not (Test-Path -LiteralPath $srcDir)) { throw "no AVD folder $srcDir" }
$srcState = Get-InstanceState $Source
if ($srcState -and (Get-Process -Id $srcState.pid -ErrorAction SilentlyContinue)) { throw "$Source is running (PID $($srcState.pid)); stop it first so its disks are consistent" }
if (Test-Path -LiteralPath $dstDir) {
    if (-not $Force) { throw "$dstDir exists; use -Force to replace it" }
    $dstState = Get-InstanceState $Target
    if ($dstState -and (Get-Process -Id $dstState.pid -ErrorAction SilentlyContinue)) { throw "$Target is running" }
    Remove-Item -LiteralPath $dstDir -Recurse -Force
}

# 1. Copy the AVD folder.
$skip = @('snapshots', 'tmpAdbCmds', 'hardware-qemu.ini', 'hardware-qemu.ini.lock', 'multiinstance.lock',
          'emu-launch-params.txt', 'read-snapshot.txt', 'snapshot.trace')
New-Item -ItemType Directory -Path $dstDir | Out-Null
$copyStart = Get-Date
Get-ChildItem -LiteralPath $srcDir -Force | Where-Object { $_.Name -notin $skip } |
    ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $dstDir -Recurse -Force }
$copySec = [math]::Round(((Get-Date) - $copyStart).TotalSeconds, 1)
$cfg = Join-Path $dstDir 'config.ini'
$lines = (Get-Content -LiteralPath $cfg) -replace '^AvdId=.*', "AvdId=$Target" -replace '^avd\.ini\.displayname=.*', "avd.ini.displayname=$Target"
[IO.File]::WriteAllText($cfg, ($lines -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText($dstIni, "avd.ini.encoding=UTF-8`npath=$dstDir`npath.rel=avd/$Target.avd`ntarget=android-36`n", [Text.UTF8Encoding]::new($false))
$sizeGB = [math]::Round(((Get-ChildItem -LiteralPath $dstDir -Recurse -File | Measure-Object Length -Sum).Sum) / 1GB, 2)
Write-Host "copied   $srcDir -> $dstDir ($sizeGB GB in $copySec s)"

# Each overlay must point at the clone's own backing file, never at the source's.
$qemuImg = Join-Path $SdkDir 'emulator\qemu-img.exe'
$backing = [ordered]@{}
foreach ($q in Get-ChildItem -LiteralPath $dstDir -Filter '*.qcow2') {
    $info = & $qemuImg info $q.FullName 2>&1 | Out-String
    if ($info -match 'actual path: ([^)]+)\)') {
        $actual = [IO.Path]::GetFullPath($Matches[1])
        if (-not $actual.StartsWith([IO.Path]::GetFullPath($dstDir), [StringComparison]::OrdinalIgnoreCase)) {
            throw "$($q.Name) is backed by $actual, outside the clone"
        }
        $backing[$q.Name] = $actual
    }
}

$result = [ordered]@{ source = $Source; target = $Target; copySeconds = $copySec; sizeGB = $sizeGB; backing = $backing }
if (-not $NoReset) {
    $netOff = 'restrict=on'
    # restrict=on alone does not isolate: Wi-Fi goes through netsimd, which ignores
    # it (identity/C-network-restrict-control.txt). With VirtioWifi off the guest
    # has only the restricted eth0 radio network and no route out.
    $bootArgs = @('-no-window', '-no-audio', '-no-snapshot', '-feature', '-VirtioWifi',
                  '-wifi-user-mode-options', $netOff, '-network-user-mode-options', $netOff)
    $netProbe = 'sleep 15; f=0; ping -c1 -W3 8.8.8.8 >/dev/null 2>&1 && f=1; (echo | toybox nc -w 4 8.8.8.8 53 >/dev/null 2>&1) && f=1; ' +
                '(echo | toybox nc -w 4 10.0.2.2 5137 >/dev/null 2>&1) && f=1; [ $f = 0 ] && echo "network isolated" || echo "NETWORK REACHABLE"'
    $serial = & (Join-Path $PSScriptRoot 'start-instance.ps1') $Target $ConsolePort -ExtraArgs $bootArgs
    $started = [datetime](Get-InstanceState $Target).started
    try {
        $result.firstBootSeconds = Wait-BootCompleted $serial $TimeoutSec $started
        Invoke-Adb -s $serial root | Out-Null; Start-Sleep -Seconds 2; Invoke-Adb -s $serial wait-for-device
        $result.networkCheck = (Invoke-Adb -s $serial shell $netProbe) -join ' '
        if ($result.networkCheck -cnotmatch 'network isolated') { throw "network is not restricted on $Target; refusing to reset with network" }
        $result.before = & (Join-Path $PSScriptRoot 'read-identity.ps1') $serial -App
        Invoke-Adb -s $serial shell 'settings delete secure android_id; sleep 3; stop; rm -f /data/system/users/0/settings_ssaid.xml; sync' | Write-Host
        # Not 'adb reboot': a guest reboot froze emulator 37.2.13 (qemu at 0 CPU,
        # console dead). Stop the emulator and cold boot it again instead.
        & (Join-Path $PSScriptRoot 'stop-instance.ps1') $Target -KeepAdb | Write-Host
        $serial = & (Join-Path $PSScriptRoot 'start-instance.ps1') $Target $ConsolePort -ExtraArgs $bootArgs
        $result.restartBootSeconds = Wait-BootCompleted $serial $TimeoutSec ([datetime](Get-InstanceState $Target).started)
        Invoke-Adb -s $serial root | Out-Null; Start-Sleep -Seconds 2; Invoke-Adb -s $serial wait-for-device
        $result.networkCheckAfter = (Invoke-Adb -s $serial shell $netProbe) -join ' '
        $result.after = & (Join-Path $PSScriptRoot 'read-identity.ps1') $serial -App
        # Per-app SSAID as the spike test app sees it (present only if it is installed).
        $result.compare = [ordered]@{
            android_id      = @($result.before['android_id'], $result.after['android_id'])
            appAndroidId    = @($result.before['app.ANDROID_ID'], $result.after['app.ANDROID_ID'])
            ssaidUserkey    = @($result.before['ssaid.userkey.sha256.afterAppRead'], $result.after['ssaid.userkey.sha256.afterAppRead'])
        }
    } finally {
        if (-not $KeepRunning) { & (Join-Path $PSScriptRoot 'stop-instance.ps1') $Target -KeepAdb | Write-Host }
    }
}
$dir = Join-Path $EvidenceDir 'identity'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$result | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $dir "clone-$Source-to-$Target.json")
$result
