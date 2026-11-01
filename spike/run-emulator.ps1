<#
.SYNOPSIS
  One command to get a spike emulator running: fetch if needed, create the AVD if
  needed, start it, wait for boot, and print the adb serial.

.DESCRIPTION
  Everything lives in <repo>/.tmp. Running this means you accept the Android SDK
  License for your own machine (see spike/fetch-packages.ps1).
  Use the isolated adb server afterwards, for example:
    $env:ANDROID_ADB_SERVER_PORT = 5137; $env:ADB_EMU = 0
    .tmp\sdk\platform-tools\adb.exe -s emulator-5600 shell
  Stop it with: spike/stop-instance.ps1 <Name>
  Run it from a terminal or redirect to a file: the emulator and adb server inherit
  the caller's handles, so a pipe (run-emulator.ps1 | Tee-Object ...) stays open
  until the instance is stopped even though the script itself has returned.

.EXAMPLE
  pwsh spike/run-emulator.ps1                 # spike0 on console 5600
  pwsh spike/run-emulator.ps1 spike1 5602     # a second instance
#>
[CmdletBinding()]
param(
    [Parameter(Position = 0)][ValidatePattern('^[A-Za-z0-9._-]+$')][string]$Name = 'spike0',
    [Parameter(Position = 1)][int]$ConsolePort = 5600,
    [int]$TimeoutSec = 300
)

. (Join-Path $PSScriptRoot 'spike-env.ps1')

& (Join-Path $PSScriptRoot 'fetch-packages.ps1')
& (Join-Path $PSScriptRoot 'new-avd.ps1') $Name

$state = Get-InstanceState $Name
if ($state -and (Get-Process -Id $state.pid -ErrorAction SilentlyContinue)) {
    if ($state.consolePort -ne $ConsolePort) { throw "$Name is already running on console $($state.consolePort)" }
    Write-Host "running  $Name already (PID $($state.pid))"
    $serial = $state.serial; $since = Get-Date
} else {
    $serial = & (Join-Path $PSScriptRoot 'start-instance.ps1') $Name $ConsolePort
    $since = [datetime](Get-InstanceState $Name).started
}
$secs = Wait-BootCompleted $serial $TimeoutSec $since
Write-Host "booted   $Name in $secs s (sys.boot_completed=1); adb server port $AdbServerPort"
$serial
