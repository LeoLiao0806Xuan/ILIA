# ILIA 更新系统

`ilia-updater` 将应用、资料库、模型和推理运行时统一为一份签名发布清单，但每个组件独立标识、独立版本、独立负载和独立回滚。

## 1.1 本地包与受限网络

`ilia-updater pack --manifest <update-manifest.json> --signature <update-manifest.sig> --output <release.ilia>` 生成版本化本地容器。安装使用 `apply-package --root <安装目录> --package <release.ilia> --public-key <trusted-key.json>`。解析器限制展开大小并拒绝绝对路径、`..`、重复项和符号链接；验证通过后复用在线更新的哈希、SQLite 完整性、事务应用与回滚路径。

在线更新把 DNS、连接超时、服务不可达、下载中断、签名失败和负载失败映射为稳定诊断代码。更新代理位于应用数据目录 `update-proxy.json`，支持 HTTP、HTTPS 和 SOCKS5；凭据不会出现在 UI 或 Debug 输出中，研究命令不读取该文件。

下载使用 `.part` 与 `.part.json` 保存匹配组件 ID、总大小、SHA-256 和偏移。远端接受 Range 时续传；返回完整响应时截断重下；最终哈希失败立即清理部分文件。

## 安全边界

- 发布清单使用 Ed25519 分离签名；桌面端和更新助手均使用安装包内置公钥复核。
- 每个负载必须匹配清单中的字节数与 SHA-256；任何一个组件不匹配都会停止更新。
- 目标只能是安装根目录内的相对路径，拒绝绝对路径和 `..` 路径穿越。
- 更新前逐级检查目标的现有父路径；Windows Junction、重解析点和符号链接会被拒绝，避免词法上位于安装目录内的路径被重定向到目录外。
- 更新清单及负载只接受 HTTPS；`file://` 仅用于本地测试与离线验收。
- 私钥位于 `.secrets/update-private-key.json`，已被 Git 忽略，不得上传、放入安装包或发送给客户端。

## 四类更新

### 应用

桌面端先校验签名，再启动独立的 `ilia-updater.exe` 并退出。更新助手等待桌面进程结束后替换 `ilia-desktop.exe`，避免 Windows 文件锁。WebView loader 等其他应用文件可作为同一发布中的独立 `raw_file` 组件。

更新负载以流式方式下载、计算 SHA-256 并写入暂存文件，可处理数 GB 的模型文件而不把整包读入内存。更新助手自身不能在运行中替换；`ilia-updater.exe` 的版本升级随完整安装包发布。

### 资料库增量

资料更新使用 `sqlite_patch` JSON：

```json
{
  "schema_version": 1,
  "statements": [
    "INSERT INTO ...;",
    "UPDATE ...;"
  ]
}
```

更新助手先复制数据库备份，再在单个 SQLite 事务中执行语句并运行 `PRAGMA integrity_check`。任何语句或完整性检查失败都会回滚事务并恢复备份。

### 模型与运行时

模型权重、tokenizer、ONNX Runtime 以及 CUDA/Vulkan/CPU 文件分别作为签名清单中的 `model` 或 `runtime`、`raw_file` 组件发布。一次发布中的多个文件作为同一事务顺序应用；中途失败时按相反顺序恢复。

### 回滚

更新前版本保存在 `.ilia-update/backups/<release_id>/`，进度写入 `.ilia-update/journal.json`。自动失败回滚之外，也可执行：

```powershell
ilia-updater rollback --root <安装目录> --manifest-url <清单URL> --signature-url <签名URL> --public-key <公钥文件>
```

## 制作发布

`update/components.example.json` 展示组件清单。准备负载后运行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File update/build-update-release.ps1 `
  -ReleaseId v1.0.0 `
  -BaseUrl https://github.com/LeoLiao0806Xuan/ILIA/releases/download `
  -ComponentsFile update/components.v1.0.0.json
```

脚本计算负载大小和 SHA-256、生成 `update-manifest.json`，并用本地私钥产生 `update-manifest.sig`。将清单、签名和所有 `.payload` 文件作为同一个 GitHub Release 的资产上传。

桌面端默认可通过 `check_updates` 与 `install_update` 命令连接 Release 资产。首次发布前必须把 `.secrets/update-private-key.json` 备份到加密、离线位置；丢失私钥后现有客户端无法信任新密钥签发的更新。
