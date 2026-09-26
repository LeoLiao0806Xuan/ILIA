# Tauri 桌面 MVP

## 1.1 研究工作流

桌面端主导航现包含研究、项目、资料库和设置。项目区保存会话、证据快照与自动保存笔记并导出安全 Markdown/HTML；资料库区区分核心和个人资料，执行五格式预览导入、去重、删除与重建；设置区提供模型预热、运行后端、三档性能、资源估算、空闲显存回收、更新代理和本地 `.ilia` 包入口。证据阅读器支持原文、中文和对照三态，格式化引文可复制为普通中文、OSCOLA、Bluebook、ICJ、Markdown 或纯文本。

## 已接通范围

- `search_documents`：调用 BGE-M3 + FTS5/RRF 混合检索，返回证据包。
- `ask_question`：保留的 1.0.x 兼容命令，阻塞式返回带 `【n】` 引证的回答。
- `start_research`：1.1 研究入口；支持快速、标准、深度三模式，通过 `research-event` 按请求 ID 发送检索、计划、回答片段、完成、取消或失败事件。
- `cancel_research`：取消指定活动请求；新请求与窗口销毁也复用相同取消令牌。

快速模式不启动 Qwen。标准模式最多向模型提供 5 条证据。深度模式固定产生 3 个可见子问题，每个只检索一次，去重并受全局证据预算约束后生成六段研究答复。任一时刻只保留一个活动研究请求；新请求会先取消旧请求。

研究事件在流式草稿后执行逐句引证审计。若发生一次安全重写或红句删除，后端发送 `answer_replaced` 覆盖草稿，随后发送含四级标签的 `citation_audit_completed`。回答中的 `【n】` 通过 `AnswerCitation.stable_key` 定位证据，不依赖当前结果数组顺序。
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

`scripts/tauri.ps1` 会为 Tauri 子进程选择 `stable-x86_64-pc-windows-gnu`，并将同一套 MSYS2 UCRT64 DLL 放到 `PATH` 最前面。这可避免 Anaconda 附带的 MinGW DLL 被 `cc1.exe` 错误加载。脚本从 `PATH` 自动发现 `ucrt64/bin`；未加入 `PATH` 时可通过 `ILIA_MSYS_ROOT` 指定安装根目录。仓库不保存任何本机编译器绝对路径，GitHub CI 继续使用云端 Windows MSVC 工具链。

开发时默认从仓库根目录读取：

- `data/ilia.sqlite3`
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
- 真实混合检索已验证：领海宽度问题首条命中 `UNCLOS, Article 3`，同时返回来源版页码、规范化正文和联合国官方来源链接。
- 1440×900 三栏布局、检索态、回答态、引证和原文联动已做视觉验收。
