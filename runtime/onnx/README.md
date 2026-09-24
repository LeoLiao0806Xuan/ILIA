# ONNX Runtime

`onnxruntime.dll` is the CPU runtime used by the BGE-M3 embedding module. The binary is obtained from the official `onnxruntime` distribution and is not committed to source control.

The ILIA 1.0.0 payload records product version
`1.24.20260316.8.2d92497` and SHA-256
`87da0279ab54add397fd518f7022efe8b9a89fd968669d0adff9f9f242344b6c`.
Its pinned MIT license and upstream `ThirdPartyNotices.txt` are installed as
`licenses/ONNX-Runtime-MIT.txt` and
`licenses/ONNX-Runtime-ThirdPartyNotices.txt`.

Before running the embedder or hybrid search, set:

```powershell
$env:ORT_DYLIB_PATH = (Resolve-Path runtime/onnx/onnxruntime.dll)
```
