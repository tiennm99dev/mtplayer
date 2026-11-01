<#
.SYNOPSIS
  Checks that an instance's gRPC endpoint refuses calls without its token and accepts them with it.

.DESCRIPTION
  Sends a raw gRPC call (EmulatorController/getStatus, empty request) over
  cleartext HTTP/2 with .NET's HttpClient, so no grpcurl is needed. The token
  is read from the emulator's discovery file (pid_<qemu pid>.ini, matched by
  grpc.port) and is never printed. Exits non-zero unless the call without a
  token fails with UNAUTHENTICATED (16) and the call with it returns OK (0).
#>
[CmdletBinding()]
param([Parameter(Mandatory, Position = 0)][int]$GrpcPort)

. (Join-Path $PSScriptRoot 'spike-env.ps1')

$disc = $DiscoveryDirs | ForEach-Object { Get-ChildItem -LiteralPath $_ -Filter 'pid_*.ini' -ErrorAction SilentlyContinue } |
    Where-Object { (Get-Content -LiteralPath $_.FullName) -contains "grpc.port=$GrpcPort" } | Select-Object -First 1
if (-not $disc) { throw "no emulator discovery file with grpc.port=$GrpcPort" }
$line = (Get-Content -LiteralPath $disc.FullName) | Where-Object { $_ -like 'grpc.token=*' }
if (-not $line) { throw "$($disc.Name) has no grpc.token (was -grpc-use-token passed?)" }
$token = $line.Substring('grpc.token='.Length)

function Invoke-GrpcStatus([string]$Bearer) {
    $handler = [Net.Http.SocketsHttpHandler]::new()
    $client = [Net.Http.HttpClient]::new($handler)
    try {
        $req = [Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::Post,
            "http://127.0.0.1:$GrpcPort/android.emulation.control.EmulatorController/getStatus")
        $req.Version = [Version]'2.0'
        $req.VersionPolicy = [Net.Http.HttpVersionPolicy]::RequestVersionExact
        $req.Content = [Net.Http.ByteArrayContent]::new([byte[]](0, 0, 0, 0, 0))  # uncompressed, 0-byte message
        $req.Content.Headers.ContentType = 'application/grpc'
        [void]$req.Headers.TryAddWithoutValidation('te', 'trailers')
        if ($Bearer) { [void]$req.Headers.TryAddWithoutValidation('authorization', "Bearer $Bearer") }
        $resp = $client.SendAsync($req).GetAwaiter().GetResult()
        $body = $resp.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult()
        $status = $null; $msg = $null
        foreach ($h in @($resp.TrailingHeaders, $resp.Headers)) {
            $v = $null
            if (-not $status -and $h.TryGetValues('grpc-status', [ref]$v)) { $status = [int]($v | Select-Object -First 1) }
            if (-not $msg -and $h.TryGetValues('grpc-message', [ref]$v)) { $msg = [Uri]::UnescapeDataString(($v | Select-Object -First 1)) }
        }
        [pscustomobject]@{ http = [int]$resp.StatusCode; grpcStatus = $status; message = $msg; bodyBytes = $body.Length }
    } finally { $client.Dispose() }
}

$without = Invoke-GrpcStatus $null
$wrong   = Invoke-GrpcStatus 'not-the-token'
$with    = Invoke-GrpcStatus $token
Write-Host "discovery $($disc.Name): grpc.token=[redacted]"
Write-Host "no token:    http $($without.http) grpc-status $($without.grpcStatus) '$($without.message)'"
Write-Host "wrong token: http $($wrong.http) grpc-status $($wrong.grpcStatus) '$($wrong.message)'"
Write-Host "with token:  http $($with.http) grpc-status $($with.grpcStatus) body $($with.bodyBytes) bytes"
if ($without.grpcStatus -ne 16 -or $wrong.grpcStatus -ne 16 -or $with.grpcStatus -ne 0) { Write-Host 'RESULT fail'; exit 1 }
Write-Host 'RESULT pass'
