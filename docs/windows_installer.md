# Windows 离线安装包

ILIA 的 Windows x64 安装包包含桌面程序、正式 SQLite 数据库、BGE-M3、Qwen3-4B Q4_K_M、ONNX Runtime，以及 llama.cpp 的 CUDA、Vulkan、CPU 三套运行时。安装后不需要 Rust、Node.js、Python、MSYS2 或其他开发环境。

安装包同时携带 Microsoft Edge WebView2 Evergreen x64 离线安装器；目标电脑尚未安装 WebView2 时会静默补装。llama.cpp 和 ONNX Runtime 所需的 MSVC 动态库随各运行时目录部署。

## 构建

构建机需要 Node.js、Rust GNU 工具链、MSYS2 UCRT64、Inno Setup 6，以及：

`installer/prerequisites/MicrosoftEdgeWebView2RuntimeInstallerX64.exe`

执行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File installer/build-installer.ps1
```

默认构建版本为 `1.0.0`。安装器和桌面 EXE 当前没有 Authenticode 签名；发行包通过项目 GitHub Release 分发，并附带构建生成的 `SHA256SUMS.txt`。用户可运行 `Get-FileHash -Algorithm SHA256 <文件>` 与清单逐项比对。

产物位于 `dist/installer/`。由于完整离线负载超过单个安装数据文件的安全上限，交付物由一个 `setup.exe` 和若干 `.bin` 数据片组成；它们必须放在同一目录。用户只需运行 `setup.exe`。

构建脚本会生成逐文件 SHA-256 清单，并在结束时校验安装负载、模型、数据库、三套 llama.cpp 运行时和所有安装片段。校验摘要位于 `dist/installer/installer-verification.json`。

在构建完成后执行候选版冒烟测试：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File installer/smoke-test.ps1
```

脚本默认强制使用 CPU 后端，以便构建机结果不依赖 CUDA；也可用 `-Backend auto` 单独验证自动选择。测试会把最新报告同时写入 `dist/installer/installer-smoke-test.json` 和 `release/evidence/1.0.0/windows-installer-smoke.json`。

## 新电脑验收

构建机验收脚本会静默安装最终介质、启动桌面程序，并使用安装目录中的数据库、BGE-M3、Qwen3-4B 和 llama.cpp 执行真实检索与问答，然后卸载。报告位于 `dist/installer/installer-smoke-test.json`，可审计副本保存在 `release/evidence/<version>/`。以下流程仍需在干净 Windows VM 以及 CUDA、Vulkan-only、CPU-only 环境执行，不应把构建机测试表述为新电脑验收已经通过。

1. 在未安装 Rust、Node.js、Python、MSYS2 的 Windows 10 1809+ 或 Windows 11 x64 机器上安装。
2. 断开网络并启动 ILIA，确认界面正常显示。
3. 分别执行一次检索和本地问答，确认数据库、BGE-M3、Qwen3-4B 均从安装目录加载。
4. 在 NVIDIA、Vulkan-only 和无独立显卡机器上分别确认 CUDA → Vulkan → CPU 自动降级。
5. 卸载 ILIA，确认程序目录被移除；用户日志保留在本地应用数据目录，便于故障分析。
