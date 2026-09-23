$ErrorActionPreference = "Stop"

$msysRoot = if ($env:ILIA_MSYS_ROOT) { $env:ILIA_MSYS_ROOT } else { "D:\msys" }
$ucrtBin = Join-Path $msysRoot "ucrt64\bin"
$msysBin = Join-Path $msysRoot "usr\bin"
$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"

if (-not (Test-Path -LiteralPath (Join-Path $ucrtBin "gcc.exe"))) {
    throw "MSYS2 UCRT64 compiler not found at $ucrtBin. Set ILIA_MSYS_ROOT to override the MSYS2 root."
}

# cc1.exe lives below GCC's lib directory, so Windows searches PATH for its DLLs.
# Put the matching UCRT64 DLLs first to avoid MinGW DLLs bundled by Anaconda.
$env:Path = "$ucrtBin;$msysBin;$cargoBin;$env:Path"
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-gnu"

$tauri = Join-Path $PSScriptRoot "..\node_modules\.bin\tauri.cmd"
if (-not (Test-Path -LiteralPath $tauri)) {
    throw "Desktop dependencies are missing. Run npm install in apps/desktop first."
}

& $tauri @args
exit $LASTEXITCODE
