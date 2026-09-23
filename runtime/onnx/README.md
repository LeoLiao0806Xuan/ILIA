# ONNX Runtime

`onnxruntime.dll` is the CPU runtime used by the BGE-M3 embedding module. The binary is obtained from the official `onnxruntime` distribution and is not committed to source control.

Before running the embedder or hybrid search, set:

```powershell
$env:ORT_DYLIB_PATH = (Resolve-Path runtime/onnx/onnxruntime.dll)
```
