# NVIDIA CUDA runtime redistribution record

The ILIA next-release working tree bundles only these CUDA 13.3 runtime libraries from the pinned
llama.cpp `b10964` CUDA archive; it does not redistribute the CUDA Toolkit:

| File | SHA-256 |
| --- | --- |
| `cudart64_13.dll` | `b00ca6f53699120da815bf3e06e2e4285fae2f201235b883dcbb50eec51e2a2a` |
| `cublas64_13.dll` | `f1d500d0cd892f5b8c6b6cdbffd82d0c55d5f5427215668e7ceb55aeeccc1b63` |
| `cublasLt64_13.dll` | `b592cd016d7673e9cb97716a22b27c4010ee635377a3ba28f37070a9bdb76a68` |

Archive source:
<https://github.com/ggml-org/llama.cpp/releases/download/b10964/cudart-llama-bin-win-cuda-13.3-x64.zip>

The NVIDIA CUDA EULA, last updated 2026-01-26 when this record was prepared,
lists versioned forms of `cudart.dll`, `cublas.dll`, and `cublasLt.dll` in
Attachment A as redistributable with an application, subject to the agreement's
distribution requirements:
<https://docs.nvidia.com/cuda/eula/index.html#attachment-a>

No separate NVIDIA application or fee is required by that public grant. The
release owner must re-check the then-current EULA before changing CUDA versions
or adding NVIDIA files.

## Implemented distribution controls

Attachment A eligibility is necessary but not sufficient. ILIA therefore:

* verifies the three DLL hashes against `cuda-redistributables.json`;
* stages CUDA only inside ILIA's private runtime directory;
* packages only the llama server and dependent libraries, not the unrelated
  llama.cpp command-line utilities from the upstream archive;
* includes the official NVIDIA EULA snapshot and its SHA-256 provenance; and
* requires acceptance of `INSTALLER-THIRD-PARTY-TERMS.txt`, which applies the
  NVIDIA restrictions only to NVIDIA components and not to ILIA's Apache-2.0
  source code.

External legal review of the installer language remains recommended before a
public release.
