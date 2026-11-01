<#
.SYNOPSIS
  Compiles, packages and signs the spike identity test app (spike/test-app) into
  <repo>/.tmp/tools/test-app/mtp-spike-test.apk.

.DESCRIPTION
  Uses only the pinned Google packages in <repo>/.tmp/sdk (build-tools 36.1.0 and
  platforms/android-36, from spike/fetch-packages.ps1) and the host JDK
  ($env:JAVA_HOME, else javac on PATH). Steps: javac -> d8 -> aapt2 link ->
  add classes.dex -> zipalign -> apksigner sign/verify. Every output stays
  under .tmp; only the source and this script are repo code. The signing key
  is a throwaway key generated once into .tmp/tools/test-app/spike.keystore.
  Writes the JDK version and tool output to .tmp/evidence/identity/test-app-make.txt.
#>
[CmdletBinding()]
param()

. (Join-Path $PSScriptRoot 'fetch-common.ps1')
$ErrorActionPreference = 'Stop'

function Resolve-JdkTool([string]$Tool) {
    if ($env:JAVA_HOME) {
        $p = Join-Path $env:JAVA_HOME "bin\$Tool.exe"
        if (Test-Path -LiteralPath $p) { return $p }
    }
    $c = Get-Command $Tool -ErrorAction SilentlyContinue
    if ($c) { return $c.Source }
    throw "no $Tool found: set JAVA_HOME to a JDK 17+ (the JDK is not part of the Android SDK and is not fetched by the spike)"
}
$javac = Resolve-JdkTool 'javac'; $java = Resolve-JdkTool 'java'; $keytool = Resolve-JdkTool 'keytool'

$bt = Join-Path $SdkDir 'build-tools\36.1.0'
$androidJar = Join-Path $SdkDir 'platforms\android-36\android.jar'
foreach ($f in (Join-Path $bt 'aapt2.exe'), (Join-Path $bt 'zipalign.exe'), (Join-Path $bt 'lib\d8.jar'), (Join-Path $bt 'lib\apksigner.jar'), $androidJar) {
    if (-not (Test-Path -LiteralPath $f)) { throw "missing $f; run spike/fetch-packages.ps1" }
}

$src = Join-Path $PSScriptRoot 'test-app'
$out = Join-Path $Tmp 'tools\test-app'
$classes = Join-Path $out 'classes'; $dex = Join-Path $out 'dex'
foreach ($d in $classes, $dex) { if (Test-Path -LiteralPath $d) { Remove-Item -LiteralPath $d -Recurse -Force }; New-Item -ItemType Directory -Force -Path $d | Out-Null }
$unaligned = Join-Path $out 'unaligned.apk'; $aligned = Join-Path $out 'aligned.apk'
$apk = Join-Path $out 'mtp-spike-test.apk'
$ks = Join-Path $out 'spike.keystore'
$log = Join-Path $Tmp 'evidence\identity\test-app-make.txt'
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $log) | Out-Null

function Invoke-Step([string]$Label, [string]$Exe, [string[]]$ArgList) {
    $o = & $Exe @ArgList 2>&1 | Out-String
    Add-Content -LiteralPath $log -Value "## $Label`n$o"
    if ($LASTEXITCODE -ne 0) { throw "$Label failed (exit $LASTEXITCODE):`n$o" }
    Write-Host "ok       $Label"
}

Set-Content -LiteralPath $log -Value "test app make $(Get-Date -Format o)`n## JDK`n$((& $java -version 2>&1) -join "`n")"
$sources = @(Get-ChildItem -LiteralPath (Join-Path $src 'src') -Recurse -Filter '*.java' | ForEach-Object FullName)
Invoke-Step 'javac' $javac (@('--release', '17', '-nowarn', '-cp', $androidJar, '-d', $classes) + $sources)
$classFiles = @(Get-ChildItem -LiteralPath $classes -Recurse -Filter '*.class' | ForEach-Object FullName)
# Run the jars directly with the host JDK instead of the .bat wrappers.
Invoke-Step 'd8' $java (@('-cp', (Join-Path $bt 'lib\d8.jar'), 'com.android.tools.r8.D8', '--release', '--min-api', '36',
                          '--lib', $androidJar, '--output', $dex) + $classFiles)
Invoke-Step 'aapt2 link' (Join-Path $bt 'aapt2.exe') @('link', '--manifest', (Join-Path $src 'AndroidManifest.xml'), '-I', $androidJar, '-o', $unaligned)

Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::Open($unaligned, 'Update')
try { [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, (Join-Path $dex 'classes.dex'), 'classes.dex') } finally { $zip.Dispose() }
Write-Host "ok       add classes.dex"

Invoke-Step 'zipalign' (Join-Path $bt 'zipalign.exe') @('-p', '-f', '4', $unaligned, $aligned)

# Throwaway spike signing key. Its password is random per keystore, kept next to it
# under .tmp (gitignored) and never printed.
$ksPassFile = Join-Path $out 'spike.keystore.pass'
if (-not (Test-Path -LiteralPath $ks) -or -not (Test-Path -LiteralPath $ksPassFile)) {
    if (Test-Path -LiteralPath $ks) { Remove-Item -LiteralPath $ks -Force }
    $bytes = [byte[]]::new(24); [Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
    [IO.File]::WriteAllText($ksPassFile, [Convert]::ToHexString($bytes).ToLowerInvariant())
}
$ksPass = [IO.File]::ReadAllText($ksPassFile).Trim()
if (-not (Test-Path -LiteralPath $ks)) {
    Invoke-Step 'keytool' $keytool @('-genkeypair', '-keystore', $ks, '-storepass', $ksPass, '-keypass', $ksPass, '-alias', 'spike',
        '-keyalg', 'RSA', '-keysize', '2048', '-validity', '3650', '-dname', 'CN=MTPlayer spike test')
}
$signer = Join-Path $bt 'lib\apksigner.jar'
Invoke-Step 'apksigner sign' $java @('-jar', $signer, 'sign', '--ks', $ks, '--ks-pass', "pass:$ksPass", '--ks-key-alias', 'spike', '--out', $apk, $aligned)
Invoke-Step 'apksigner verify' $java @('-jar', $signer, 'verify', '--verbose', $apk)
Remove-Item -LiteralPath $unaligned, $aligned -Force
Write-Host "apk      $apk"
$apk
