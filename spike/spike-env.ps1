# Shared runtime helpers for the spike instance scripts. Dot-source this file.
# Every emulator and adb call runs with the Android env vars pointed into
# <repo>/.tmp, so nothing touches the user's own SDK, AVDs or adb server.

. (Join-Path $PSScriptRoot 'fetch-common.ps1')

$script:AvdDir      = Join-Path $Tmp 'avd'
$script:HomeRoot    = Join-Path $Tmp 'home'
$script:EvidenceDir = Join-Path $Tmp 'evidence'
$script:EmulatorExe = Join-Path $SdkDir 'emulator\emulator.exe'
$script:AdbExe      = Join-Path $SdkDir 'platform-tools\adb.exe'
# Emulator temp dir (TMP/TEMP): discovery files (avd/running/pid_*.ini with the
# gRPC token), crash db and netsimd artifacts land here instead of %TEMP%.
$script:EmuTempDir  = Join-Path $Tmp 'tmp'
$script:EmuLocalAppData = Join-Path $EmuTempDir 'localappdata'
# Discovery dirs to search: the isolated one first, then the stock location
# (used only if the emulator ignores the LOCALAPPDATA override).
$script:DiscoveryDirs = @((Join-Path $EmuLocalAppData 'Temp\avd\running'),
                          (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Temp\avd\running'))
$script:AdbServerPort = 5137   # never the default 5037: the user's adb server stays untouched
$script:ImageSysDir = 'system-images/android-36/default/x86_64/'

# Per-instance home: holds that instance's console auth token and emulator settings.
function Get-InstanceHome([string]$Name) { Join-Path $HomeRoot $Name }

# Points every Android env var into .tmp for this process and its children.
# $HomeDir is the emulator/user home: per instance for the emulator, 'adb' for the adb server.
function Use-SpikeEnv([string]$HomeDir) {
    New-Item -ItemType Directory -Force -Path $HomeDir, $AvdDir, $EvidenceDir | Out-Null
    $env:ANDROID_SDK_ROOT        = $SdkDir
    $env:ANDROID_HOME            = $SdkDir
    $env:ANDROID_AVD_HOME        = $AvdDir
    $env:ANDROID_EMULATOR_HOME   = $HomeDir
    $env:ANDROID_USER_HOME       = $HomeDir
    $env:ANDROID_ADB_SERVER_PORT = "$AdbServerPort"
    # Stop the adb server's automatic scan of 127.0.0.1:5555-5585: it would
    # otherwise attach to other emulators on this host (MuMu listens on 5555).
    # Spike emulators still appear: each one registers itself with this server.
    $env:ADB_EMU = '0'
}

# Runs adb against the isolated server (port 5137). Note: stock adb.exe on Windows
# still keeps its key pair in %USERPROFILE%\.android (it ignores ANDROID_USER_HOME).
function Invoke-Adb {
    Use-SpikeEnv (Join-Path $HomeRoot 'adb')
    & $AdbExe @args
}

# Console port N -> instance index (N-5600)/2 -> gRPC port 8600+index.
function Get-GrpcPort([int]$ConsolePort) {
    if ($ConsolePort -lt 5600 -or $ConsolePort % 2 -ne 0) { throw "console port must be even and >= 5600, got $ConsolePort" }
    8600 + ($ConsolePort - 5600) / 2
}

# The PID plus its descendants that run from <repo>/.tmp/sdk (emulator -> qemu and helpers).
function Get-OwnTree([int]$RootPid) {
    $all = Get-CimInstance Win32_Process
    $ids = [System.Collections.Generic.List[int]]::new(); $ids.Add($RootPid)
    for ($i = 0; $i -lt $ids.Count; $i++) {
        $all | Where-Object { $_.ParentProcessId -eq $ids[$i] -and $_.ExecutablePath -and
            $_.ExecutablePath.StartsWith($SdkDir, [StringComparison]::OrdinalIgnoreCase) } |
            ForEach-Object { if (-not $ids.Contains([int]$_.ProcessId)) { $ids.Add([int]$_.ProcessId) } }
    }
    $ids
}

function Get-InstanceState([string]$Name) {
    $f = Join-Path $EvidenceDir "$Name.instance.json"
    if (Test-Path -LiteralPath $f) { Get-Content -Raw -LiteralPath $f | ConvertFrom-Json -AsHashtable }
}

# Sends console commands after 'auth <token>' (token read from the instance home,
# never printed). Returns the console's replies as text.
function Invoke-EmuConsole([string]$Name, [int]$Port, [string[]]$Commands, [switch]$NoAuth) {
    $client = [Net.Sockets.TcpClient]::new()
    try {
        $client.Connect('127.0.0.1', $Port)
        $stream = $client.GetStream(); $stream.ReadTimeout = 3000
        $reader = [IO.StreamReader]::new($stream); $writer = [IO.StreamWriter]::new($stream)
        $writer.NewLine = "`r`n"; $writer.AutoFlush = $true
        $read = {
            $sb = [Text.StringBuilder]::new()
            try { while ($true) { $line = $reader.ReadLine(); if ($null -eq $line) { break }; [void]$sb.AppendLine($line); if ($line -match '^(OK|KO)') { break } } } catch [IO.IOException] { }
            $sb.ToString()
        }
        $out = & $read
        if (-not $NoAuth) {
            # The banner names the token file. Emulator 37.2.13 on Windows ignores
            # ANDROID_EMULATOR_HOME/USERPROFILE for it and always uses the Windows
            # profile folder, so accept only that file or one inside the instance home.
            if ($out -notmatch "(?s)auth_token> in\s*'([^']+)'") { throw "console banner names no token file" }
            $tokenFile = [IO.Path]::GetFullPath($Matches[1])
            $own = [IO.Path]::GetFullPath((Get-InstanceHome $Name)) + [IO.Path]::DirectorySeparatorChar
            $profileToken = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.emulator_console_auth_token'
            if (-not ($tokenFile.StartsWith($own, [StringComparison]::OrdinalIgnoreCase) -or $tokenFile -ieq $profileToken)) {
                throw "console token file $tokenFile is not one the emulator should use; refusing it"
            }
            $token = (Get-Content -Raw -LiteralPath $tokenFile).Trim()
            $writer.WriteLine("auth $token")
            $out += "> auth [redacted]`n" + (& $read)
        }
        foreach ($c in $Commands) { $writer.WriteLine($c); $out += "> $c`n" + (& $read) }
        $out
    } finally { $client.Dispose() }
}

# Waits until sys.boot_completed=1 on the serial; returns seconds waited.
function Wait-BootCompleted([string]$Serial, [int]$TimeoutSec = 300, [datetime]$Since = (Get-Date)) {
    while (((Get-Date) - $Since).TotalSeconds -lt $TimeoutSec) {
        $v = (Invoke-Adb -s $Serial shell getprop sys.boot_completed 2>$null | Out-String).Trim()
        if ($v -eq '1') { return [math]::Round(((Get-Date) - $Since).TotalSeconds, 1) }
        Start-Sleep -Milliseconds 500
    }
    throw "$Serial did not reach sys.boot_completed=1 within $TimeoutSec s"
}
