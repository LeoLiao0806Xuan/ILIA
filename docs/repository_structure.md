# ILIA 仓库结构

本结构于 2026-09-21 确认。目录名统一使用小写英文和连字符；Rust crate 使用 `ilia-*` 命名。

```text
ILIA/
├─ apps/
│  └─ desktop/              # 已实现：Tauri 2 桌面 MVP
├─ crates/
│  ├─ ilia-core/            # 已实现：检索领域类型和响应契约
│  ├─ ilia-database/        # 已实现：核心库只读、个人库读取、SQL 过滤
│  ├─ ilia-embedding/       # 已实现：BGE-M3 本地 ONNX 嵌入
│  ├─ ilia-retrieval/       # 已实现：核心/个人联合检索、融合与证据选择
│  ├─ ilia-inference/       # 已实现：llama.cpp 进程管理与有引证问答
│  └─ ilia-updater/         # 已实现：签名更新、事务应用与失败回滚
├─ tools/
│  ├─ corpus-importer/      # 已实现：PDF 入库
│  ├─ corpus-validator/     # 已实现：资料与数据库校验
│  ├─ corpus-embedder/      # 已实现：BGE-M3 批量向量入库
│  └─ eval-runner/          # 已实现：JSONL 检索评测
├─ corpus/
│  ├─ manifests/            # 范围清单、来源、哈希与解析配置
│  ├─ schemas/              # SQLite schema/migration
│  ├─ normalized/           # 下一版随包的 49 份 ILIA 规范化正文及清单
│  └─ sources/              # 不可变原始资料及来源审计附件
├─ data/                    # 可重建核心数据库和版本化验证/评测报告
├─ runtime/
│  ├─ cpu/                  # 已安装：llama.cpp CPU
│  ├─ vulkan/               # 已安装：llama.cpp Vulkan
│  └─ cuda/                 # 已安装：llama.cpp CUDA 13.3
├─ models/                  # BGE-M3 ONNX 与 Qwen3-4B GGUF
├─ tests/                   # Rust 集成测试与 JSONL 评测集
├─ update/                  # 公钥、组件样例与签名发布构建脚本
├─ installer/               # Windows 离线安装器与验证脚本
├─ docs/                    # 范围、架构与开发文档
├─ Cargo.toml               # Rust workspace
└─ README.md
```

## 当前数据流

```text
manifest + normalized text + SQL schema
                 ↓
          corpus-importer
                 ↓
 core SQLite + FTS5 + BGE-M3 (5,093 units)
          ↙              ↘
 corpus-validator    Rust retrieval API
                            ↓
        core/user federation + SQL filters
                            ↓
            RRF + quotas + evidence selection
                     ↙             ↘
                eval-runner    Qwen3-4B / llama.cpp
                                     ↓
                         grounded answer + citations
                                     ↓
                        Tauri evidence workspace
                                     ↓
                    signed updater + atomic rollback
```

`data/`、`corpus/normalized/` 与 `corpus/sources/` 分离：数据库和规范化正文是可重建发布产物，`sources/` 是需要长期保留哈希与来源链、但不进入下一版安装包的输入证据。

从 1.1 开发基线开始，桌面端把随包核心库以只读方式打开，并在 Tauri 应用数据目录幂等创建 `user.sqlite` 与 `workspace.sqlite`。两份可写数据库具有独立的 `schema_migrations`；核心资料更新不得覆盖它们。源码树暂时继续使用 `data/ilia.sqlite3` 作为核心库构建产物，安装资源发现逻辑同时支持后续的 `data/core.sqlite`。
