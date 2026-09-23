function Set-IliaGnuEnvironment {
    $ucrtBin = $null
    if ($env:ILIA_MSYS_ROOT) {
        $candidate = Join-Path $env:ILIA_MSYS_ROOT "ucrt64\bin"
        if (Test-Path -LiteralPath (Join-Path $candidate "gcc.exe") -PathType Leaf) {
            $ucrtBin = $candidate
        }
    } else {
        foreach ($entry in ($env:Path -split ";")) {
            if (-not $entry) { continue }
            $candidate = $entry.Trim().Trim('"')
            if ((Split-Path $candidate -Leaf) -ieq "bin" -and
                (Split-Path (Split-Path $candidate -Parent) -Leaf) -ieq "ucrt64" -and
                (Test-Path -LiteralPath (Join-Path $candidate "gcc.exe") -PathType Leaf)) {
                $ucrtBin = $candidate
                break
            }
        }
    }

    if (-not $ucrtBin) {
        throw "MSYS2 UCRT64 was not found on PATH. Add its ucrt64/bin directory to PATH or set ILIA_MSYS_ROOT."
    }

    $msysRoot = Split-Path (Split-Path $ucrtBin -Parent) -Parent
    $msysBin = Join-Path $msysRoot "usr\bin"
    $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
    $env:Path = "$ucrtBin;$msysBin;$cargoBin;$env:Path"
    $env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-gnu"
}
