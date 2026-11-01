<#
.SYNOPSIS
  Fetches the pinned Magisk release APK and extracts its x86_64 magisk binary
  to <repo>/.tmp/tools/magisk/magisk.

.DESCRIPTION
  Reads the "magisk" entry of spike/packages.pin.json, downloads the APK to
  .tmp/dl/magisk, verifies size and SHA-256, extracts the x86_64 binary into a
  new empty directory .tmp/dl/x-magisk, and copies it to .tmp/tools/magisk.
  A verified binary already in place is reused and nothing is downloaded.

  -UpdatePins pins the latest stable GitHub release (gh api, releases/latest),
  fetches it, and records the APK and binary hashes. Review the diff after.

  Magisk is GPL-3.0 and is fetched on this machine only; it is never copied
  into the repository.
#>
[CmdletBinding()]
param([switch]$UpdatePins)

. (Join-Path $PSScriptRoot 'fetch-common.ps1')

$Member  = 'lib/x86_64/libmagisk.so'
$ToolDir = Join-Path $Tmp 'tools\magisk'
$Binary  = Join-Path $ToolDir 'magisk'

function Install-Magisk($Pin) {
    $apk = Join-Path $DlDir ('magisk\' + $Pin.asset)
    $got = Get-VerifiedDownload $Pin.url $apk $Pin.size 'SHA256' $Pin.sha256
    $x = Join-Path $DlDir 'x-magisk'
    Expand-ToNewDir $apk $x @($Pin.member)
    $src = Join-Path $x ($Pin.member -replace '/', '\')
    if (-not (Test-Path -LiteralPath $src -PathType Leaf)) { throw "$($Pin.asset) has no $($Pin.member)" }
    New-Item -ItemType Directory -Force -Path $ToolDir | Out-Null
    Copy-Item -LiteralPath $src -Destination $Binary -Force
    Remove-Item -LiteralPath $x -Recurse -Force
    Write-Host "install  $($Pin.member) -> $Binary"
    @{ downloaded = $got; sha256 = (Get-FileHashHex $Binary 'SHA256') }
}

$pins = Read-Pins
if (-not $pins) { $pins = [ordered]@{ schema = 1 } }

if ($UpdatePins) {
    $rel = gh api repos/topjohnwu/Magisk/releases/latest | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'gh api failed' }
    if ($rel.prerelease -or $rel.draft) { throw "latest release $($rel.tag_name) is not stable" }
    $asset = $rel.assets | Where-Object { $_.name -eq "Magisk-$($rel.tag_name).apk" }
    if (-not $asset) { throw "no Magisk-$($rel.tag_name).apk in release" }
    if ($asset.digest -notmatch '^sha256:([0-9a-f]{64})$') { throw 'release asset has no sha256 digest' }
    $pin = [ordered]@{
        repo = 'topjohnwu/Magisk'; tag = $rel.tag_name; license = 'GPL-3.0'
        asset = $asset.name; url = $asset.browser_download_url; size = [long]$asset.size; sha256 = $Matches[1]
        member = $Member; binarySha256 = $null
    }
    $r = Install-Magisk $pin
    $pin.binarySha256 = $r.sha256
    $pins['magisk'] = $pin
    Write-Pins $pins
    return
}

if (-not $pins.Contains('magisk')) { throw "no magisk pin in $PinFile; run with -UpdatePins first" }
$pin = $pins.magisk
if ((Test-Path -LiteralPath $Binary) -and (Get-FileHashHex $Binary 'SHA256') -eq $pin.binarySha256) {
    Write-Host "reuse    $Binary (Magisk $($pin.tag), sha256 ok)"
    Write-Host 'done     downloads=0'
    return
}
$r = Install-Magisk $pin
if ($r.sha256 -ne $pin.binarySha256) { throw "extracted magisk sha256 $($r.sha256) differs from pin $($pin.binarySha256)" }
Write-Host "done     downloads=$([int]$r.downloaded)"
