# Corpus and evaluation tools

- `corpus-importer/`：从 manifest 和官方原件重建 SQLite 资料库。
- `corpus-validator/`：校验哈希、结构连续性、页码、FTS5 与 SQLite 完整性；必须用 `--version <version>` 生成 `validation_report_<version>.json`，禁止复用无版本报告。
- `corpus-embedder/`：用本地 BGE-M3 为全部内容单元生成向量并写入 SQLite。
- `eval-runner/`：运行 JSONL 检索用例并生成 Recall@1/5/10、MRR、精确定位、双语文书名和过滤逃逸统计；原始基线为 200 项，规范化 49 文书发布集为 197 项，1.1 开发集为 305 项。
- `build_retrieval_eval_1_1.py`：从 197 题基线、核心数据库和版本化主题元数据确定性生成 305 题 1.1 评测集；`--check` 只校验已提交文件及主题文书 ID，供 CI 使用。
- `tests/eval/citation_support_1.1.jsonl`：人工维护的中英文引证支持对照集，`direct`、`summary`、`unsupported`、`conflict` 各 3 项；`tools/tests/test_citation_support_eval.py` 固定标签平衡与格式。
- `prepare-distributable-corpus.py`：从冻结的 50 文书基线生成 49 份规范化正文、发布数据库和排除 ICRC 习惯法资料后的 197 题评测集。
- `generate-dependency-notices.py`：根据锁文件生成 Rust/npm 完整依赖清单，并汇总本地包中的许可与 NOTICE 文件。
- `generate-corpus-rights-matrix.py`：根据冻结的 50 份资料清单生成逐项正文/PDF 再分发风险矩阵。
- `release_rights_gate.py`：CI 与安装器共用的发布许可门禁；阻止未获准的 `red`/`pending` 资料或组件、未满足处理条件的 `yellow` 资料以及来源 PDF 进入发布阶段。
- `sync-third-party-licenses.ps1`：下载并校验固定版本的模型与运行时许可原文。
- `privacy_offline_audit.py`：对导入、工作区、检索和嵌入代码的网络/敏感日志边界生成带源码哈希的报告。
- `update_test_report.py`：实际执行 `ilia-updater` 测试并保存命令、主机、退出码与输出尾部。
- `aggregate_release_evidence.py`：汇总候选版本的数据库、评测、隐私、安装器、回滚和哈希证据；默认对任何缺项返回失败，`--allow-incomplete` 只用于开发期盘点。
