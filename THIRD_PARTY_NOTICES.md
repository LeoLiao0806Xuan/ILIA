# Third-Party Notices — ILIA 1.0.0-rc.1

This document is a release-candidate inventory, not a final legal approval. Entries marked **pending** block public distribution of the bundled installer until the release owner or legal reviewer records the applicable redistribution terms and required notices.

| Component | Bundled artifact | License/source status | Review status |
| --- | --- | --- | --- |
| Qwen3-4B GGUF | `models/qwen3-4b/Qwen3-4B-Q4_K_M.gguf` | Apache-2.0 metadata at <https://huggingface.co/Qwen/Qwen3-4B-GGUF> | Source metadata verified; final notice packaging pending |
| BGE-M3 | Base embedding model | MIT metadata at <https://huggingface.co/BAAI/bge-m3> | Source metadata verified; final notice packaging pending |
| gpahal BGE-M3 ONNX INT8 conversion | `models/bge-m3/model.onnx` | MIT metadata at <https://huggingface.co/gpahal/bge-m3-onnx-int8> | Source metadata verified; exact conversion notice review pending |
| llama.cpp / ggml | CPU, Vulkan and CUDA inference executables and libraries | MIT; upstream at <https://github.com/ggml-org/llama.cpp> | License identified; transitive binary notices pending |
| ONNX Runtime | `runtime/onnx/*.dll` | MIT; upstream at <https://github.com/microsoft/onnxruntime> | License identified; upstream `ThirdPartyNotices.txt` packaging pending |
| LLVM OpenMP runtime | `libomp.dll` | Apache-2.0 WITH LLVM-exception | License file is present in the runtime payload |
| NVIDIA CUDA runtime libraries | CUDA DLLs in `runtime/cuda` | NVIDIA CUDA Toolkit redistribution terms | **Pending release-owner/legal confirmation** |
| Microsoft Visual C++ runtime | MSVC DLLs copied into runtime directories | Microsoft Visual C++ Redistributable terms | **Pending release-owner/legal confirmation** |
| Microsoft Edge WebView2 Runtime | Offline bootstrapper in the installer | Microsoft WebView2 distribution terms | **Pending release-owner/legal confirmation** |
| Tauri, Rust and npm dependencies | Desktop application binary and frontend assets | Multiple open-source licenses | Automated dependency license bundle pending |
| International-law corpus | 50 official legal documents from UN, ICJ, OHCHR, ICRC and related official sources | Source-specific terms and public-document policies | **All 50 redistribution reviews pending** |

## Included copyright notice

llama.cpp / ggml: Copyright (c) 2023-2026 The ggml authors. Licensed under the MIT License.

ONNX Runtime: Copyright (c) Microsoft Corporation. Licensed under the MIT License.

## Release rule

The `1.0.0-rc.1` installer is an internal release candidate. Do not attach it to a public GitHub Release until every pending row above is resolved, required license texts and third-party notices are included in the staged installer, and the 50 corpus entries no longer have `license_review = pending`.
