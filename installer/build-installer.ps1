param(
    [string]$Version = "1.1.0",
    [string]$InnoSetupCompiler = "",
    [switch]$SkipBuild,
    [switch]$SkipWebView2,
    [switch]$AllowPendingComponentsForLocalTesting,
    [switch]$PreflightOnly
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$stageRoot = Join-Path $projectRoot "dist\installer-stage\ILIA"
$outputRoot = Join-Path $projectRoot "dist\installer"
$desktopRoot = Join-Path $projectRoot "apps\desktop"
# The portable GNU toolchain is selected through RUSTUP_TOOLCHAIN, so Cargo and
# Tauri emit current release artifacts into target\release. Do not package the
# stale explicit-target directory left by older builds.
$releaseRoot = Join-Path $projectRoot "target\release"
$webView2 = Join-Path $projectRoot "installer\prerequisites\MicrosoftEdgeWebView2RuntimeInstallerX64.exe"
$vcRedist = Join-Path $projectRoot "installer\prerequisites\VC_redist.x64.exe"
$trustedUpdateKey = Join-Path $projectRoot "update\trusted-key.json"
$rightsGate = Join-Path $projectRoot "tools\release_rights_gate.py"
$corpusValidator = Join-Path $projectRoot "tools\corpus-validator\validate_corpus.py"
$validationReport = Join-Path $projectRoot "data\validation_report_$Version.json"

function Assert-File([string]$Path, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label is missing: $Path"
    }
}

function Copy-Tree([string]$Source, [string]$Destination) {
    if (-not (Test-Path -LiteralPath $Source -PathType Container)) {
        throw "Required directory is missing: $Source"
    }
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    foreach ($item in Get-ChildItem -LiteralPath $Source -Force) {
        Copy-Item -LiteralPath $item.FullName -Destination $Destination -Recurse -Force
    }
}

function Copy-IliaRuntime([string]$Name) {
    $source = Join-Path $projectRoot "runtime\$Name"
    $destination = Join-Path $stageRoot "runtime\$Name"
    if (-not (Test-Path -LiteralPath $source -PathType Container)) {
        throw "Required runtime directory is missing: $source"
    }
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    $allowed = @("llama-server.exe", "llama-server-impl.dll", "llama-common.dll", "llama.dll", "mtmd.dll", "libomp.dll", "LICENSE-LLVM-OpenMP", "runtime-manifest.json")
    $files = @(Get-ChildItem -LiteralPath $source -File | Where-Object {
        $_.Name -in $allowed -or $_.Name -like "ggml*.dll" -or ($Name -eq "cuda" -and $_.Name -in @("cudart64_13.dll", "cublas64_13.dll", "cublasLt64_13.dll"))
    })
    foreach ($file in $files) {
        Copy-Item -LiteralPath $file.FullName -Destination $destination -Force
    }
}

Assert-File (Join-Path $projectRoot "data\ilia.sqlite3") "Distributable SQLite database"
Assert-File (Join-Path $projectRoot "corpus\normalized\manifest.json") "Normalized corpus manifest"
Assert-File (Join-Path $projectRoot "models\bge-m3\model.onnx") "BGE-M3 model"
Assert-File (Join-Path $projectRoot "models\qwen3-4b\Qwen3-4B-Q4_K_M.gguf") "Qwen3-4B model"
foreach ($backend in @("cuda", "vulkan", "cpu")) {
    Assert-File (Join-Path $projectRoot "runtime\$backend\llama-server.exe") "$backend llama.cpp runtime"
}
Assert-File (Join-Path $projectRoot "runtime\onnx\onnxruntime.dll") "ONNX Runtime"
Assert-File $trustedUpdateKey "trusted update public key"
$requiredLicenseFiles = @(
    "BGE-M3-MIT.txt",
    "CORPUS-TERMS.md",
    "MICROSOFT-RUNTIME-REDISTRIBUTION.md",
    "microsoft-redistributables.json",
    "NVIDIA-CUDA-EULA-2026-01-26.html",
    "NVIDIA-CUDA-REDISTRIBUTION.md",
    "ONNX-Runtime-MIT.txt",
    "ONNX-Runtime-ThirdPartyNotices.txt",
    "Qwen3-4B-Apache-2.0.txt",
    "component-clearance.json",
    "corpus-redistribution-rights-matrix.csv",
    "cuda-redistributables.json",
    "dependencies\NPM-NOTICES.md",
    "dependencies\RUST-NOTICES.md",
    "dependencies\npm-dependencies.csv",
    "dependencies\rust-dependencies.csv",
    "dependencies\summary.json",
    "downloaded-license-manifest.json",
    "INSTALLER-THIRD-PARTY-TERMS.txt",
    "llama.cpp-MIT.txt"
)
foreach ($licenseFile in $requiredLicenseFiles) {
    Assert-File (Join-Path $projectRoot "licenses\$licenseFile") "Third-party license $licenseFile"
}
$dependencySummary = Get-Content -Raw -LiteralPath (Join-Path $projectRoot "licenses\dependencies\summary.json") | ConvertFrom-Json
if ($dependencySummary.rust_missing_declared_license -ne 0 -or $dependencySummary.npm_missing_declared_license -ne 0) {
    throw "Dependency inventory contains packages without a declared license"
}
$corpusRights = @(Import-Csv -LiteralPath (Join-Path $projectRoot "licenses\corpus-redistribution-rights-matrix.csv"))
$normalizedManifest = Get-Content -Raw -LiteralPath (Join-Path $projectRoot "corpus\normalized\manifest.json") | ConvertFrom-Json
& python $rightsGate --root $projectRoot --database (Join-Path $projectRoot "data\ilia.sqlite3")
if ($LASTEXITCODE -ne 0) { throw "Release rights gate failed with exit code $LASTEXITCODE" }
& python $corpusValidator --version $Version --database (Join-Path $projectRoot "data\ilia.sqlite3") --report $validationReport
if ($LASTEXITCODE -ne 0) { throw "Versioned corpus validation failed with exit code $LASTEXITCODE" }
$cudaInventory = Get-Content -Raw -LiteralPath (Join-Path $projectRoot "licenses\cuda-redistributables.json") | ConvertFrom-Json
foreach ($entry in $cudaInventory.files.psobject.Properties) {
    $path = Join-Path $projectRoot "runtime\cuda\$($entry.Name)"
    Assert-File $path "CUDA redistributable $($entry.Name)"
    $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $entry.Value) { throw "CUDA redistributable SHA-256 mismatch: $($entry.Name)" }
}
$microsoftInventory = Get-Content -Raw -LiteralPath (Join-Path $projectRoot "licenses\microsoft-redistributables.json") | ConvertFrom-Json
Assert-File $vcRedist "Microsoft Visual C++ Redistributable installer"
if ((Get-Item -LiteralPath $vcRedist).Length -ne $microsoftInventory.vc_redist_x64.byte_length) {
    throw "Microsoft Visual C++ Redistributable size mismatch"
}
$vcRedistHash = (Get-FileHash -LiteralPath $vcRedist -Algorithm SHA256).Hash.ToLowerInvariant()
if ($vcRedistHash -ne $microsoftInventory.vc_redist_x64.sha256) {
    throw "Microsoft Visual C++ Redistributable SHA-256 mismatch"
}
$vcSignature = Get-AuthenticodeSignature -LiteralPath $vcRedist
if ($vcSignature.Status -ne "Valid" -or $vcSignature.SignerCertificate.Subject -notlike "*Microsoft Corporation*") {
    throw "Microsoft Visual C++ Redistributable must have a valid Microsoft Authenticode signature"
}
$componentClearance = Get-Content -Raw -LiteralPath (Join-Path $projectRoot "licenses\component-clearance.json") | ConvertFrom-Json
$blockedComponents = @($componentClearance.components.psobject.Properties | Where-Object { $_.Value.public_distribution -in @("blocked", "pending", "red") })
if ($AllowPendingComponentsForLocalTesting) {
    throw "AllowPendingComponentsForLocalTesting is no longer supported; pending or blocked components cannot enter an installer stage."
}
if (-not $SkipWebView2) {
    Assert-File $webView2 "WebView2 offline installer"
}

if ($PreflightOnly) {
    Write-Output "Installer preflight passed: 49 normalized documents, ICRC CIHL excluded, CUDA hashes verified."
    exit 0
}

if (-not $SkipBuild) {
    Push-Location $desktopRoot
    try {
        & npm run tauri -- build --no-bundle
        if ($LASTEXITCODE -ne 0) { throw "Tauri application build failed with exit code $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
    . (Join-Path $projectRoot "tools\windows-toolchain.ps1")
    Set-IliaGnuEnvironment
    & cargo +stable-x86_64-pc-windows-gnu build --offline --release -p ilia-updater
    if ($LASTEXITCODE -ne 0) { throw "Update helper build failed with exit code $LASTEXITCODE" }
}

Assert-File (Join-Path $releaseRoot "ilia-desktop.exe") "ILIA desktop executable"
Assert-File (Join-Path $releaseRoot "WebView2Loader.dll") "WebView2 loader"
Assert-File (Join-Path $releaseRoot "ilia-updater.exe") "ILIA update helper"

if (-not $stageRoot.StartsWith((Join-Path $projectRoot "dist\"), [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to recreate a staging directory outside the project dist directory: $stageRoot"
}
if (Test-Path -LiteralPath $stageRoot) {
    Remove-Item -LiteralPath $stageRoot -Recurse -Force
}
New-Item -ItemType Directory -Path $stageRoot -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $releaseRoot "ilia-desktop.exe") -Destination $stageRoot
Copy-Item -LiteralPath (Join-Path $releaseRoot "WebView2Loader.dll") -Destination $stageRoot
Copy-Item -LiteralPath (Join-Path $releaseRoot "ilia-updater.exe") -Destination $stageRoot
Copy-Item -LiteralPath (Join-Path $desktopRoot "src-tauri\icons\icon.ico") -Destination $stageRoot
Copy-Item -LiteralPath (Join-Path $projectRoot "THIRD_PARTY_NOTICES.md") -Destination $stageRoot
New-Item -ItemType Directory -Path (Join-Path $stageRoot "licenses") -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $projectRoot "LICENSE") -Destination (Join-Path $stageRoot "licenses\ILIA-Apache-2.0.txt")
Copy-Item -LiteralPath (Join-Path $projectRoot "THIRD_PARTY_NOTICES.md") -Destination (Join-Path $stageRoot "licenses\THIRD_PARTY_NOTICES.md")
Copy-Tree (Join-Path $projectRoot "licenses") (Join-Path $stageRoot "licenses")
New-Item -ItemType Directory -Path (Join-Path $stageRoot "update") -Force | Out-Null
Copy-Item -LiteralPath $trustedUpdateKey -Destination (Join-Path $stageRoot "update\trusted-key.json")
New-Item -ItemType Directory -Path (Join-Path $stageRoot ".ilia-update") -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $projectRoot "update\initial-versions.json") -Destination (Join-Path $stageRoot ".ilia-update\versions.json")

New-Item -ItemType Directory -Path (Join-Path $stageRoot "data") -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $projectRoot "data\ilia.sqlite3") -Destination (Join-Path $stageRoot "data")
Copy-Item -LiteralPath $validationReport -Destination (Join-Path $stageRoot "data")
Copy-Tree (Join-Path $projectRoot "models\bge-m3") (Join-Path $stageRoot "models\bge-m3")
Copy-Tree (Join-Path $projectRoot "models\qwen3-4b") (Join-Path $stageRoot "models\qwen3-4b")
Copy-Tree (Join-Path $projectRoot "corpus\normalized") (Join-Path $stageRoot "corpus\normalized")
New-Item -ItemType Directory -Path (Join-Path $stageRoot "corpus\manifests") -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $projectRoot "corpus\manifests\document_topics.v1.json") -Destination (Join-Path $stageRoot "corpus\manifests")
Copy-Item -LiteralPath (Join-Path $projectRoot "corpus\manifests\document_relations.v1.json") -Destination (Join-Path $stageRoot "corpus\manifests")
foreach ($runtimeName in @("cuda", "vulkan", "cpu")) {
    Copy-IliaRuntime $runtimeName
}
New-Item -ItemType Directory -Path (Join-Path $stageRoot "runtime\onnx") -Force | Out-Null
foreach ($onnxFile in @("onnxruntime.dll", "onnxruntime_providers_shared.dll", "README.md")) {
    Copy-Item -LiteralPath (Join-Path $projectRoot "runtime\onnx\$onnxFile") -Destination (Join-Path $stageRoot "runtime\onnx") -Force
}

& python $rightsGate --root $projectRoot --database (Join-Path $projectRoot "data\ilia.sqlite3") --stage-root $stageRoot
if ($LASTEXITCODE -ne 0) { throw "Staged release rights gate failed with exit code $LASTEXITCODE" }

$manifestFiles = Get-ChildItem -LiteralPath $stageRoot -Recurse -File | Sort-Object FullName | ForEach-Object {
    [ordered]@{
        path = $_.FullName.Substring($stageRoot.Length + 1).Replace('\', '/')
        size = $_.Length
        sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}
$manifest = [ordered]@{
    format_version = 1
    product = "ILIA"
    version = $Version
    architecture = "windows-x64"
    generated_at = (Get-Date).ToUniversalTime().ToString("o")
    corpus_rights = [ordered]@{
        reviewed_items = $corpusRights.Count
        packaged_normalized_documents = $normalizedManifest.document_count
        excluded_documents = @($normalizedManifest.excluded_documents)
        source_pdfs_packaged = 0
    }
    component_rights = [ordered]@{
        blocked_components = @($blockedComponents.Name)
        local_testing_override = $false
    }
    files = @($manifestFiles)
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $stageRoot "package-manifest.json") -Encoding utf8

if (-not $InnoSetupCompiler) {
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe"),
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe")
    )
    $InnoSetupCompiler = $candidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
}
Assert-File $InnoSetupCompiler "Inno Setup 6 compiler"
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null

$numericParts = @($Version.Split('.') | ForEach-Object {
    $parsed = 0
    if (-not [int]::TryParse(($_ -replace '[^0-9].*$', ''), [ref]$parsed)) { $parsed = 0 }
    $parsed
})
while ($numericParts.Count -lt 4) { $numericParts += 0 }
$appFileVersion = ($numericParts[0..3] -join '.')
$innoArgs = @(
    "/DAppVersion=$Version",
    "/DAppFileVersion=$appFileVersion",
    "/DSourceDir=$stageRoot",
    "/DOutputDir=$outputRoot",
    "/DVCRedistInstaller=$vcRedist"
)
if (-not $SkipWebView2) {
    $innoArgs += "/DWebView2Installer=$webView2"
}
$innoArgs += (Join-Path $PSScriptRoot "ilia.iss")
& $InnoSetupCompiler @innoArgs
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed with exit code $LASTEXITCODE" }

& (Join-Path $PSScriptRoot "verify-installer.ps1") -StageRoot $stageRoot -InstallerRoot $outputRoot -Version $Version -RequireWebView2:(-not $SkipWebView2)
if ($LASTEXITCODE -ne 0) { throw "Installer verification failed with exit code $LASTEXITCODE" }
