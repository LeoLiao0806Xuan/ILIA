param(
    [string]$Version = "1.0.0",
    [string]$InnoSetupCompiler = "",
    [switch]$SkipBuild,
    [switch]$SkipWebView2
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
$trustedUpdateKey = Join-Path $projectRoot "update\trusted-key.json"

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

Assert-File (Join-Path $projectRoot "data\ilia_prototype.sqlite3") "SQLite database"
Assert-File (Join-Path $projectRoot "models\bge-m3\model.onnx") "BGE-M3 model"
Assert-File (Join-Path $projectRoot "models\qwen3-4b\Qwen3-4B-Q4_K_M.gguf") "Qwen3-4B model"
foreach ($backend in @("cuda", "vulkan", "cpu")) {
    Assert-File (Join-Path $projectRoot "runtime\$backend\llama-server.exe") "$backend llama.cpp runtime"
}
Assert-File (Join-Path $projectRoot "runtime\onnx\onnxruntime.dll") "ONNX Runtime"
Assert-File $trustedUpdateKey "trusted update public key"
if (-not $SkipWebView2) {
    Assert-File $webView2 "WebView2 offline installer"
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
Copy-Item -LiteralPath (Join-Path $projectRoot "data\ilia_prototype.sqlite3") -Destination (Join-Path $stageRoot "data")
Copy-Tree (Join-Path $projectRoot "models\bge-m3") (Join-Path $stageRoot "models\bge-m3")
Copy-Tree (Join-Path $projectRoot "models\qwen3-4b") (Join-Path $stageRoot "models\qwen3-4b")
Copy-Tree (Join-Path $projectRoot "corpus\sources") (Join-Path $stageRoot "corpus\sources")
foreach ($runtimeName in @("cuda", "vulkan", "cpu", "onnx")) {
    Copy-Tree (Join-Path $projectRoot "runtime\$runtimeName") (Join-Path $stageRoot "runtime\$runtimeName")
}

$vcRuntimeNames = @("msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll")
foreach ($runtimeName in @("cuda", "vulkan", "cpu", "onnx")) {
    foreach ($dll in $vcRuntimeNames) {
        $source = Join-Path $env:WINDIR "System32\$dll"
        Assert-File $source "Microsoft VC runtime $dll"
        Copy-Item -LiteralPath $source -Destination (Join-Path $stageRoot "runtime\$runtimeName\$dll") -Force
    }
}

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

$innoArgs = @(
    "/DAppVersion=$Version",
    "/DAppFileVersion=1.0.0.0",
    "/DSourceDir=$stageRoot",
    "/DOutputDir=$outputRoot"
)
if (-not $SkipWebView2) {
    $innoArgs += "/DWebView2Installer=$webView2"
}
$innoArgs += (Join-Path $PSScriptRoot "ilia.iss")
& $InnoSetupCompiler @innoArgs
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed with exit code $LASTEXITCODE" }

& (Join-Path $PSScriptRoot "verify-installer.ps1") -StageRoot $stageRoot -InstallerRoot $outputRoot -Version $Version -RequireWebView2:(-not $SkipWebView2)
if ($LASTEXITCODE -ne 0) { throw "Installer verification failed with exit code $LASTEXITCODE" }
