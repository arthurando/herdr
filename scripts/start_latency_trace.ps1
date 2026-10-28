param(
    [string]$Client = (Join-Path $PSScriptRoot 'herdr.exe'),
    [string]$TraceDirectory = (Join-Path $PSScriptRoot 'traces'),
    [string]$Session = ''
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $Client -PathType Leaf)) { throw "Client not found: $Client" }
New-Item -ItemType Directory -Force -Path $TraceDirectory | Out-Null
$tracePath = Join-Path $TraceDirectory ('latency-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N') + '.jsonl')
$previousTrace = $env:HERDR_LATENCY_TRACE
try {
    $env:HERDR_LATENCY_TRACE = $tracePath
    Write-Host "Numeric latency trace: $tracePath"
    Write-Host 'Use the normal Sid VM session. The trace stops after 180 seconds.'
    if ($Session) { & $Client --session $Session client } else { & $Client client }
} finally {
    $env:HERDR_LATENCY_TRACE = $previousTrace
}
