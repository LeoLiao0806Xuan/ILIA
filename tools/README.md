# Corpus and evaluation tools

- `corpus-importer/`：从 manifest 和官方原件重建 SQLite 资料库。
- `corpus-validator/`：校验哈希、结构连续性、页码、FTS5 与 SQLite 完整性。
- `corpus-embedder/`：用本地 BGE-M3 为全部内容单元生成向量并写入 SQLite。
- `eval-runner/`：运行 JSONL 检索用例并生成 Recall、MRR 和标签统计；当前基线为 100 项。
