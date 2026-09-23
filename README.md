# ILIA 本地国际法资料与检索原型

当前版本已经从资料原型推进到可安装、可更新的本地国际法智能助手。

## 已实现

- 第一版 50 项资料范围和清单，50 份正式资料均已入库。
- SQLite 001 schema、FTS5、来源哈希、版本、法律性质、条款和判例段落结构。
- Rust workspace：`ilia-core`、`ilia-database`、`ilia-embedding`、`ilia-retrieval`、`ilia-inference`、`ilia-updater`、`ilia-corpus-embedder` 和 `ilia-eval-runner`。
- 中英文文书名、条款号和 ICJ 段落号精确检索。
- BGE-M3 INT8 ONNX 本地嵌入；5,254/5,254 个内容单元已生成 1024 维向量并写入 SQLite。
- FTS5 + BGE-M3 的 RRF 混合排序，以及带字符预算的证据选择。
- Qwen3-4B Q4_K_M + llama.cpp 本地问答；CUDA/Vulkan/CPU 自动探测与降级、托管子进程、无证据拒答和 `【n】` 引证校验。
- Tauri 2 桌面界面：研究问题、混合检索、本地问答、可点击引证、原文证据、页码、法律性质和受限官方来源链接已接通。
- 200 项检索基线评测，覆盖全部 50 份资料，当前 200/200 通过；Recall@5/10 为 1.0。
- Windows x64 离线安装包：随包部署数据库、BGE-M3、Qwen3-4B、CUDA/Vulkan/CPU、ONNX Runtime、MSVC DLL 与 WebView2 离线运行时，新电脑不需要开发环境。
- Ed25519 签名更新系统：分别支持应用、SQLite 资料增量、模型和运行时更新，并提供逐负载 SHA-256、更新日志、备份和失败自动回滚。
- 资料完整性校验通过，人工复核队列已清零。

当前库共含 50 份文件、5,254 个可引用内容单元，覆盖基础国际法文件、人权法、国际人道法和代表性 ICJ 判例。

## 复现

```powershell
python tools/corpus-importer/import_corpus.py
python tools/corpus-validator/validate_corpus.py
cargo test --workspace
$env:ORT_DYLIB_PATH = (Resolve-Path runtime/onnx/onnxruntime.dll)
cargo run --release -p ilia-corpus-embedder -- --db data/ilia_prototype.sqlite3 --cache-dir models/bge-m3
cargo run --release -p ilia-eval-runner -- --db data/ilia_prototype.sqlite3 --cases tests/eval/retrieval_baseline.jsonl --model-cache models/bge-m3 --output data/retrieval_eval_report.json
cargo run --release -p ilia-retrieval --bin ilia-search -- --db data/ilia_prototype.sqlite3 --model-cache models/bge-m3 --query "《联合国海洋法公约》领海宽度不得超过十二海里"
cargo run --release -p ilia-inference --bin ilia-runtime -- --runtime-root runtime --backend auto
cargo run --release -p ilia-inference --bin ilia-ask -- --db data/ilia_prototype.sqlite3 --bge-cache models/bge-m3 --runtime-root runtime --backend auto --qwen-model models/qwen3-4b/Qwen3-4B-Q4_K_M.gguf --question "《联合国海洋法公约》规定领海宽度不得超过多少海里？"
cd apps/desktop
npm install
npm run tauri -- dev
```

生成完整 Windows 离线安装包：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File installer/build-installer.ps1
```

`corpus/sources/` 保存不可变原始资料，`data/` 保存可重建数据库和校验报告。目录职责和实现状态见 `docs/repository_structure.md`。
检索融合和证据选择细节见 `docs/retrieval_mvp.md`。
本地问答层的安全边界、版本和运行方式见 `docs/local_qa.md`。
三档运行时的探测、选择和降级契约见 `docs/runtime_selection.md`。
安装包结构、构建和新电脑验收流程见 `docs/windows_installer.md`。
签名更新协议、发布制作和回滚流程见 `docs/update_system.md`。
桌面端命令、界面和构建方式见 `docs/desktop_mvp.md`。
