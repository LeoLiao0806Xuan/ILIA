$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "windows-toolchain.ps1")
Set-IliaGnuEnvironment

$cargoArgs = @($args | ForEach-Object {
    # PowerShell consumes a bare `--` while binding arguments to a .ps1 file.
    # Use `---` at the wrapper boundary and restore Cargo's separator here.
    if ($_ -eq "---") { "--" } else { $_ }
})

& cargo @cargoArgs
exit $LASTEXITCODE
