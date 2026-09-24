param(
    [Parameter(Mandatory = $true)][string]$StageRoot,
    [Parameter(Mandatory = $true)][string]$InstallerRoot,
    [Parameter(Mandatory = $true)][string]$Version,
    [switch]$RequireWebView2
)

$ErrorActionPreference = "Stop"
$required = @(
    "ilia-desktop.exe",
    "WebView2Loader.dll",
    "ilia-updater.exe",
    "icon.ico",
    "THIRD_PARTY_NOTICES.md",
    "licenses\ILIA-Apache-2.0.txt",
    "licenses\THIRD_PARTY_NOTICES.md",
    "licenses\README.md",
    "licenses\BGE-M3-MIT.txt",
    "licenses\CORPUS-TERMS.md",
    "licenses\MICROSOFT-RUNTIME-REDISTRIBUTION.md",
    "licenses\microsoft-redistributables.json",
    "licenses\NVIDIA-CUDA-EULA-2026-01-26.html",
    "licenses\NVIDIA-CUDA-REDISTRIBUTION.md",
    "licenses\ONNX-Runtime-MIT.txt",
    "licenses\ONNX-Runtime-ThirdPartyNotices.txt",
    "licenses\Qwen3-4B-Apache-2.0.txt",
    "licenses\component-clearance.json",
    "licenses\corpus-redistribution-rights-matrix.csv",
    "licenses\cuda-redistributables.json",
    "licenses\dependencies\NPM-NOTICES.md",
    "licenses\dependencies\RUST-NOTICES.md",
    "licenses\dependencies\npm-dependencies.csv",
    "licenses\dependencies\rust-dependencies.csv",
    "licenses\dependencies\summary.json",
    "licenses\downloaded-license-manifest.json",
    "licenses\INSTALLER-THIRD-PARTY-TERMS.txt",
    "licenses\llama.cpp-MIT.txt",
    "data\ilia.sqlite3",
    "corpus\normalized\manifest.json",
    "models\bge-m3\model.onnx",
    "models\qwen3-4b\Qwen3-4B-Q4_K_M.gguf",
    "runtime\cuda\llama-server.exe",
    "runtime\vulkan\llama-server.exe",
    "runtime\cpu\llama-server.exe",
    "runtime\onnx\onnxruntime.dll",
    "package-manifest.json",
    "update\trusted-key.json",
    ".ilia-update\versions.json"
)
foreach ($relative in $required) {
    $path = Join-Path $StageRoot $relative
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Staged package is missing $relative" }
}

$manifest = Get-Content -Raw -LiteralPath (Join-Path $StageRoot "package-manifest.json") | ConvertFrom-Json
if ($manifest.version -ne $Version) { throw "Package manifest version mismatch" }
if ($manifest.corpus_rights.reviewed_items -ne 50) { throw "Package manifest corpus rights count mismatch" }
if ($manifest.corpus_rights.packaged_normalized_documents -ne 49) { throw "Package must contain 49 normalized documents" }
if ($manifest.corpus_rights.source_pdfs_packaged -ne 0) { throw "Package must not contain source PDFs" }
if (@($manifest.corpus_rights.excluded_documents).Count -ne 1 -or $manifest.corpus_rights.excluded_documents[0] -ne "icrc-cihl-rules") { throw "Package must exclude icrc-cihl-rules" }
if ($manifest.component_rights.local_testing_override -and $Version -notmatch '(?i)(dev|test|local|rc|alpha|beta)') {
    throw "A package containing the component-rights local testing override must use a prerelease/test version"
}

$normalizedManifest = Get-Content -Raw -LiteralPath (Join-Path $StageRoot "corpus\normalized\manifest.json") | ConvertFrom-Json
if ($normalizedManifest.document_count -ne 49 -or @($normalizedManifest.artifacts).Count -ne 49) { throw "Staged normalized corpus count mismatch" }
foreach ($artifact in $normalizedManifest.artifacts) {
    $path = Join-Path $StageRoot ($artifact.path.Replace('/', '\'))
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Normalized corpus artifact is missing: $($artifact.path)" }
    if ((Get-Item -LiteralPath $path).Length -ne $artifact.byte_length) { throw "Normalized corpus size mismatch: $($artifact.path)" }
    $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $artifact.sha256) { throw "Normalized corpus SHA-256 mismatch: $($artifact.path)" }
}
if (@(Get-ChildItem -LiteralPath (Join-Path $StageRoot "corpus") -Recurse -File -Filter "*.pdf").Count -ne 0) {
    throw "Source PDFs must not be present in the staged corpus"
}
foreach ($runtimeName in @("cuda", "vulkan", "cpu")) {
    $unexpectedExecutables = @(Get-ChildItem -LiteralPath (Join-Path $StageRoot "runtime\$runtimeName") -File -Filter "*.exe" | Where-Object { $_.Name -ne "llama-server.exe" })
    if ($unexpectedExecutables.Count -ne 0) { throw "Unexpected runtime executables: $($unexpectedExecutables.Name -join ', ')" }
}
$cudaInventory = Get-Content -Raw -LiteralPath (Join-Path $StageRoot "licenses\cuda-redistributables.json") | ConvertFrom-Json
foreach ($entry in $cudaInventory.files.psobject.Properties) {
    $path = Join-Path $StageRoot "runtime\cuda\$($entry.Name)"
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "CUDA redistributable is missing: $($entry.Name)" }
    $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $entry.Value) { throw "CUDA redistributable SHA-256 mismatch: $($entry.Name)" }
}
foreach ($file in $manifest.files) {
    $path = Join-Path $StageRoot ($file.path.Replace('/', '\'))
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Manifest file is missing: $($file.path)" }
    if ((Get-Item -LiteralPath $path).Length -ne $file.size) { throw "Size mismatch: $($file.path)" }
    $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $file.sha256) { throw "SHA-256 mismatch: $($file.path)" }
}

$baseName = "ILIA-$Version-windows-x64-offline-setup"
$setup = Join-Path $InstallerRoot "$baseName.exe"
if (-not (Test-Path -LiteralPath $setup -PathType Leaf)) { throw "Installer launcher is missing: $setup" }
$slices = @(Get-ChildItem -LiteralPath $InstallerRoot -Filter "$baseName-*.bin" -File)
if ($slices.Count -eq 0) { throw "Installer payload slices are missing" }
if ($RequireWebView2) {
    $webView2 = Join-Path (Split-Path $PSScriptRoot -Parent) "installer\prerequisites\MicrosoftEdgeWebView2RuntimeInstallerX64.exe"
    if (-not (Test-Path -LiteralPath $webView2 -PathType Leaf)) { throw "WebView2 offline installer is missing" }
}

$installerFiles = @(Get-ChildItem -LiteralPath $InstallerRoot -Filter "$baseName*" -File | Sort-Object Name)
$installerHashes = @($installerFiles | ForEach-Object {
    [ordered]@{
        name = $_.Name
        size = $_.Length
        generated_at = $_.LastWriteTimeUtc.ToString("o")
        sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        authenticode_status = (Get-AuthenticodeSignature -LiteralPath $_.FullName).Status.ToString()
    }
})
$installerHashes | ForEach-Object { "$($_.sha256)  $($_.name)" } |
    Set-Content -LiteralPath (Join-Path $InstallerRoot "SHA256SUMS.txt") -Encoding utf8

$summary = [ordered]@{
    status = "passed"
    version = $Version
    verified_at = (Get-Date).ToUniversalTime().ToString("o")
    package_generated_at = $manifest.generated_at
    staged_files = @($manifest.files).Count + 1
    staged_bytes = (Get-ChildItem -LiteralPath $StageRoot -Recurse -File | Measure-Object Length -Sum).Sum
    installer_launcher = (Split-Path $setup -Leaf)
    installer_slices = @($slices.Name)
    installer_bytes = ((Get-Item -LiteralPath $setup).Length + ($slices | Measure-Object Length -Sum).Sum)
    installer_files = $installerHashes
    desktop_authenticode_status = (Get-AuthenticodeSignature -LiteralPath (Join-Path $StageRoot "ilia-desktop.exe")).Status.ToString()
}
$summary | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $InstallerRoot "installer-verification.json") -Encoding utf8
$summary | ConvertTo-Json -Depth 4
