# Qwen3-4B + llama.cpp 本地问答层

## 1.1 工作区、导入与发布证据

- 工作区位于应用数据目录 `workspace.sqlite`，个人资料位于 `user.sqlite`；核心库仍只读。
- 个人资料支持 PDF 文本层、TXT、Markdown、HTML 和 DOCX。扫描 PDF 返回不支持 OCR；HTML 活动内容和 DOCX 宏/嵌入对象不会执行。
- `python tools/privacy_offline_audit.py --version 1.1.0 --output release/evidence/1.1.0/privacy_offline_report.json` 固定离线路径静态边界。
- `python tools/update_test_report.py --version 1.1.0 --output release/evidence/1.1.0/update-rollback-report.json` 记录更新安全与回滚测试。
- `python tools/aggregate_release_evidence.py --version 1.1.0 --evidence-dir release/evidence/1.1.0` 是严格门禁；开发阶段可加 `--allow-incomplete` 生成诚实缺项清单。

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
  ↓ 确定性逐句校验 + 受约束 JSON 语义支持检查
direct / summary / unsupported / conflict
  ↓ 最多一次安全重写；仍红的陈述删除
stable_key + chunk_id + citation_label
```

`ilia-inference` 负责：

- 启动和回收 `llama-server`，健康检查通过后才接受请求；服务仅监听 `127.0.0.1` 的临时端口，每次启动生成独立 API key，并限制 CORS 来源为 localhost。
- 使用 GGUF 内置 Jinja chat template，并通过 `enable_thinking=false` 和 `/no_think` 关闭思维输出。
- 按 Qwen 官方非思考模式建议使用 temperature 0.7、top-p 0.8、top-k 20、presence penalty 1.5。
- 明确把检索内容标记为不可信指令的数据，降低资料内提示词注入风险。
- 无证据时跳过模型并返回固定的“资料不足”答复。
- 只接受形如 `【n】` 且确实存在于证据包中的编号，映射回带资料库命名空间的稳定证据键、`chunk_id` 和正式引用位置；另检查每个实质句均含引证。
- 对结构合格的陈述使用严格 JSON schema 执行 `direct`、`summary`、`unsupported`、`conflict` 四级语义支持检查；缺项、编号漂移、陈述漂移或 JSON 解析失败一律按 `unsupported` 处理。
- 红色陈述触发最多一次完整安全重写；复核后仍为红色的陈述由程序删除，并显示固定的“部分结论因缺少证据未输出”。证据文本始终被标记为不可信数据。

`grounded=true` 表示最终保留的答案至少包含一个有效证据编号、没有越界编号，且每个实质句都带引证；`结论【1】。` 与 `结论。【1】` 两种句末格式均被接受。四级标签是本地模型的自动支持度判断，不是法律真实性证明。人工对照集位于 `tests/eval/citation_support_1.1.jsonl`，四种标签各 3 项并覆盖中英文。

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
  --question "《联合国海洋法公约》规定领海宽度不得超过多少海里？" `
  --stream --audit
```

命令输出 JSON，顶层包含结构化运行时启动报告和问答结果。后者包含答案、引证映射、完整证据包、模型标识、token 数、耗时和校验警告。各后端日志分别写入 `data/llama-server-{backend}.log`，命令结束时子进程自动回收。自动探测和降级契约见 `docs/runtime_selection.md`。

桌面端 1.1 研究入口使用 llama.cpp 的 SSE 响应。独立读取线程将每个 `data:` 片段送入 200ms 轮询的控制通道，再作为 `answer_delta` Tauri 事件发送；因此慢首段不会阻断取消。安全重写或红句删除后发送一次 `answer_replaced`，前端以新文本替换草稿。快速查询完全跳过运行时启动。
