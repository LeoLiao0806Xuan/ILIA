# Qwen3-4B + llama.cpp 本地问答层

## 已实现链路

```text
用户问题
  ↓ BGE-M3 查询向量
FTS5 + 向量 RRF 检索
  ↓ 最多 5 条、总计 12,000 字符
证据包
  ↓ 受约束提示词
Qwen3-4B Q4_K_M / llama.cpp CUDA、Vulkan 或 CPU
  ↓
答案 + 【证据编号】
  ↓ 程序校验
chunk_id + citation_label
```

`ilia-inference` 负责：

- 启动和回收 `llama-server`，健康检查通过后才接受请求；服务仅监听 `127.0.0.1` 的临时端口，每次启动生成独立 API key，并限制 CORS 来源为 localhost。
- 使用 GGUF 内置 Jinja chat template，并通过 `enable_thinking=false` 和 `/no_think` 关闭思维输出。
- 按 Qwen 官方非思考模式建议使用 temperature 0.7、top-p 0.8、top-k 20、presence penalty 1.5。
- 明确把检索内容标记为不可信指令的数据，降低资料内提示词注入风险。
- 无证据时跳过模型并返回固定的“资料不足”答复。
- 只接受形如 `【n】` 且确实存在于证据包中的编号，映射回稳定的 `chunk_id` 和正式引用位置；另检查每个实质句均含引证，首次不合格时自动要求模型完整修订一次。

`grounded=true` 表示答案至少包含一个有效证据编号、没有越界编号，且每个实质句都带引证；`结论【1】。` 与 `结论。【1】` 两种句末格式均被接受。它是结构校验，不等同于自动证明引证在语义上完全支持该句。二次生成后仍未通过时，桌面端会保留回答但明确显示黄色警告，并要求用户以原文证据为准。

## 本机固定版本

- Qwen/Qwen3-4B-GGUF，revision `bc640142…`，Q4_K_M，2,497,280,256 bytes。
- llama.cpp `0.4.1-dev`，build `10964`，commit `b29c606e2`，Windows x64 CUDA 13.3、Vulkan 和 CPU 三套运行时。
- RTX 4070 Laptop GPU 8GB；自动模式优先 CUDA，并可依次降级到 Vulkan、CPU。

模型和运行时的完整哈希分别见 `models/qwen3-4b/model-manifest.json` 与 `runtime/*/runtime-manifest.json`。

## 运行

```powershell
$env:ORT_DYLIB_PATH = (Resolve-Path runtime/onnx/onnxruntime.dll)
cargo run --release -p ilia-inference --bin ilia-ask -- `
  --db data/ilia.sqlite3 `
  --bge-cache models/bge-m3 `
  --runtime-root runtime `
  --backend auto `
  --qwen-model models/qwen3-4b/Qwen3-4B-Q4_K_M.gguf `
  --question "《联合国海洋法公约》规定领海宽度不得超过多少海里？"
```

命令输出 JSON，顶层包含结构化运行时启动报告和问答结果。后者包含答案、引证映射、完整证据包、模型标识、token 数、耗时和校验警告。各后端日志分别写入 `data/llama-server-{backend}.log`，命令结束时子进程自动回收。自动探测和降级契约见 `docs/runtime_selection.md`。
