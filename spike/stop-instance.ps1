<#
.SYNOPSIS
  Stops one spike instance cleanly, touching only the processes that instance started.

.DESCRIPTION
  Reads .tmp/evidence/<Name>.instance.json. Records the emulator PID and its
  descendant processes (only those running from <repo>/.tmp/sdk), sends 'kill'
  on the authenticated console (falling back to 'adb emu kill'), waits for a
  graceful exit (which saves the Quick Boot snapshot), and only then uses
  taskkill on those recorded PIDs. netsimd (the emulator's network daemon) is
  shared by every running instance, whichever started it: while another spike
  instance runs it is left alone; after the last one it gets the same graceful
  wait. -KeepAdb leaves the isolated adb server (port 5137) running; otherwise
  it is stopped too when no instance remains.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory, Position = 0)][ValidatePattern('^[A-Za-z0-9._-]+$')][string]$Name,
    [int]$TimeoutSec = 90,
    [switch]$KeepAdb
)

. (Join-Path $PSScriptRoot 'spike-env.ps1')

function Get-Alive([int[]]$Ids) { ,@($Ids | Where-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue }) }
# Waits up to $Sec for the PIDs to exit, then taskkill, then taskkill /F.
function Stop-Leftover([int[]]$Ids, [int]$Sec) {
    $deadline = (Get-Date).AddSeconds($Sec)
    while ((Get-Alive $Ids).Count -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
    $left = Get-Alive $Ids
    if ($left.Count) {
        Write-Host "taskkill $($left -join ',') (still running)"
        foreach ($id in $left) { taskkill /PID $id 2>&1 | Out-Null }
        Start-Sleep -Seconds 5
        foreach ($id in (Get-Alive $left)) { taskkill /F /PID $id 2>&1 | Out-Null }
    }
}
# The emulator spawns "emulator -kill <qemu pid> -sleep N" watchdogs on shutdown; they are ours too.
function Get-Watchdogs([int[]]$Ids) {
    Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and
        $_.ExecutablePath.StartsWith($SdkDir, [StringComparison]::OrdinalIgnoreCase) -and
        $_.CommandLine -match '-kill (\d+)' -and [int]$Matches[1] -in $Ids } | ForEach-Object { [int]$_.ProcessId }
}

$state = Get-InstanceState $Name
if (-not $state) { Write-Host "no state for $Name; nothing to stop"; return }
$main = Get-Process -Id $state.pid -ErrorAction SilentlyContinue
if ($main -and -not ($main.Path -and $main.Path.StartsWith($SdkDir, [StringComparison]::OrdinalIgnoreCase))) {
    $what = if ($main.Path) { $main.Path } else { "$($main.ProcessName) (path not readable)" }
    throw "PID $($state.pid) is $what, not our emulator; refusing to touch it"
}

# Other spike instances still running (PID alive and running from .tmp/sdk).
$others = @(Get-ChildItem -LiteralPath $EvidenceDir -Filter '*.instance.json' | ForEach-Object {
    $s = Get-Content -Raw -LiteralPath $_.FullName | ConvertFrom-Json
    if ($s.name -ne $Name -and -not $s.PSObject.Properties['stopped'] -and (Test-OurPid $s.pid)) { $s.name }
})

if ($main) {
    $tree = @(Get-OwnTree $state.pid)
    if ($others.Count) {
        $shared = @($tree | Where-Object { (Get-Process -Id $_ -ErrorAction SilentlyContinue).ProcessName -eq 'netsimd' })
        if ($shared.Count) {
            $tree = @($tree | Where-Object { $_ -notin $shared })
            Write-Host "netsimd  PID $($shared -join ',') is shared; left running for $($others -join ',')"
        }
    }
    Write-Host "stopping $Name tree PIDs $($tree -join ',')"
    $sent = $false
    try {
        $reply = Invoke-EmuConsole $Name $state.consolePort @('kill')
        $sent = $reply -match '(?m)^> kill'
        Write-Host "console  kill sent"
    } catch { Write-Host "console  kill failed: $($_.Exception.Message); trying adb emu kill" }
    if (-not $sent) { Invoke-Adb -s $state.serial emu kill 2>&1 | Write-Host }

    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        Start-Sleep -Milliseconds 500
        foreach ($w in @(Get-Watchdogs $tree)) { if ($w -notin $tree) { $tree += $w } }
    } while ((Get-Alive $tree).Count -and (Get-Date) -lt $deadline)
    Write-Host "exited   tree PIDs $($tree -join ',')"
    Stop-Leftover $tree 0
    Write-Host "stopped  $Name"
} else {
    Write-Host "$Name (PID $($state.pid)) is not running"
}
$state.stopped = (Get-Date).ToString('o')
$state | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $EvidenceDir "$Name.instance.json")

if (-not $others.Count) {
    # Last instance: a netsimd started by an earlier (already stopped) instance is ours too.
    $netsim = @(Get-CimInstance Win32_Process -Filter "Name = 'netsimd.exe'" | Where-Object { $_.ExecutablePath -and
        $_.ExecutablePath.StartsWith($SdkDir, [StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { [int]$_.ProcessId })
    if ($netsim.Count) { Write-Host "netsimd  waiting for PID $($netsim -join ',')"; Stop-Leftover $netsim 30 }
}

if (-not $KeepAdb) {
    if (-not $others.Count) { Invoke-Adb kill-server 2>&1 | Out-Null; Write-Host "adb      isolated server on $AdbServerPort stopped" }
}
