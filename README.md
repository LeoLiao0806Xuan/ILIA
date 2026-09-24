# ILIA — 本地国际法智能助手

ILIA（International Law Intelligence Assistant）是一款面向国际法学习、研究与实务检索的 Windows 桌面应用。它把法律资料检索、英文原文翻译、有据问答和原始文献阅览放在同一个工作台中，让每项结论都能回到具体条款、判例段落和官方来源。

当前版本：`1.0.0`

## 为什么使用 ILIA

- **从原文出发**：同时使用文书名、条款号、全文关键词和语义检索定位资料。
- **回答可以核验**：本地模型生成的每项实质结论均链接至入选证据，点击 `【n】` 即可查看原文。
- **英文文献中文阅读**：对英文条款和判例段落进行本地简体中文翻译，原文与译文并列展示，不覆盖原始资料。
- **适配不同电脑**：自动探测 NVIDIA CUDA、Vulkan 和 CPU，并在不可用时依次降级。

## 已收录资料

首版资料库包含 50 份国际法核心文献、5,254 个可引用内容单元，并随安装包提供对应的原始 PDF；可从应用的“资料库”按标题浏览并打开阅读。资料覆盖：

- 《联合国宪章》《国际法院规约》《维也纳条约法公约》等基础文件；
- 核心国际人权公约；
- 日内瓦公约及国际人道法资料；
- 具有代表性的国际法院判决与咨询意见。

检索评测集包含 200 个问题，覆盖全部 50 份资料；当前基线测试为 200/200，Recall@5 与 Recall@10 均为 1.0。

## 核心功能

### 混合检索

FTS5 全文检索与 BGE-M3 语义向量检索协同工作，并支持中英文文书名称、条款号及 ICJ 段落号的精确定位。

### 本地有据问答

Qwen3-4B 通过 llama.cpp 在本机运行，只依据检索到的资料组织答案。系统会检查 `【n】` 引证；未通过完整引证校验的内容会显示醒目提示。

### 英文原文翻译

在右侧选择英文证据后，点击“翻译为中文”即可生成简体中文译文。翻译保留标题、条款号、段落号、专有名称、数字、日期和原有分段，并始终与英文原文并列展示。译文属于本地机器翻译，正式引用时应以原文为准。

### 原文与官方来源

每条证据展示规范引用、页码、法律性质、英文原文及官方来源链接；“资料库”则提供完整原始 PDF 的浏览入口，便于连续阅读、核验和引用。

## 安装与运行

Windows x64 离线安装介质位于 `dist/installer/`。安装时必须把以下四个文件放在同一目录：

```text
ILIA-1.0.0-windows-x64-offline-setup.exe
ILIA-1.0.0-windows-x64-offline-setup-1.bin
ILIA-1.0.0-windows-x64-offline-setup-2.bin
ILIA-1.0.0-windows-x64-offline-setup-3.bin
```

双击 `...setup.exe` 并按提示安装。离线介质已经包含桌面程序、资料库、Qwen3-4B、BGE-M3、CUDA/Vulkan/CPU 运行时、ONNX Runtime 及 WebView2 离线运行时，无需安装 Rust、Node.js 或 Python。

下载后可使用同目录的 `SHA256SUMS.txt` 核验文件完整性。当前候选安装器尚未进行 Windows Authenticode 代码签名，Windows 可能显示“未知发布者”。

## 使用边界

- 当前版本不包含实时的缔约国、批准日期、保留、声明和退出状态；相关信息请以联合国等主管机构的最新官方记录为准。
- ILIA 提供资料检索、机器翻译与辅助解释，不构成法律意见，也不替代执业律师或相关主管机构的专业判断。

## 技术组成

ILIA 使用 Tauri 2、Rust、SQLite/FTS5、BGE-M3、Qwen3-4B、llama.cpp 和 ONNX Runtime 构建。应用、资料库、模型与运行时可分别进行签名更新，并具有负载哈希校验、备份和失败回滚能力。

项目结构、开发与验证说明见 [`docs/repository_structure.md`](docs/repository_structure.md)，版本变更见 [`docs/release_notes_1.0.0.md`](docs/release_notes_1.0.0.md)。第三方组件及资料许可状态见 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。

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
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 clippy --workspace --all-targets -- -D warnings
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 test --workspace
cd apps/desktop
npm run build
```

## 许可证

项目源代码采用 Apache-2.0 许可证。随包资料、模型和第三方运行组件可能适用各自的使用与再分发条件，请在分发构建产物前核对第三方声明和对应许可。
