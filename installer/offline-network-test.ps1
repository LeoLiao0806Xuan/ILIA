param(
    [string]$Version = "1.1.0",
    [string]$Output = ""
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$stageRoot = Join-Path $projectRoot "dist\installer-stage\ILIA"
$auditRoot = Join-Path $projectRoot "dist\offline-network-audit"
$stopFile = Join-Path $auditRoot "stop"
$stdout = Join-Path $auditRoot "answer.json"
$stderr = Join-Path $auditRoot "answer.stderr.log"
if (-not $Output) { $Output = Join-Path $projectRoot "release\evidence\$Version\runtime-offline-network.json" }
if (Test-Path -LiteralPath $auditRoot) { Remove-Item -LiteralPath $auditRoot -Recurse -Force }
New-Item -ItemType Directory -Path $auditRoot -Force | Out-Null

$watcher = Start-Job -ScriptBlock {
    param($StopFile)
    while (-not (Test-Path -LiteralPath $StopFile)) {
        $processes = @(Get-Process -Name @("ilia-ask", "ilia-search", "llama-server") -ErrorAction SilentlyContinue)
        foreach ($process in $processes) {
            Get-NetTCPConnection -OwningProcess $process.Id -State Established -ErrorAction SilentlyContinue |
                ForEach-Object {
                    [pscustomobject]@{
                        observed_at_utc = [DateTime]::UtcNow.ToString("o")
                        process = $process.ProcessName
                        process_id = $process.Id
                        local_address = $_.LocalAddress
                        local_port = $_.LocalPort
                        remote_address = $_.RemoteAddress
                        remote_port = $_.RemotePort
                    }
                }
        }
        Start-Sleep -Milliseconds 50
    }
} -ArgumentList $stopFile

$oldOrt = $env:ORT_DYLIB_PATH
try {
    $env:ORT_DYLIB_PATH = Join-Path $stageRoot "runtime\onnx\onnxruntime.dll"
    $ask = Join-Path $projectRoot "target\release\ilia-ask.exe"
    $arguments = @(
        "--db", (Join-Path $stageRoot "data\ilia.sqlite3"),
        "--bge-cache", (Join-Path $stageRoot "models\bge-m3"),
        "--runtime-root", (Join-Path $stageRoot "runtime"),
        "--backend", "cpu",
        "--qwen-model", (Join-Path $stageRoot "models\qwen3-4b\Qwen3-4B-Q4_K_M.gguf"),
        "--question", '"Under UNCLOS Article 3, what is the maximum breadth of the territorial sea?"',
        "--log-dir", $auditRoot
    )
    $process = Start-Process -FilePath $ask -ArgumentList $arguments -WindowStyle Hidden -Wait -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    if ($process.ExitCode -ne 0) { throw "Offline answer process failed with exit code $($process.ExitCode)" }
} finally {
    $env:ORT_DYLIB_PATH = $oldOrt
    [IO.File]::WriteAllText($stopFile, "stop")
    Wait-Job -Job $watcher -Timeout 10 | Out-Null
}

$observations = @(Receive-Job -Job $watcher)
Remove-Job -Job $watcher -Force
$deduplicated = @($observations | Sort-Object process,local_address,local_port,remote_address,remote_port -Unique)
$external = @($deduplicated | Where-Object {
    $_.remote_address -notin @("127.0.0.1", "::1")
})
$answer = Get-Content -Raw -LiteralPath $stdout | ConvertFrom-Json
$report = [ordered]@{
    report_version = 1
    release_version = $Version
    generated_at_utc = [DateTime]::UtcNow.ToString("o")
    scope = "50ms TCP connection observation during installed-payload retrieval and CPU local answer"
    packet_driver = "PktMon unavailable without administrator rights; Get-NetTCPConnection process observation used"
    answer_grounded = [bool]$answer.answer.grounded
    observed_connections = $deduplicated
    external_connections = $external
    status = if ($process.ExitCode -eq 0 -and $answer.answer.grounded -and $external.Count -eq 0) { "passed" } else { "failed" }
}
$parent = Split-Path $Output -Parent
New-Item -ItemType Directory -Path $parent -Force | Out-Null
$report | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath $Output -Encoding utf8
$report | ConvertTo-Json -Depth 7
if ($report.status -ne "passed") { exit 1 }
