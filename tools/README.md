# Corpus and evaluation tools

- `corpus-importer/`：从 manifest 和官方原件重建 SQLite 资料库。
- `corpus-validator/`：校验哈希、结构连续性、页码、FTS5 与 SQLite 完整性。
- `corpus-embedder/`：用本地 BGE-M3 为全部内容单元生成向量并写入 SQLite。
- `eval-runner/`：运行 JSONL 检索用例并生成 Recall、MRR 和标签统计；原始基线为 200 项，规范化 49 文书发布集为 197 项。
- `prepare-distributable-corpus.py`：从冻结的 50 文书基线生成 49 份规范化正文、发布数据库和排除 ICRC 习惯法资料后的 197 题评测集。
- `generate-dependency-notices.py`：根据锁文件生成 Rust/npm 完整依赖清单，并汇总本地包中的许可与 NOTICE 文件。
- `generate-corpus-rights-matrix.py`：根据冻结的 50 份资料清单生成逐项正文/PDF 再分发风险矩阵。
- `sync-third-party-licenses.ps1`：下载并校验固定版本的模型与运行时许可原文。
