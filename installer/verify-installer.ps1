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
    "data\ilia_prototype.sqlite3",
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

$summary = [ordered]@{
    status = "passed"
    version = $Version
    staged_files = @($manifest.files).Count + 1
    staged_bytes = (Get-ChildItem -LiteralPath $StageRoot -Recurse -File | Measure-Object Length -Sum).Sum
    installer_launcher = $setup
    installer_slices = @($slices.FullName)
    installer_bytes = ((Get-Item -LiteralPath $setup).Length + ($slices | Measure-Object Length -Sum).Sum)
}
$summary | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $InstallerRoot "installer-verification.json") -Encoding utf8
$summary | ConvertTo-Json -Depth 4
