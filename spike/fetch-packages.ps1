<#
.SYNOPSIS
  Fetches the pinned Google SDK packages into <repo>/.tmp/sdk in the standard SDK layout.

.DESCRIPTION
  Reads spike/packages.pin.json. For every package that is not already installed
  from the pinned zip, it downloads Google's repository XML, checks that the pin
  still matches it (channel-0, uses-license android-sdk-license, URL, size,
  SHA-1), downloads the zip to .tmp/dl, verifies size and SHA-1, extracts it into
  its own new empty directory .tmp/dl/x-<name>, and moves the result into
  .tmp/sdk. It records the android-sdk-license acceptance as the SHA-1 of the
  XML license text node in .tmp/sdk/licenses/android-sdk-license. That hash is
  used to detect changes to the license text. It differs from the hash Android
  Studio writes for the same text, so sdkmanager may not recognize this file.

  A second run with everything in place downloads nothing.

  -UpdatePins rewrites the package pins from the latest stable (channel-0)
  revisions in Google's XML, then exits. Review the diff before using it.

  The packages are Google's, under the Android SDK License. Running this script
  means you accept that license for your own machine. Nothing is copied into
  the repository and no Google file is changed.
#>
[CmdletBinding()]
param([switch]$UpdatePins)

. (Join-Path $PSScriptRoot 'fetch-common.ps1')

$RepoBase = 'https://dl.google.com/android/repository/'
$Wanted = @(
    @{ path = 'emulator';                                xml = 'repository2-3.xml';              probe = 'emulator.exe' }
    @{ path = 'platform-tools';                          xml = 'repository2-3.xml';              probe = 'adb.exe' }
    @{ path = 'system-images;android-36;default;x86_64'; xml = 'sys-img/android/sys-img2-3.xml'; probe = 'system.img' }
    # Only for the spike's identity test app (spike/make-test-app.ps1); the product ships neither.
    @{ path = 'build-tools;36.1.0';                      xml = 'repository2-3.xml';              probe = 'aapt2.exe' }
    @{ path = 'platforms;android-36';                    xml = 'repository2-3.xml';              probe = 'android.jar' }
)
$LicenseId = 'android-sdk-license'
$XmlCache = @{}

function Get-RepoXml([string]$Rel) {
    if (-not $XmlCache.ContainsKey($Rel)) {
        $dest = Join-Path $DlDir ('xml\' + ($Rel -replace '/', '_'))
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $dest) | Out-Null
        & $CurlExe --fail --location --silent --show-error --retry 3 --output $dest ($RepoBase + $Rel)
        if ($LASTEXITCODE -ne 0) { throw "curl failed for $Rel" }
        $doc = [xml]::new(); $doc.PreserveWhitespace = $true; $doc.Load($dest)
        $XmlCache[$Rel] = $doc
    }
    $XmlCache[$Rel]
}

# SHA-1 of the UTF-8 bytes of the untrimmed XML license text node, used to detect text changes.
function Get-LicenseHash([xml]$Doc, [string]$Id) {
    $node = $Doc.DocumentElement.SelectSingleNode("license[@id='$Id']")
    if (-not $node) { throw "license $Id not in XML" }
    $sha = [Security.Cryptography.SHA1]::Create()
    (($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($node.InnerText)) | ForEach-Object { $_.ToString('x2') }) -join '')
}

function Get-Revision($Pkg) {
    $r = $Pkg.SelectSingleNode('revision')
    if ($r.SelectSingleNode('preview')) { return $null }  # previews are never stable
    (@('major', 'minor', 'micro') | ForEach-Object { $r.SelectSingleNode($_) } | Where-Object { $_ } | ForEach-Object { $_.InnerText }) -join '.'
}

function Get-StableEntries([xml]$Doc, [string]$Path, [string]$Rel) {
    foreach ($p in $Doc.DocumentElement.SelectNodes("remotePackage[@path='$Path']")) {
        $rev = Get-Revision $p
        if (-not $rev) { continue }
        if ($p.SelectSingleNode('channelRef').GetAttribute('ref') -ne 'channel-0') { continue }
        if ($p.SelectSingleNode('uses-license').GetAttribute('ref') -ne $LicenseId) { continue }
        foreach ($a in $p.SelectNodes('archives/archive')) {
            $os = $a.SelectSingleNode('host-os'); $arch = $a.SelectSingleNode('host-arch')
            if ($os -and $os.InnerText -ne 'windows') { continue }
            if ($arch -and $arch.InnerText -ne 'x64') { continue }
            $c = $a.SelectSingleNode('complete')
            $sum = $c.SelectSingleNode('checksum')
            if ($sum.GetAttribute('type') -and $sum.GetAttribute('type') -ne 'sha1') { continue }
            [pscustomobject]@{
                revision = $rev; channel = 'channel-0'; license = $LicenseId
                url = $RepoBase + ($Rel -replace '[^/]+$', '') + $c.SelectSingleNode('url').InnerText
                size = [long]$c.SelectSingleNode('size').InnerText
                sha1 = $sum.InnerText.Trim().ToLowerInvariant()
            }
        }
    }
}

function Find-Entry($W, [string]$Revision) {
    $doc = Get-RepoXml $W.xml
    $all = @(Get-StableEntries $doc $W.path $W.xml)
    if ($Revision) { return $all | Where-Object { $_.revision -eq $Revision } | Select-Object -First 1 }
    $all | Sort-Object { [version]($_.revision + '.0') } -Descending | Select-Object -First 1
}

function Get-InstallDir([string]$Path) { Join-Path $SdkDir ($Path -replace ';', '\') }
function Get-MarkerFile([string]$Path) { Join-Path $SdkDir ('.mtp\' + ($Path -replace ';', '_') + '.json') }

# ---- -UpdatePins: pick the latest stable entries and rewrite the pin file ----
if ($UpdatePins) {
    $pins = Read-Pins
    if (-not $pins) { $pins = [ordered]@{ schema = 1 } }
    $list = foreach ($w in $Wanted) {
        $e = Find-Entry $w $null
        if (-not $e) { throw "no channel-0 $LicenseId entry for $($w.path)" }
        [ordered]@{ path = $w.path; revision = $e.revision; channel = $e.channel; license = $e.license
                    xml = $RepoBase + $w.xml; url = $e.url; size = $e.size; sha1 = $e.sha1 }
    }
    $pins['licenses'] = [ordered]@{ $LicenseId = [ordered]@{ sha1 = (Get-LicenseHash (Get-RepoXml $Wanted[0].xml) $LicenseId) } }
    $pins['packages'] = @($list)
    Write-Pins $pins
    return
}

# ---- normal run ----
Write-Host "license  these packages are under the Android SDK License ($LicenseId); running this script records your acceptance of it for this machine"
$pins = Read-Pins
if (-not $pins -or -not $pins.Contains('packages')) { throw "no package pins in $PinFile; run with -UpdatePins first" }
$licHash = $pins.licenses[$LicenseId].sha1
$downloads = 0

foreach ($pin in $pins.packages) {
    $w = $Wanted | Where-Object { $_.path -eq $pin.path }
    if (-not $w) { throw "pinned package $($pin.path) is not one this script knows" }
    $dir = Get-InstallDir $pin.path
    $marker = Get-MarkerFile $pin.path
    if ((Test-Path -LiteralPath $marker) -and (Test-Path -LiteralPath (Join-Path $dir $w.probe))) {
        $m = Get-Content -Raw -LiteralPath $marker | ConvertFrom-Json
        if ($m.sha1 -eq $pin.sha1 -and $m.revision -eq $pin.revision) {
            Write-Host "reuse    $($pin.path) $($pin.revision) (installed from verified zip)"
            continue
        }
    }

    # The pin must still match Google's XML, and be stable and under android-sdk-license.
    $e = Find-Entry $w $pin.revision
    if (-not $e) { throw "$($pin.path) $($pin.revision) is not a channel-0 $LicenseId package in $($w.xml); refusing" }
    foreach ($k in 'url', 'size', 'sha1', 'channel', 'license') {
        if ("$($e.$k)" -ne "$($pin[$k])") { throw "$($pin.path): pinned $k '$($pin[$k])' differs from XML '$($e.$k)'; refusing" }
    }
    $xmlLic = Get-LicenseHash (Get-RepoXml $w.xml) $LicenseId
    if ($xmlLic -ne $licHash) { throw "android-sdk-license text changed (sha1 $xmlLic, pinned $licHash); re-read it and re-pin" }

    $zip = Join-Path $DlDir (Split-Path -Leaf $pin.url)
    if (Get-VerifiedDownload $pin.url $zip $pin.size 'SHA1' $pin.sha1) { $downloads++ }

    $x = Join-Path $DlDir ('x-' + ($pin.path -replace ';', '-'))
    Write-Host "extract  $(Split-Path -Leaf $zip) -> $x"
    Expand-ToNewDir $zip $x
    $top = @(Get-ChildItem -LiteralPath $x -Force)
    if ($top.Count -ne 1 -or -not $top[0].PSIsContainer) { throw "$zip should hold exactly one top-level folder" }
    if (-not (Test-Path -LiteralPath (Join-Path $top[0].FullName $w.probe))) { throw "$zip has no $($w.probe)" }

    if (Test-Path -LiteralPath $dir) { Remove-Item -LiteralPath $dir -Recurse -Force }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $dir) | Out-Null
    Move-Item -LiteralPath $top[0].FullName -Destination $dir
    Remove-Item -LiteralPath $x -Recurse -Force
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $marker) | Out-Null
    [ordered]@{ path = $pin.path; revision = $pin.revision; sha1 = $pin.sha1; installed = (Get-Date).ToString('o') } |
        ConvertTo-Json | Set-Content -LiteralPath $marker -Encoding utf8NoBOM
    Write-Host "install  $($pin.path) $($pin.revision) -> $dir"
}

# License acceptance record: one text-node hash per line, appended with a leading newline.
$licFile = Join-Path $SdkDir "licenses\$LicenseId"
$have = if (Test-Path -LiteralPath $licFile) { (Get-Content -LiteralPath $licFile) -contains $licHash } else { $false }
if (-not $have) {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $licFile) | Out-Null
    [IO.File]::AppendAllText($licFile, "`n$licHash")
    Write-Host "license  accepted $LicenseId ($licHash) -> $licFile"
} else {
    Write-Host "reuse    licenses/$LicenseId"
}
Write-Host "done     downloads=$downloads"
