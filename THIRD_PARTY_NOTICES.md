# Third-Party Notices — ILIA 1.0.1

This document records third-party components and source provenance for ILIA
1.0.1. It does not retroactively describe the published v1.0.0
installer. Authoritative license files and redistribution records are installed
under `licenses/`. Entries marked **pending** require follow-up by the release
owner.

| Component | Bundled artifact | License/source status | Review status |
| --- | --- | --- | --- |
| Qwen3-4B GGUF | `models/qwen3-4b/Qwen3-4B-Q4_K_M.gguf`, revision `bc640142c66e1fdd12af0bd68f40445458f3869b` | Apache-2.0 at <https://huggingface.co/Qwen/Qwen3-4B-GGUF> | Pinned upstream license packaged as `licenses/Qwen3-4B-Apache-2.0.txt` |
| BGE-M3 | Base model revision `5617a9f61b028005a4858fdac845db406aefb181` | MIT model-card metadata at <https://huggingface.co/BAAI/bge-m3> | Pinned metadata and standard terms recorded in `licenses/BGE-M3-MIT.txt`; upstream publishes no standalone LICENSE file |
| gpahal BGE-M3 ONNX INT8 conversion | `models/bge-m3/model.onnx`, revision `2b34e84df040034d4b9eabb62383a87c18955822` | MIT model-card metadata at <https://huggingface.co/gpahal/bge-m3-onnx-int8> | Pinned conversion provenance recorded in `licenses/BGE-M3-MIT.txt`; upstream publishes no standalone LICENSE file |
| llama.cpp / ggml | CPU, Vulkan and CUDA build `b10964`, commit `b29c606e2` | MIT; upstream at <https://github.com/ggml-org/llama.cpp> | Pinned upstream license packaged as `licenses/llama.cpp-MIT.txt` |
| ONNX Runtime | `runtime/onnx/onnxruntime.dll`, product version `1.24.20260316.8.2d92497` | MIT; upstream at <https://github.com/microsoft/onnxruntime> | Pinned MIT license and `ThirdPartyNotices.txt` from commit `2d92497` packaged under `licenses/` |
| LLVM OpenMP runtime | `libomp.dll` | Apache-2.0 WITH LLVM-exception | License file is present in the runtime payload |
| NVIDIA CUDA runtime libraries | `cudart64_13.dll`, `cublas64_13.dll`, `cublasLt64_13.dll` | NVIDIA CUDA EULA Attachment A | Exact source and hashes pinned; NVIDIA EULA and downstream installer terms packaged; build restricts these files to ILIA's application runtime and rejects hash drift |
| Microsoft Visual C++ runtime | Official signed `VC_redist.x64.exe` 14.51.36247.0 | Microsoft Visual Studio Redistributable terms | Source, size, SHA-256 and Microsoft Authenticode signer are enforced; the unmodified package is installed silently before ILIA starts |
| Microsoft Edge WebView2 Runtime | Evergreen x64 Standalone Installer | Microsoft-supported offline deployment workflow | Installer version, hash and official distribution reference recorded in `licenses/MICROSOFT-RUNTIME-REDISTRIBUTION.md` |
| Tauri, Rust and npm dependencies | Desktop application binary and frontend assets | Multiple open-source licenses | Complete locked inventories and locally available notices generated under `licenses/dependencies/`; missing local notice files are explicitly counted in `summary.json` |
| International-law corpus | 49 ILIA-generated normalized legal-text artifacts from UN, ICJ, OHCHR, ICRC and related official sources | Source-specific terms; not covered by ILIA's Apache-2.0 licence | No source PDF is packaged; artifact hashes and provenance are recorded in `corpus/normalized/manifest.json`; ICRC customary IHL rules are excluded |

## Included copyright notices

llama.cpp / ggml: Copyright (c) 2023-2026 The ggml authors. Licensed under the MIT License.

ONNX Runtime: Copyright (c) Microsoft Corporation. Licensed under the MIT License.

The authoritative texts packaged under `licenses/` control over this summary.

## Corpus licence boundary

ILIA is currently released free of charge as a non-commercial open-source
project. Open-source distribution is still redistribution. The Apache-2.0
licence applies to ILIA source code only and must not be represented as granting
rights in corpus materials. See `licenses/CORPUS-TERMS.md`.

## Release responsibility

The release owner is responsible for maintaining this inventory and completing any pending source-specific notice or redistribution review for future corpus and runtime updates.
