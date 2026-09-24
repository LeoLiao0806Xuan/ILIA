# Contributing

ILIA is a Rust workspace with a Tauri 2 desktop frontend. Pull requests should keep source code, downloadable assets and generated data separate.

On Windows, invoke Cargo through `tools/cargo.ps1`. It puts the project's MSYS2 UCRT64 runtime before Anaconda's older MinGW DLLs and prevents the `cc1.exe` `clock_gettime64` entry-point error.

Before opening a pull request, run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 fmt --all --- --check
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 clippy --workspace --all-targets --- -D warnings
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 test --workspace
npm ci --prefix apps/desktop
npm run build --prefix apps/desktop
```

PowerShell removes a bare `--` when it invokes a `.ps1` file. In commands that need Cargo's argument separator, write `---`; the wrapper converts it back to `--` before starting Cargo. GitHub Actions uses Cargo directly and therefore keeps the standard `--` syntax.

Do not commit model weights, ad-hoc SQLite databases, llama.cpp binaries, downloaded PDFs, installer payloads, update private keys or generated release directories. The reviewed `data/ilia_prototype.sqlite3` baseline and its deterministic distributable derivative `data/ilia.sqlite3` are the only SQLite exceptions. Update manifests must be signed only by the release owner using the private key described in `docs/update_system.md`.
