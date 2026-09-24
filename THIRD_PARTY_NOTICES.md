# Third-Party Notices — ILIA 1.0.0

This document records third-party components and source provenance for the 1.0.0 release. Entries marked **pending** require follow-up by the release owner before any change to the corresponding redistributed material.

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

## Release responsibility

The release owner is responsible for maintaining this inventory and completing any pending source-specific notice or redistribution review for future corpus and runtime updates.
