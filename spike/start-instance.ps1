<#
.SYNOPSIS
  Starts the stock emulator for one spike AVD with everything pointed into <repo>/.tmp.

.DESCRIPTION
  Console on -ConsolePort (even, >= 5600), adb on ConsolePort+1, gRPC on
  8600 + (ConsolePort-5600)/2 with a per-instance gRPC token (published in the
  discovery file under .tmp/tmp). The emulator home is per instance:
  .tmp/home/<Name>. The console auth token is NOT per instance: on Windows the
  stock emulator always reads it from %USERPROFILE%\.emulator_console_auth_token
  (Windows profile folder, no flag or env var moves it), shared by every instance.
  Writes .tmp/evidence/<Name>-emulator.log (+ .err.log), <Name>.pid and
  <Name>.instance.json. Refuses to start if the instance is already running or
  a needed port is taken: it never picks another port.
  Returns the adb serial.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory, Position = 0)][ValidatePattern('^[A-Za-z0-9._-]+$')][string]$Name,
    [Parameter(Mandatory, Position = 1)][int]$ConsolePort,
    [string[]]$ExtraArgs = @()
)

. (Join-Path $PSScriptRoot 'spike-env.ps1')

$grpcPort = Get-GrpcPort $ConsolePort
$adbPort = $ConsolePort + 1
if (-not (Test-Path -LiteralPath (Join-Path $AvdDir "$Name.ini"))) { throw "no AVD $Name in $AvdDir; run spike/new-avd.ps1 $Name" }

$state = Get-InstanceState $Name
if ($state -and (Get-Process -Id $state.pid -ErrorAction SilentlyContinue)) {
    throw "$Name already running as PID $($state.pid); stop it with spike/stop-instance.ps1 $Name"
}
$busy = Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -in @($ConsolePort, $adbPort, $grpcPort) }
if ($busy) {
    throw "port(s) $(($busy.LocalPort | Sort-Object -Unique) -join ',') already in use by PID(s) $(($busy.OwningProcess | Sort-Object -Unique) -join ','); stop that owner instead of changing ports"
}

$instHome = Get-InstanceHome $Name
Use-SpikeEnv $instHome

$log = Join-Path $EvidenceDir "$Name-emulator.log"
$errLog = Join-Path $EvidenceDir "$Name-emulator.err.log"
$emuArgs = @('-avd', $Name, '-port', $ConsolePort, '-grpc', $grpcPort, '-grpc-use-token',
             '-no-boot-anim', '-no-metrics', '-accel', 'on', '-gpu', 'host') + $ExtraArgs
# %TEMP% gets the crash db and netsimd artifacts; %LOCALAPPDATA%\Temp\avd\running
# gets the discovery file (with the gRPC token). Point both into .tmp for the
# child only, then restore. (The console token ignores every env var: see spike-env.ps1.)
New-Item -ItemType Directory -Force -Path $EmuTempDir, (Join-Path $EmuLocalAppData 'Temp') | Out-Null
$saved = @{}; foreach ($k in 'TMP', 'TEMP', 'LOCALAPPDATA') { $saved[$k] = [Environment]::GetEnvironmentVariable($k) }
try {
    $env:TMP = $EmuTempDir; $env:TEMP = $EmuTempDir; $env:LOCALAPPDATA = $EmuLocalAppData
    $started = Get-Date
    $p = Start-Process -FilePath $EmulatorExe -ArgumentList $emuArgs -PassThru `
            -RedirectStandardOutput $log -RedirectStandardError $errLog -WorkingDirectory $instHome
} finally {
    foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
}
Set-Content -LiteralPath (Join-Path $EvidenceDir "$Name.pid") -Value $p.Id -NoNewline
$serial = "emulator-$ConsolePort"
@{ name = $Name; pid = $p.Id; consolePort = $ConsolePort; adbPort = $adbPort; grpcPort = $grpcPort
   serial = $serial; started = $started.ToString('o'); args = $emuArgs } |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $EvidenceDir "$Name.instance.json")
Write-Host "started  $Name PID $($p.Id) console $ConsolePort adb $adbPort grpc $grpcPort log $log"
$serial
