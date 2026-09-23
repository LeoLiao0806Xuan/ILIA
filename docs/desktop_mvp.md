# Tauri 桌面 MVP

## 已接通范围

- `search_documents`：调用 BGE-M3 + FTS5/RRF 混合检索，返回证据包。
- `ask_question`：复用检索证据，按 CUDA → Vulkan → CPU 降级启动 Qwen3-4B / llama.cpp，并返回带 `【n】` 引证的回答。
- `get_runtime_status` / `set_runtime_preference`：展示或切换自动、CUDA、Vulkan、CPU 运行偏好。
- 证据区展示文书名、规范引用和摘要；原文区展示语言、页码、法律性质、完整文本与官方来源。
- 官方来源通过 Tauri opener 交给系统浏览器；能力范围只允许当前语料涉及的 UN、ICJ、OHCHR 与 Internet Archive 域名。

前端位于 `apps/desktop/src/`，Rust 命令层位于 `apps/desktop/src-tauri/src/lib.rs`。桌面层只编排现有 crate，不复制数据库、检索或推理逻辑。

## 开发运行

```powershell
cd apps/desktop
npm install
npm run tauri -- dev
```

`scripts/tauri.ps1` 会为 Tauri 子进程选择 `stable-x86_64-pc-windows-gnu`，并将同一套 MSYS2 UCRT64 DLL 放到 `PATH` 最前面。这可避免 Anaconda 附带的 MinGW DLL 被 `cc1.exe` 错误加载。默认 MSYS2 根目录是 `D:\msys`，其他位置可通过 `ILIA_MSYS_ROOT` 指定。

开发时默认从仓库根目录读取：

- `data/ilia_prototype.sqlite3`
- `models/bge-m3/`
- `models/qwen3-4b/Qwen3-4B-Q4_K_M.gguf`
- `runtime/onnx/`、`runtime/cuda/`、`runtime/vulkan/`、`runtime/cpu/`

如需从其他根目录启动，可设置 `ILIA_ROOT`。

## 构建

```powershell
cd apps/desktop
npm run build
npm run tauri -- build --no-bundle
```

当前生成未打包的 Windows 程序：

```text
target/x86_64-pc-windows-gnu/release/ilia-desktop.exe
```

安装包、资源随包分发和更新系统仍属于后续发行阶段；当前程序按开发版约定从 `ILIA_ROOT` 或仓库根目录加载数据库、模型和运行时。

## 验证

- TypeScript/Vite 生产构建通过。
- Rust workspace：14 项单元/集成测试全部通过。
- 真实混合检索已验证：领海宽度问题首条命中 `UNCLOS, Article 3`，同时返回第 21 页、原文和联合国官方 PDF。
- 1440×900 三栏布局、检索态、回答态、引证和原文联动已做视觉验收。
