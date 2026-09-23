# ILIA 仓库结构

本结构于 2026-09-21 确认。目录名统一使用小写英文和连字符；Rust crate 使用 `ilia-*` 命名。

```text
ILIA/
├─ apps/
│  └─ desktop/              # 已实现：Tauri 2 桌面 MVP
├─ crates/
│  ├─ ilia-core/            # 已实现：检索领域类型和响应契约
│  ├─ ilia-database/        # 已实现：只读 SQLite、FTS5、精确定位
│  ├─ ilia-embedding/       # 已实现：BGE-M3 本地 ONNX 嵌入
│  ├─ ilia-retrieval/       # 已实现：查询解析与检索服务
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
│  └─ sources/              # 不可变原始资料及来源审计附件
├─ data/                    # 可重建数据库和验证/评测报告
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
manifest + official PDFs + SQL schema
                 ↓
          corpus-importer
                 ↓
 SQLite + FTS5 + BGE-M3 (5,254 units)
          ↙              ↘
 corpus-validator    Rust retrieval API
                            ↓
                 RRF + evidence selection
                     ↙             ↘
                eval-runner    Qwen3-4B / llama.cpp
                                     ↓
                         grounded answer + citations
                                     ↓
                        Tauri evidence workspace
                                     ↓
                    signed updater + atomic rollback
```

`data/` 与 `corpus/sources/` 分离：前者是可重建产物，后者是需要长期保留哈希与来源链的输入证据。
