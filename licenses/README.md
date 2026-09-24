# Installer license bundle

The installer copies this entire directory, the ILIA `LICENSE`, and
`THIRD_PARTY_NOTICES.md` into its `licenses` directory.

Run `tools/sync-third-party-licenses.ps1` to download the pinned authoritative
license artifacts for Qwen3-4B, llama.cpp, and ONNX Runtime. The script verifies
their SHA-256 hashes before replacing local copies. BGE-M3, CUDA, Visual C++ and
WebView2 provenance records are maintained directly in this directory.

Run `python tools/generate-dependency-notices.py` after restoring the locked
Rust and npm dependencies. It produces complete lockfile inventories and copies
locally available package licence/notice files into `dependencies/`. A missing
local notice is reported in `dependencies/summary.json`; it does not mean that
the package omitted a declared SPDX licence.

Run `python tools/generate-corpus-rights-matrix.py` to refresh the 50-item
corpus review. `CORPUS-TERMS.md` makes clear that Apache-2.0 applies to ILIA
source code, not to third-party legal materials.

`component-clearance.json` is the machine-readable release gate for bundled
binary components. A `blocked` component prevents a normal installer build.
