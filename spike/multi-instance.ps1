<#
.SYNOPSIS
  Runs four spike instances at once from one shared read-only system image and
  records boot times, host RAM, commit charge, CPU idle, independent control,
  screenshots and per-instance identity. Everything lives in <repo>/.tmp.

.DESCRIPTION
  Steps (evidence goes to .tmp/evidence/multi/):
  1. Fetch the pinned packages and Magisk (reused when already verified) and
     create the AVDs with spike/new-avd.ps1 (-Cores, -RamMb; -Fresh recreates them).
  2. Fingerprint .tmp/sdk: every file and folder with size and mtime, plus
     SHA-256 of the system image files (sdk-before.json).
  3. Round 1: start every instance back to back, wait for sys.boot_completed=1
     on all of them, then sample host memory/CPU, list adb devices, set a
     different font_scale on each and read all of them back, apply a different
     identity to the first two with spike/apply-identity.ps1, read the model
     of every instance, and screenshot each one's Settings > About phone.
     After -SettleSec (a snapshot saved right after a boot or a framework
     restart can hang on load), sync and stop each instance gracefully, which
     saves its own Quick Boot snapshot.
  4. List each AVD folder (own overlays and snapshot, no image copy).
  5. Round 2: start all again (Quick Boot from each AVD's own snapshot), time
     the boots, read the models again, sample the host, stop all.
  6. Fingerprint .tmp/sdk again and diff it; stop the isolated adb server;
     check that no process runs from .tmp/sdk and that every port is closed.
  Writes multi-instance-<time>.json and .txt (summary). On any error it stops
  every instance it started.

  Console ports must be even and >= 5600 (adb = console+1, gRPC = 8600 +
  (console-5600)/2). The script never changes ports when one is taken.

.EXAMPLE
  pwsh spike/multi-instance.ps1 -Fresh
#>
[CmdletBinding()]
param(
    [ValidatePattern('^[A-Za-z0-9._-]+$')][string[]]$Names = @('m0', 'm1', 'm2', 'm3'),
    [int[]]$ConsolePorts = @(5630, 5640, 5650, 5660),
    [int]$Cores = 2,
    [int]$RamMb = 2048,
    [int]$SettleSec = 100,
    [int]$BootTimeoutSec = 600,
    [switch]$Fresh
)

. (Join-Path $PSScriptRoot 'spike-env.ps1')

if ($Names.Count -ne $ConsolePorts.Count) { throw 'give one console port per name' }
if ($Names.Count -lt 2) { throw 'need at least two instances (identity check uses the first two)' }
$OutDir = Join-Path $EvidenceDir 'multi'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$Summary = [ordered]@{ started = (Get-Date).ToString('o'); names = $Names; consolePorts = $ConsolePorts
                       cores = $Cores; ramMb = $RamMb }
$Inst = [ordered]@{}
for ($i = 0; $i -lt $Names.Count; $i++) {
    $cp = $ConsolePorts[$i]
    $Inst[$Names[$i]] = [ordered]@{ name = $Names[$i]; consolePort = $cp; adbPort = $cp + 1
                                    grpcPort = (Get-GrpcPort $cp); serial = "emulator-$cp" }
}
$AllPorts = @($AdbServerPort) + @($Inst.Values | ForEach-Object { $_.consolePort, $_.adbPort, $_.grpcPort })
if (($AllPorts | Sort-Object -Unique).Count -ne $AllPorts.Count) { throw "ports overlap: $($AllPorts -join ',')" }

# Sum of one numeric property in MB (0 for no objects; Measure-Object has no .Sum then under StrictMode).
function Get-SumMB($Objects, [string]$Property) { $t = 0; foreach ($o in $Objects) { if ($null -ne $o) { $t += $o.$Property } }; [int]($t / 1MB) }
function Save-Text([string]$File, $Text) { Set-Content -LiteralPath (Join-Path $OutDir $File) -Value $Text }
function Get-Listening([int[]]$Ports) {
    @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -in $Ports } |
      ForEach-Object { "$($_.LocalAddress):$($_.LocalPort) pid $($_.OwningProcess)" })
}
function Get-SdkProcs {
    @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and
        $_.ExecutablePath.StartsWith($SdkDir, [StringComparison]::OrdinalIgnoreCase) })
}

# Every file and folder under .tmp/sdk with size and mtime; SHA-256 of the image files.
function Get-SdkFingerprint {
    $map = [ordered]@{}
    foreach ($it in Get-ChildItem -LiteralPath $SdkDir -Recurse -Force) {
        $rel = [IO.Path]::GetRelativePath($SdkDir, $it.FullName) -replace '\\', '/'
        $map[$rel] = if ($it.PSIsContainer) { "dir mtime=$($it.LastWriteTimeUtc.ToString('o'))" }
                     else { "len=$($it.Length) mtime=$($it.LastWriteTimeUtc.ToString('o'))" }
    }
    foreach ($f in Get-ChildItem -LiteralPath (Join-Path $SdkDir $ImageSysDir) -File -Recurse) {
        $rel = [IO.Path]::GetRelativePath($SdkDir, $f.FullName) -replace '\\', '/'
        $map[$rel] += " sha256=$(Get-FileHashHex $f.FullName SHA256)"
    }
    $map
}

# Host memory, commit charge, CPU idle (average over $CpuSec one-second samples)
# and the working set / private bytes of every process running from .tmp/sdk.
function Get-HostSample([string]$Label, [int]$CpuSec = 10) {
    $c = (Get-Counter '\Memory\Committed Bytes', '\Memory\Commit Limit').CounterSamples
    $idle = (Get-Counter '\Processor(_Total)\% Idle Time' -SampleInterval 1 -MaxSamples $CpuSec).CounterSamples |
            Measure-Object CookedValue -Average
    $os = Get-CimInstance Win32_OperatingSystem
    $procs = @(Get-SdkProcs | ForEach-Object { Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue })
    $per = [ordered]@{}
    foreach ($n in $Inst.Keys) {
        $st = Get-InstanceState $n
        if (-not $st -or $st.ContainsKey('stopped') -or -not (Get-Process -Id $st.pid -ErrorAction SilentlyContinue)) { continue }
        $tree = @(Get-OwnTree $st.pid | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })
        $per[$n] = [ordered]@{ pids = @($tree.Id); names = @($tree.ProcessName)
            workingSetMB = (Get-SumMB $tree 'WorkingSet64')
            privateMB = (Get-SumMB $tree 'PrivateMemorySize64') }
    }
    [ordered]@{
        label = $Label; time = (Get-Date).ToString('o')
        physTotalMB = [int]($os.TotalVisibleMemorySize / 1KB); physUsedMB = [int](($os.TotalVisibleMemorySize - $os.FreePhysicalMemory) / 1KB)
        committedGB = [math]::Round(($c | Where-Object Path -like '*committed bytes').CookedValue / 1GB, 2)
        commitLimitGB = [math]::Round(($c | Where-Object Path -like '*commit limit').CookedValue / 1GB, 2)
        cpuIdlePct = [math]::Round($idle.Average, 1); cpuIdleSeconds = $CpuSec
        sdkProcessCount = $procs.Count
        sdkWorkingSetMB = (Get-SumMB $procs 'WorkingSet64')
        sdkPrivateMB = (Get-SumMB $procs 'PrivateMemorySize64')
        perInstance = $per
    }
}

# Starts every instance back to back, then waits for sys.boot_completed=1 on all.
function Start-All([string]$Round) {
    foreach ($n in $Inst.Keys) {
        $avd = Join-Path $AvdDir "$n.avd"
        $Inst[$n]["$Round.hadSnapshot"] = Test-Path -LiteralPath (Join-Path $avd 'snapshots\default_boot\snapshot.pb')
        $Inst[$n]["$Round.hadUserdata"] = Test-Path -LiteralPath (Join-Path $avd 'userdata-qemu.img.qcow2')
        $serial = & (Join-Path $PSScriptRoot 'start-instance.ps1') $n $Inst[$n].consolePort
        $st = Get-InstanceState $n
        if ($serial -ne $Inst[$n].serial) { throw "unexpected serial $serial for $n" }
        $Inst[$n]["$Round.pid"] = $st.pid; $Inst[$n]["$Round.started"] = $st.started
    }
    $pending = [System.Collections.Generic.List[string]]::new([string[]]@($Inst.Keys))
    $t0 = Get-Date
    while ($pending.Count) {
        if (((Get-Date) - $t0).TotalSeconds -gt $BootTimeoutSec) { throw "not booted in $BootTimeoutSec s: $($pending -join ',')" }
        foreach ($n in @($pending)) {
            if (-not (Get-Process -Id $Inst[$n]["$Round.pid"] -ErrorAction SilentlyContinue)) { throw "$n exited during boot (see $n-emulator.err.log)" }
            $v = (Invoke-Adb -s $Inst[$n].serial shell getprop sys.boot_completed 2>$null | Out-String).Trim()
            if ($v -eq '1') {
                $secs = [math]::Round(((Get-Date) - [datetime]$Inst[$n]["$Round.started"]).TotalSeconds, 1)
                $Inst[$n]["$Round.bootSeconds"] = $secs; [void]$pending.Remove($n)
                Write-Host "booted   $n ($($Inst[$n].serial)) $Round in $secs s"
            }
        }
        if ($pending.Count) { Start-Sleep -Milliseconds 500 }
    }
}

function Stop-All([string]$Round) {
    foreach ($n in $Inst.Keys) {
        $st = Get-InstanceState $n
        if (-not $st -or -not (Get-Process -Id $st.pid -ErrorAction SilentlyContinue)) { continue }
        $out = & (Join-Path $PSScriptRoot 'stop-instance.ps1') $n -KeepAdb *>&1 | Out-String
        Save-Text "$n-stop-$Round.txt" $out
        $Inst[$n]["$Round.stopGraceful"] = $out -notmatch 'taskkill'
        # Keep this round's emulator log; its own "Boot completed in N ms" cross-checks the adb timing.
        $log = Join-Path $OutDir "$n-emulator-$Round.log"
        Copy-Item -LiteralPath (Join-Path $EvidenceDir "$n-emulator.log") -Destination $log
        $boot = Select-String -LiteralPath $log -Pattern 'Boot completed in (\d+) ms' | Select-Object -First 1
        $Inst[$n]["$Round.emulatorBootMs"] = if ($boot) { [int]$boot.Matches[0].Groups[1].Value } else { $null }
        $Inst[$n]["$Round.snapshotLog"] = @(Select-String -LiteralPath $log -Pattern "snapshot 'default_boot'" |
            ForEach-Object { $_.Line.Trim() })
    }
}

function Get-Models {
    $m = [ordered]@{}
    foreach ($n in $Inst.Keys) {
        $s = $Inst[$n].serial
        $m[$n] = [ordered]@{
            model = (Invoke-Adb -s $s shell getprop ro.product.model | Out-String).Trim()
            brand = (Invoke-Adb -s $s shell getprop ro.product.brand | Out-String).Trim()
            fingerprint = (Invoke-Adb -s $s shell getprop ro.build.fingerprint | Out-String).Trim()
            serialno = (Invoke-Adb -s $s shell getprop ro.serialno | Out-String).Trim()
            deviceName = (Invoke-Adb -s $s shell settings get global device_name | Out-String).Trim()
        }
    }
    $m
}

try {
    # 1. Packages, Magisk, AVDs.
    & (Join-Path $PSScriptRoot 'fetch-packages.ps1')
    & (Join-Path $PSScriptRoot 'fetch-magisk.ps1')
    foreach ($n in $Inst.Keys) {
        $st = Get-InstanceState $n
        if ($st -and (Get-Process -Id $st.pid -ErrorAction SilentlyContinue)) { throw "$n is already running (PID $($st.pid))" }
        & (Join-Path $PSScriptRoot 'new-avd.ps1') $n -Cores $Cores -RamMb $RamMb -Force:$Fresh
        $cfg = Get-Content -LiteralPath (Join-Path $AvdDir "$n.avd\config.ini")
        if (-not ($cfg -contains "hw.cpu.ncore=$Cores" -and $cfg -contains "hw.ramSize=$RamMb")) {
            throw "$n exists with other cores/RAM; rerun with -Fresh"
        }
    }
    $busy = Get-Listening $AllPorts
    if ($busy) { throw "ports already in use: $($busy -join '; '); stop that owner instead of changing ports" }

    # 2. SDK fingerprint and host baseline.
    $sdkBefore = Get-SdkFingerprint
    $sdkBefore | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $OutDir 'sdk-before.json')
    $Summary.hostStateFile = & (Join-Path $PSScriptRoot 'record-host-state.ps1') -Label 'multi-instance start'
    $samples = [System.Collections.Generic.List[object]]::new()
    $samples.Add((Get-HostSample 'baseline (no instance)'))
    $need = $Names.Count * ($RamMb + 1024)
    $free = $samples[0].physTotalMB - $samples[0].physUsedMB
    if ($free -lt $need) { Write-Warning "only $free MB physical RAM free, about $need MB wanted" }
    Invoke-Adb start-server 2>&1 | Out-Null

    # 3. Round 1.
    Start-All 'round1'
    $samples.Add((Get-HostSample 'round1 all booted'))
    Save-Text 'adb-devices-round1.txt' (Invoke-Adb devices -l | Out-String)

    # Independent control: a different font_scale per instance (visible in the
    # screenshots), set on all first, then read all back.
    $scales = @('1.0', '1.15', '1.3', '0.85', '1.1', '1.2', '0.9', '1.25')
    $i = 0
    foreach ($n in $Inst.Keys) {
        $Inst[$n].controlValue = $scales[$i++ % $scales.Count]
        Invoke-Adb -s $Inst[$n].serial shell settings put system font_scale $Inst[$n].controlValue | Out-Null
    }
    $control = [ordered]@{}
    foreach ($n in $Inst.Keys) {
        $v = (Invoke-Adb -s $Inst[$n].serial shell settings get system font_scale | Out-String).Trim()
        $control[$n] = [ordered]@{ setting = 'system font_scale'; set = $Inst[$n].controlValue; readBack = $v
                                   ok = ([double]$v -eq [double]$Inst[$n].controlValue) }
    }
    $Summary.control = $control

    # Identity: a different model on the first two instances.
    $Summary.modelsBefore = Get-Models
    $apply = [ordered]@{}
    foreach ($k in 0, 1) {
        $n = $Names[$k]
        $tag = [char](65 + $k)
        $idFile = Join-Path $OutDir "identity-$n.json"
        [ordered]@{ model = "MTP-Multi-$tag"; brand = "MTPBrand$tag"; device = "mtpmulti$k"; manufacturer = "MTPMaker$tag"
                    name = "mtpmulti$k"; fingerprint = "MTPBrand$tag/mtpmulti$k/mtpmulti${k}:16/MTP1.000/010${k}:user/release-keys"
                    serialno = "MTPM0000000$k"; deviceName = "MTP-Multi-$tag" } |
            ConvertTo-Json | Set-Content -LiteralPath $idFile
        $r = @(& (Join-Path $PSScriptRoot 'apply-identity.ps1') $n $idFile)[-1]
        $apply[$n] = [ordered]@{ identityFile = $idFile; applied = $r.applied; mismatched = @($r.mismatched)
                                 frameworkRestartSeconds = $r.frameworkRestartSeconds }
    }
    $Summary.identityApply = $apply
    $after = Get-Models
    $Summary.modelsAfter = $after
    $n0, $n1 = $Names[0], $Names[1]
    $Summary.identityDiffer = ($after[$n0].model -eq 'MTP-Multi-A' -and $after[$n1].model -eq 'MTP-Multi-B' -and
                               $after[$n0].fingerprint -ne $after[$n1].fingerprint -and $after[$n0].serialno -ne $after[$n1].serialno)
    $Summary.othersUnchanged = @($Names | Select-Object -Skip 2 | Where-Object {
        $after[$_].model -ne $Summary.modelsBefore[$_].model }).Count -eq 0

    # Screenshot of each on Settings > About phone: shows its device name and
    # model, in its own font scale. Then back to the home screen.
    foreach ($n in $Inst.Keys) {
        $s = $Inst[$n].serial
        Invoke-Adb -s $s shell 'input keyevent KEYCODE_WAKEUP; wm dismiss-keyguard; am start -W -a android.settings.DEVICE_INFO_SETTINGS >/dev/null; sleep 3; screencap -p /data/local/tmp/multi-shot.png' | Out-Null
        $png = Join-Path $OutDir "$n-screen.png"
        Invoke-Adb -s $s pull /data/local/tmp/multi-shot.png $png | Out-Null
        Invoke-Adb -s $s shell 'rm /data/local/tmp/multi-shot.png; input keyevent KEYCODE_HOME' | Out-Null
        $Inst[$n].screenshot = $png
        $Inst[$n].screenshotSha256 = (Get-FileHashHex $png SHA256).Substring(0, 16)
    }
    $Summary.screenshotsDistinct = @($Inst.Values | ForEach-Object { $_.screenshotSha256 } | Sort-Object -Unique).Count -eq $Inst.Count

    Write-Host "settle   $SettleSec s before saving the Quick Boot snapshots"
    Start-Sleep -Seconds $SettleSec
    $samples.Add((Get-HostSample "round1 steady state (+$SettleSec s after identity)"))
    foreach ($n in $Inst.Keys) { Invoke-Adb -s $Inst[$n].serial shell sync | Out-Null }
    Stop-All 'round1'

    # 4. AVD folder contents: own overlays and snapshot, no copy of the image.
    $avdFiles = [ordered]@{}
    foreach ($n in $Inst.Keys) {
        $avd = Join-Path $AvdDir "$n.avd"
        $files = Get-ChildItem -LiteralPath $avd -Recurse -File -Force
        $avdFiles[$n] = [ordered]@{
            totalMB = (Get-SumMB $files 'Length')
            topLevel = @(Get-ChildItem -LiteralPath $avd -Force | ForEach-Object {
                if ($_.PSIsContainer) { "$($_.Name)/ $(Get-SumMB (Get-ChildItem -LiteralPath $_.FullName -Recurse -File -Force) 'Length') MB" }
                else { "$($_.Name) $([math]::Round($_.Length / 1MB, 1)) MB" } })
            snapshot = @(Get-ChildItem -LiteralPath (Join-Path $avd 'snapshots\default_boot') -File -ErrorAction SilentlyContinue |
                         ForEach-Object { "$($_.Name) $([math]::Round($_.Length / 1MB, 1)) MB" })
            imageCopies = @($files | Where-Object { $_.Name -match '^(system|vendor|ramdisk|kernel)' } | ForEach-Object { $_.Name })
            imagePaths = @(Get-Content -LiteralPath (Join-Path $avd 'hardware-qemu.ini') |
                           Where-Object { $_ -match '(systemPartition|vendorPartition|kernel\.path|ramdisk\.path)' })
        }
    }
    $Summary.avdFiles = $avdFiles

    # 5. Round 2: Quick Boot from each AVD's own snapshot.
    Start-All 'round2'
    $samples.Add((Get-HostSample 'round2 all booted (Quick Boot)'))
    $Summary.modelsRound2 = Get-Models
    Stop-All 'round2'
}
finally {
    # Stop anything still running from this run, then the isolated adb server.
    foreach ($n in $Inst.Keys) {
        $st = Get-InstanceState $n
        if ($st -and (Get-Process -Id $st.pid -ErrorAction SilentlyContinue)) {
            Write-Host "cleanup  stopping $n"
            & (Join-Path $PSScriptRoot 'stop-instance.ps1') $n -KeepAdb *>&1 | Out-Host
        }
    }
    Invoke-Adb kill-server 2>&1 | Out-Null
    Start-Sleep -Seconds 3
}

# 6. SDK diff, processes and ports.
$sdkAfter = Get-SdkFingerprint
$sdkAfter | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $OutDir 'sdk-after.json')
$Summary.sdkDiff = [ordered]@{
    files = $sdkBefore.Count
    added = @($sdkAfter.Keys | Where-Object { -not $sdkBefore.Contains($_) })
    removed = @($sdkBefore.Keys | Where-Object { -not $sdkAfter.Contains($_) })
    changed = @($sdkBefore.Keys | Where-Object { $sdkAfter.Contains($_) -and $sdkAfter[$_] -ne $sdkBefore[$_] })
}
$Summary.hostSamples = $samples
$Summary.instances = $Inst
$Summary.leftSdkProcesses = @(Get-SdkProcs | ForEach-Object { "$($_.ProcessId) $($_.Name)" })
$Summary.leftNamedProcesses = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -match '^(qemu-system|emulator|adb|netsimd)' } |
                                ForEach-Object { "$($_.Id) $($_.ProcessName) $($_.Path)" })
$Summary.listeningAfter = @(Get-Listening $AllPorts)
$Summary.finished = (Get-Date).ToString('o')

$json = Join-Path $OutDir "multi-instance-$stamp.json"
$Summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $json
$lines = @("multi-instance run $stamp ($($Names.Count) x $Cores cores / $RamMb MB)")
foreach ($n in $Inst.Keys) {
    $x = $Inst[$n]
    $lines += ("{0} {1} console {2} adb {3} grpc {4}: round1 {5} s (emulator log {6} ms, fresh userdata: {7}), " +
               "round2 Quick Boot {8} s (emulator log {9} ms); font_scale {10} read back {11}; graceful stop {12}/{13}") -f `
        $n, $x.serial, $x.consolePort, $x.adbPort, $x.grpcPort, $x['round1.bootSeconds'], $x['round1.emulatorBootMs'],
        (-not $x['round1.hadUserdata']), $x['round2.bootSeconds'], $x['round2.emulatorBootMs'], $control[$n].set,
        $control[$n].readBack, $x['round1.stopGraceful'], $x['round2.stopGraceful']
}
foreach ($s in $samples) {
    $lines += "host [{0}] physUsed {1}/{2} MB, commit {3}/{4} GB, CPU idle {5}% ({6} s avg), sdk procs {7}: WS {8} MB private {9} MB" -f `
        $s.label, $s.physUsedMB, $s.physTotalMB, $s.committedGB, $s.commitLimitGB, $s.cpuIdlePct, $s.cpuIdleSeconds,
        $s.sdkProcessCount, $s.sdkWorkingSetMB, $s.sdkPrivateMB
}
foreach ($n in $Inst.Keys) {
    $lines += "model {0}: before '{1}', after '{2}' (device_name '{3}'), round2 '{4}'" -f $n, $Summary.modelsBefore[$n].model,
        $after[$n].model, $after[$n].deviceName, $Summary.modelsRound2[$n].model
}
$lines += "screenshots distinct: $($Summary.screenshotsDistinct)"
$lines += "identity differ (first two): $($Summary.identityDiffer); others unchanged: $($Summary.othersUnchanged)"
$lines += "sdk diff: $($Summary.sdkDiff.files) entries, added $($Summary.sdkDiff.added.Count), removed $($Summary.sdkDiff.removed.Count), changed $($Summary.sdkDiff.changed.Count)"
$lines += "left: sdk processes $($Summary.leftSdkProcesses.Count), named processes $($Summary.leftNamedProcesses.Count), listening ports $($Summary.listeningAfter.Count)"
$txt = Join-Path $OutDir "multi-instance-$stamp.txt"
$lines | Set-Content -LiteralPath $txt
$lines | Write-Host
Write-Host "wrote    $json"
$Summary
