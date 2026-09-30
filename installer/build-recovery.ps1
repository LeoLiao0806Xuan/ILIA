param(
    [string]$Version = "1.1.10",
    [string]$Package = "",
    [string]$InnoSetupCompiler = ""
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$outputRoot = Join-Path $projectRoot "dist\recovery"
if (-not $Package) {
    $Package = Join-Path $projectRoot "dist\update\ILIA-$Version-offline-update.ilia"
}
$packagePath = (Resolve-Path $Package).Path

if (-not $InnoSetupCompiler) {
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe"),
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe")
    )
    $InnoSetupCompiler = $candidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
}
if (-not $InnoSetupCompiler -or -not (Test-Path -LiteralPath $InnoSetupCompiler -PathType Leaf)) {
    throw "Inno Setup 6 compiler is missing"
}

New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
$parts = @($Version.Split('.') | ForEach-Object { [int]$_ })
while ($parts.Count -lt 4) { $parts += 0 }
$fileVersion = $parts[0..3] -join '.'
& $InnoSetupCompiler "/DAppVersion=$Version" "/DAppFileVersion=$fileVersion" "/DUpdatePackage=$packagePath" "/DOutputDir=$outputRoot" (Join-Path $PSScriptRoot "ilia-recovery.iss")
if ($LASTEXITCODE -ne 0) { throw "Recovery installer build failed with exit code $LASTEXITCODE" }

$output = Join-Path $outputRoot "ILIA-$Version-windows-x64-recovery.exe"
if (-not (Test-Path -LiteralPath $output -PathType Leaf)) { throw "Recovery installer output is missing: $output" }
[ordered]@{
    version = $Version
    output = $output
    bytes = (Get-Item -LiteralPath $output).Length
    sha256 = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
} | ConvertTo-Json
