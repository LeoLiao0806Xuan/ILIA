param(
    [string]$Version = "1.0.0-rc.1",
    [ValidateSet("auto", "cuda", "vulkan", "cpu")][string]$Backend = "cpu",
    [switch]$SkipCliBuild
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$distRoot = Join-Path $projectRoot "dist"
$installerRoot = Join-Path $distRoot "installer"
$installRoot = Join-Path $distRoot "installer-smoke-current"
$logRoot = Join-Path $distRoot "installer-smoke-logs"
$evidenceRoot = Join-Path $projectRoot "release\evidence\$Version"
$reportPath = Join-Path $installerRoot "installer-smoke-test.json"
$evidencePath = Join-Path $evidenceRoot "windows-installer-smoke.json"
$baseName = "ILIA-$Version-windows-x64-offline-setup"
$setupPath = Join-Path $installerRoot "$baseName.exe"
$releaseRoot = Join-Path $projectRoot "target\x86_64-pc-windows-gnu\release"
$searchExe = Join-Path $releaseRoot "ilia-search.exe"
$askExe = Join-Path $releaseRoot "ilia-ask.exe"
$question = "Under UNCLOS Article 3, what is the maximum breadth of the territorial sea?"
$startedAt = (Get-Date).ToUniversalTime()
$errors = [System.Collections.Generic.List[string]]::new()
$appProcess = $null
$installExit = $null
$uninstallExit = $null
$installedAppStarted = $false
$temporaryInstallRemoved = $false
$searchResult = $null
$answerResult = $null
$originalOrtPath = $env:ORT_DYLIB_PATH

function Assert-File([string]$Path, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label is missing: $Path"
    }
}

function Assert-SafeSmokePath([string]$Path) {
    $resolved = [IO.Path]::GetFullPath($Path)
    $expected = [IO.Path]::GetFullPath("$distRoot\")
    if (-not $resolved.StartsWith($expected, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to modify smoke-test path outside dist: $resolved"
    }
}

Assert-SafeSmokePath $installRoot
Assert-SafeSmokePath $logRoot
Assert-File $setupPath "Installer launcher"

$installerFiles = @(Get-ChildItem -LiteralPath $installerRoot -Filter "$baseName*" -File | Sort-Object Name)
if ($installerFiles.Count -lt 2) { throw "Installer launcher or data slices are missing" }
$installerEvidence = @($installerFiles | ForEach-Object {
    [ordered]@{
        name = $_.Name
        size = $_.Length
        generated_at = $_.LastWriteTimeUtc.ToString("o")
        sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        authenticode_status = (Get-AuthenticodeSignature -LiteralPath $_.FullName).Status.ToString()
    }
})

try {
    if (-not $SkipCliBuild) {
        . (Join-Path $projectRoot "tools\windows-toolchain.ps1")
        Set-IliaGnuEnvironment
        & cargo +stable-x86_64-pc-windows-gnu build --offline --release `
            -p ilia-retrieval --bin ilia-search -p ilia-inference --bin ilia-ask
        if ($LASTEXITCODE -ne 0) { throw "Smoke-test CLI build failed with exit code $LASTEXITCODE" }
    }
    Assert-File $searchExe "Retrieval smoke-test executable"
    Assert-File $askExe "Question-answering smoke-test executable"

    if (Test-Path -LiteralPath $installRoot) { Remove-Item -LiteralPath $installRoot -Recurse -Force }
    if (Test-Path -LiteralPath $logRoot) { Remove-Item -LiteralPath $logRoot -Recurse -Force }
    New-Item -ItemType Directory -Path $logRoot -Force | Out-Null

    $installStartedAt = (Get-Date).ToUniversalTime()
    $installer = Start-Process -FilePath $setupPath -ArgumentList @(
        "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-", "/DIR=$installRoot"
    ) -WindowStyle Hidden -Wait -PassThru
    $installExit = $installer.ExitCode
    if ($installExit -ne 0) { throw "Silent installation failed with exit code $installExit" }

    $desktopPath = Join-Path $installRoot "ilia-desktop.exe"
    Assert-File $desktopPath "Installed desktop application"
    $appProcess = Start-Process -FilePath $desktopPath -WorkingDirectory $installRoot -WindowStyle Hidden -PassThru
    Start-Sleep -Seconds 5
    $installedAppStarted = -not $appProcess.HasExited
    if (-not $installedAppStarted) { throw "Installed desktop application exited during startup" }
    $null = $appProcess.CloseMainWindow()
    if (-not $appProcess.WaitForExit(10000)) {
        Stop-Process -Id $appProcess.Id -Force
        $appProcess.WaitForExit()
    }
    $appProcess = $null
    Start-Sleep -Seconds 5

    $database = Join-Path $installRoot "data\ilia_prototype.sqlite3"
    $bgeCache = Join-Path $installRoot "models\bge-m3"
    $runtimeRoot = Join-Path $installRoot "runtime"
    $qwenModel = Join-Path $installRoot "models\qwen3-4b\Qwen3-4B-Q4_K_M.gguf"
    $env:ORT_DYLIB_PATH = Join-Path $runtimeRoot "onnx\onnxruntime.dll"

    $searchTimer = [Diagnostics.Stopwatch]::StartNew()
    $searchJson = (& $searchExe --db $database --model-cache $bgeCache --query $question | Out-String)
    $searchTimer.Stop()
    if ($LASTEXITCODE -ne 0) { throw "Installed-payload retrieval failed with exit code $LASTEXITCODE" }
    $search = $searchJson | ConvertFrom-Json
    if (@($search.hits).Count -eq 0 -or @($search.evidence).Count -eq 0) {
        throw "Installed-payload retrieval returned no hits or evidence"
    }
    $searchResult = [ordered]@{
        passed = $true
        query = $question
        duration_ms = $searchTimer.ElapsedMilliseconds
        hit_count = @($search.hits).Count
        evidence_count = @($search.evidence).Count
        top_document = $search.hits[0].canonical_title
        top_citation = $search.hits[0].citation_label
    }

    $answerTimer = [Diagnostics.Stopwatch]::StartNew()
    $answerJson = (& $askExe --db $database --bge-cache $bgeCache --runtime-root $runtimeRoot `
        --backend $Backend --qwen-model $qwenModel --question $question --log-dir $logRoot | Out-String)
    $answerTimer.Stop()
    if ($LASTEXITCODE -ne 0) { throw "Installed-payload local question answering failed with exit code $LASTEXITCODE" }
    $answer = $answerJson | ConvertFrom-Json
    if (-not $answer.answer.grounded -or [string]::IsNullOrWhiteSpace($answer.answer.answer)) {
        throw "Installed-payload answer was empty or failed citation grounding"
    }
    $answerResult = [ordered]@{
        passed = $true
        question = $question
        duration_ms = $answerTimer.ElapsedMilliseconds
        backend = $answer.runtime.selected_backend
        grounded = [bool]$answer.answer.grounded
        evidence_count = @($answer.answer.evidence).Count
        citations = @($answer.answer.evidence | ForEach-Object citation_label | Select-Object -Unique)
        answer = $answer.answer.answer
    }
} catch {
    $errors.Add($_.Exception.Message)
} finally {
    $env:ORT_DYLIB_PATH = $originalOrtPath
    if ($appProcess -and -not $appProcess.HasExited) {
        Stop-Process -Id $appProcess.Id -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 3
    }
    $uninstallerPath = Join-Path $installRoot "unins000.exe"
    if (Test-Path -LiteralPath $uninstallerPath -PathType Leaf) {
        try {
            $uninstaller = Start-Process -FilePath $uninstallerPath -ArgumentList @(
                "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"
            ) -WindowStyle Hidden -Wait -PassThru
            $uninstallExit = $uninstaller.ExitCode
        } catch {
            $errors.Add("Uninstall failed: $($_.Exception.Message)")
        }
    }
    foreach ($attempt in 1..15) {
        if (-not (Test-Path -LiteralPath $installRoot)) {
            $temporaryInstallRemoved = $true
            break
        }
        Start-Sleep -Seconds 1
    }
    if (-not $temporaryInstallRemoved) {
        $errors.Add("Temporary installation directory was not removed")
    }
}

$status = if ($errors.Count -eq 0 -and $installExit -eq 0 -and $installedAppStarted -and
    $searchResult -and $answerResult -and $uninstallExit -eq 0 -and $temporaryInstallRemoved) {
    "passed"
} else {
    "failed"
}
$report = [ordered]@{
    schema_version = 2
    status = $status
    version = $Version
    tested_at = (Get-Date).ToUniversalTime().ToString("o")
    test_started_at = $startedAt.ToString("o")
    environment = [ordered]@{
        os = [Environment]::OSVersion.VersionString
        architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
        backend_requested = $Backend
        scope = "build-machine release-candidate smoke test; not a clean-VM certification"
    }
    installer_files = $installerEvidence
    installation = [ordered]@{
        started_at = if ($installStartedAt) { $installStartedAt.ToString("o") } else { $null }
        exit_code = $installExit
        desktop_started = $installedAppStarted
    }
    retrieval = $searchResult
    local_answer = $answerResult
    uninstall = [ordered]@{
        exit_code = $uninstallExit
        temporary_install_removed = $temporaryInstallRemoved
    }
    errors = @($errors)
}

New-Item -ItemType Directory -Path $installerRoot -Force | Out-Null
New-Item -ItemType Directory -Path $evidenceRoot -Force | Out-Null
$reportJson = $report | ConvertTo-Json -Depth 8
$reportJson | Set-Content -LiteralPath $reportPath -Encoding utf8
$reportJson | Set-Content -LiteralPath $evidencePath -Encoding utf8
Copy-Item -LiteralPath (Join-Path $installerRoot "installer-verification.json") `
    -Destination (Join-Path $evidenceRoot "installer-verification.json") -Force
Copy-Item -LiteralPath (Join-Path $installerRoot "SHA256SUMS.txt") `
    -Destination (Join-Path $evidenceRoot "SHA256SUMS.txt") -Force
$reportJson
if ($status -ne "passed") { exit 1 }
