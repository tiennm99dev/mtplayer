<#
.SYNOPSIS
  Creates an AVD under <repo>/.tmp/avd by writing its .ini files by hand (no avdmanager).

.DESCRIPTION
  Writes .tmp/avd/<Name>.ini and .tmp/avd/<Name>.avd/config.ini for the stock
  system-images;android-36;default;x86_64 image. The image itself is never
  copied or changed: the emulator creates the AVD's own writable files
  (userdata-qemu.img, qcow2 overlays, snapshots) inside <Name>.avd on first boot.

  -Cores and -RamMb set hw.cpu.ncore and hw.ramSize (defaults: 4 cores, 2048 MB).
  -Force replaces an existing AVD of that name (deletes its <Name>.avd folder).
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9._-]+$')][string]$Name,
    [ValidateRange(1, 16)][int]$Cores = 4,
    [ValidateRange(1024, 16384)][int]$RamMb = 2048,
    [switch]$Force
)

. (Join-Path $PSScriptRoot 'spike-env.ps1')

$imageDir = Join-Path $SdkDir $ImageSysDir
if (-not (Test-Path -LiteralPath (Join-Path $imageDir 'system.img'))) {
    throw "system image missing at $imageDir; run spike/fetch-packages.ps1 first"
}

New-Item -ItemType Directory -Force -Path $AvdDir | Out-Null
$iniFile = Join-Path $AvdDir "$Name.ini"
$avdPath = Join-Path $AvdDir "$Name.avd"
if ((Test-Path -LiteralPath $iniFile) -or (Test-Path -LiteralPath $avdPath)) {
    if (-not $Force) { Write-Host "exists   $avdPath (use -Force to recreate)"; return }
    Remove-Item -LiteralPath $iniFile -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $avdPath -Recurse -Force -ErrorAction SilentlyContinue
}
New-Item -ItemType Directory -Path $avdPath | Out-Null

function Write-Ini([string]$Path, [System.Collections.Specialized.OrderedDictionary]$Map) {
    $text = ($Map.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join "`n"
    [IO.File]::WriteAllText($Path, $text + "`n", [Text.UTF8Encoding]::new($false))
    Write-Host "wrote    $Path"
}

Write-Ini $iniFile ([ordered]@{
    'avd.ini.encoding' = 'UTF-8'
    'path'             = $avdPath
    'path.rel'         = "avd/$Name.avd"
    'target'           = 'android-36'
})

Write-Ini (Join-Path $avdPath 'config.ini') ([ordered]@{
    'avd.ini.encoding'        = 'UTF-8'
    'AvdId'                   = $Name
    'avd.ini.displayname'     = $Name
    'image.sysdir.1'          = $ImageSysDir
    'tag.id'                  = 'default'
    'tag.display'             = 'Default Android System Image'
    'abi.type'                = 'x86_64'
    'hw.cpu.arch'             = 'x86_64'
    'hw.cpu.ncore'            = "$Cores"
    'hw.ramSize'              = "$RamMb"
    'hw.gpu.enabled'          = 'yes'
    'hw.gpu.mode'             = 'host'
    'disk.dataPartition.size' = '6G'
    'hw.keyboard'             = 'yes'
    'hw.lcd.width'            = '1080'
    'hw.lcd.height'           = '2400'
    'hw.lcd.density'          = '420'
    'skin.name'               = '1080x2400'
    'skin.dynamic'            = 'yes'
    'showDeviceFrame'         = 'no'
    'PlayStore.enabled'       = 'false'
    'hw.sdCard'               = 'no'
    'fastboot.forceColdBoot'  = 'no'
})
