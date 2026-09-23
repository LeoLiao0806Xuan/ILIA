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
