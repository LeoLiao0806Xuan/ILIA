# Contributing

ILIA is a Rust workspace with a Tauri 2 desktop frontend. Pull requests should keep source code, downloadable assets and generated data separate.

Before opening a pull request, run:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm ci --prefix apps/desktop
npm run build --prefix apps/desktop
```

Do not commit model weights, SQLite databases, llama.cpp binaries, downloaded PDFs, installer payloads, update private keys or generated release directories. Update manifests must be signed only by the release owner using the private key described in `docs/update_system.md`.
