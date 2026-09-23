$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "..\..\..\tools\windows-toolchain.ps1")
Set-IliaGnuEnvironment

$tauri = Join-Path $PSScriptRoot "..\node_modules\.bin\tauri.cmd"
if (-not (Test-Path -LiteralPath $tauri)) {
    throw "Desktop dependencies are missing. Run npm install in apps/desktop first."
}

& $tauri @args
exit $LASTEXITCODE
