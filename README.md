# ILIA — 本地国际法智能助手

ILIA（International Law Intelligence Assistant）是一款完全离线运行的 Windows 国际法检索与辅助问答应用。它把法律资料检索、中文问答、原文翻译和文献阅读放在同一个工作台中，让结论回到具体条款、判例段落和来源。

当前版本：`1.0.1`

## 核心能力

- **完全离线**：问题、资料和模型都保存在本机，不调用收费 API。
- **混合检索**：结合条款/段落精确定位、SQLite FTS5 全文检索和 BGE-M3 语义检索。
- **有据问答**：Qwen3-4B 只依据本地证据回答，并使用 `【1】`、`【2】` 标注文献依据。
- **中英文支持**：可用中文检索英文法律资料，并在本地生成简体中文译文。
- **文献核验**：展示文书、条款、段落、来源版页码、原文和官方来源链接。
- **硬件自适应**：自动按 CUDA → Vulkan → CPU 降级，支持 8GB 显存设备。
- **离线安装**：模型、资料库和运行时随安装介质提供，安装后即可查询。

## 已收录资料

1.0.1 内置 49 份国际法核心文献和 5,093 个可引用内容单元，包括：

- 《联合国宪章》《国际法院规约》《维也纳条约法公约》等基础文件；
- 核心国际人权公约；
- 日内瓦四公约及附加议定书；
- 具有代表性的国际法院判决与咨询意见。

197 题检索评测全部通过，Recall@5 与 Recall@10 均为 1.0。

## 安装与运行

Windows x64 离线安装介质位于 `dist/installer/`。安装时必须把以下四个文件放在同一目录：

```text
ILIA-1.0.1-windows-x64-offline-setup.exe
ILIA-1.0.1-windows-x64-offline-setup-1.bin
ILIA-1.0.1-windows-x64-offline-setup-2.bin
ILIA-1.0.1-windows-x64-offline-setup-3.bin
```

双击 `...setup.exe` 并按提示安装。离线介质已经包含桌面程序、资料库、Qwen3-4B、BGE-M3、CUDA/Vulkan/CPU 运行时、ONNX Runtime 及 WebView2 离线运行时，无需安装 Rust、Node.js 或 Python。

下载后可使用同目录的 `SHA256SUMS.txt` 核验文件完整性。1.0.1 安装器尚未进行 Windows Authenticode 代码签名，Windows 可能显示“未知发布者”。

## 技术组成

ILIA 使用 Tauri 2、Rust、SQLite/FTS5、BGE-M3、Qwen3-4B、llama.cpp 和 ONNX Runtime 构建。应用、资料库、模型与运行时可分别进行签名更新，并具有负载哈希校验、备份和失败回滚能力。

项目结构、开发与验证说明见 [`docs/repository_structure.md`](docs/repository_structure.md)，版本变更见 [`docs/release_notes_1.0.1.md`](docs/release_notes_1.0.1.md)。第三方组件及资料许可状态见 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。

## 从源码运行

环境准备完成后：

```powershell
cd apps/desktop
npm install
npm run tauri -- dev
```

运行全部 Rust 检查与前端生产构建：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 fmt --all -- --check
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 clippy --workspace --all-targets --- -D warnings
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 test --workspace
cd apps/desktop
npm run build
```

## 许可证与说明

源代码采用 Apache-2.0。第三方组件与语料说明见 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)；ILIA 提供研究辅助，不构成法律意见。
