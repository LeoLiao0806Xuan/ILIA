# Rust crates

- `ilia-core`：文书摘要、检索命中、匹配类型与响应契约。
- `ilia-database`：只读 SQLite 连接、schema 检查、文书解析、条款/段落精确查询及 FTS5。
- `ilia-embedding`：BGE-M3 INT8 ONNX 本地推理，输出 1024 维 dense 向量。
- `ilia-retrieval`：中英文查询解析、精确/FTS5/向量召回、RRF 融合、证据选择和 `ilia-search` CLI。
- `ilia-inference`：llama.cpp 运行时探测、进程管理、证据约束问答与引证校验。
- `ilia-updater`：Ed25519 签名清单、SHA-256 负载校验、应用/资料/模型/运行时更新及失败回滚。
