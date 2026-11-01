# Shared helpers for the spike fetch scripts. Dot-source this file.
# All paths derive from the repo root; nothing outside <repo>/.tmp is written.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$script:RepoRoot = Split-Path -Parent $PSScriptRoot
$script:Tmp      = Join-Path $RepoRoot '.tmp'
$script:DlDir    = Join-Path $Tmp 'dl'
$script:SdkDir   = Join-Path $Tmp 'sdk'
$script:PinFile  = Join-Path $PSScriptRoot 'packages.pin.json'
# Windows' own curl.exe and bsdtar, never whatever is first on PATH.
$script:CurlExe  = Join-Path $env:SystemRoot 'System32\curl.exe'
$script:TarExe   = Join-Path $env:SystemRoot 'System32\tar.exe'

function Get-FileHashHex([string]$Path, [string]$Algorithm) {
    (Get-FileHash -LiteralPath $Path -Algorithm $Algorithm).Hash.ToLowerInvariant()
}

function Test-Download([string]$Path, [long]$Size, [string]$Algorithm, [string]$Hash) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $false }
    if ((Get-Item -LiteralPath $Path).Length -ne $Size) { return $false }
    return (Get-FileHashHex $Path $Algorithm) -eq $Hash.ToLowerInvariant()
}

# Downloads $Url to $Path unless a verified copy is already there.
# Returns $true when it downloaded, $false when it reused the existing copy.
function Get-VerifiedDownload([string]$Url, [string]$Path, [long]$Size, [string]$Algorithm, [string]$Hash) {
    if (Test-Download $Path $Size $Algorithm $Hash) {
        Write-Host "reuse    $(Split-Path -Leaf $Path) ($Algorithm ok)"
        return $false
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Path) | Out-Null
    $part = "$Path.part"
    Remove-Item -LiteralPath $part, $Path -Force -ErrorAction SilentlyContinue
    Write-Host "download $Url"
    & $CurlExe --fail --location --silent --show-error --retry 3 --output $part $Url
    if ($LASTEXITCODE -ne 0) { throw "curl failed ($LASTEXITCODE) for $Url" }
    if (-not (Test-Download $part $Size $Algorithm $Hash)) {
        Remove-Item -LiteralPath $part -Force
        throw "size or $Algorithm mismatch for $Url; refusing it"
    }
    Move-Item -LiteralPath $part -Destination $Path
    Write-Host "verified $(Split-Path -Leaf $Path) ($Algorithm ok)"
    return $true
}

# Extracts an archive into a brand-new empty directory. Optional $Members limits
# extraction to those archive paths. bsdtar refuses absolute and '..' paths.
function Expand-ToNewDir([string]$Archive, [string]$Dest, [string[]]$Members = @()) {
    if (Test-Path -LiteralPath $Dest) { Remove-Item -LiteralPath $Dest -Recurse -Force }
    New-Item -ItemType Directory -Path $Dest | Out-Null
    & $TarExe -x -f $Archive -C $Dest @Members
    if ($LASTEXITCODE -ne 0) { throw "tar failed ($LASTEXITCODE) extracting $Archive" }
}

function Read-Pins {
    if (-not (Test-Path -LiteralPath $PinFile)) { return $null }
    Get-Content -Raw -LiteralPath $PinFile | ConvertFrom-Json -AsHashtable
}

function Write-Pins($Pins) {
    $json = $Pins | ConvertTo-Json -Depth 10
    [IO.File]::WriteAllText($PinFile, ($json -replace "`r`n", "`n") + "`n", [Text.UTF8Encoding]::new($false))
    Write-Host "wrote    $PinFile"
}
