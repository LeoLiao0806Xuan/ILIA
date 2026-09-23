# ILIA 1.0.0-rc.1 发布说明

`1.0.0-rc.1` 是 ILIA 的首个 1.0 发布候选版，用于最终许可审查、干净环境验收和硬件兼容性验证；它不是面向公众的正式 `1.0.0`。

## 候选版能力

- 50 份国际法资料、5,254 个可引用内容单元及对应 BGE-M3 向量。
- FTS5、精确定位和向量召回组成的混合检索。
- Qwen3-4B 与 llama.cpp 离线问答，回答包含可回到原文的引证。
- CUDA、Vulkan、CPU 自动探测与降级。
- Tauri 桌面界面、原文证据、来源链接及签名更新和失败回滚基础设施。
- Windows x64 离线安装介质。

## 重要限制

- 50 份资料及随包模型、运行时的再分发许可仍在审查。候选介质已包含 `THIRD_PARTY_NOTICES.md` 草案和许可证目录；所有 pending 项解决、必需的权利人声明和完整许可文本补齐前，不得将候选安装介质作为公开正式版发布。
- `treaty_parties`、`treaty_statements`、`protocol_relations` 尚无动态数据；缔约状态、批准日期、保留、声明和议定书关系能力计划在 1.1.0 或以后提供。
- 安装器及桌面 EXE 尚未进行 Authenticode 代码签名，Windows 可能显示未知发布者或 SmartScreen 提示。
- 构建机验收不能代替干净 Windows VM 以及 CUDA、Vulkan-only、CPU-only 设备的最终发布验收。

## 安全下载与校验

候选版只能从项目正式 GitHub Release 下载。将 `setup.exe` 与所有 `.bin` 数据片放在同一目录，并使用发布资产中的 `SHA256SUMS.txt` 核对每个文件：

```powershell
Get-FileHash -Algorithm SHA256 .\ILIA-1.0.0-rc.1-windows-x64-offline-setup.exe
Get-FileHash -Algorithm SHA256 .\ILIA-1.0.0-rc.1-windows-x64-offline-setup-*.bin
```

只有计算结果与清单完全一致时才应安装。

## 法律免责声明

ILIA 提供国际法资料检索与辅助解释，不构成法律意见，不替代执业律师或相关主管机构的专业判断。条约状态、保留、声明及最新法律发展应以官方来源为准。

## 晋升正式版的条件

完成第三方许可审查与声明、最终制品验收、GitHub Actions 绿色检查及发布审批后，才能将所有版本字段从 `1.0.0-rc.1` 晋升为 `1.0.0`。
