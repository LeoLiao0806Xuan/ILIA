#!/usr/bin/env python3
"""Generate a machine-readable static privacy/offline boundary report."""
from __future__ import annotations

import argparse
import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path


OFFLINE_PATHS = (
    "crates/ilia-database/src/import.rs",
    "crates/ilia-database/src/workspace.rs",
    "crates/ilia-retrieval/src/lib.rs",
    "crates/ilia-embedding/src/lib.rs",
)
NETWORK_MARKERS = ("ureq::", "reqwest::", "TcpStream", "http://", "https://")
SECRET_LOG_MARKERS = ("println!(proxy", "eprintln!(proxy", "dbg!(proxy", "println!(source_text")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    digest.update(path.read_bytes())
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    findings = []
    files = []
    for relative in OFFLINE_PATHS:
        path = args.root / relative
        text = path.read_text(encoding="utf-8")
        markers = [marker for marker in NETWORK_MARKERS if marker in text]
        secrets = [marker for marker in SECRET_LOG_MARKERS if marker in text]
        files.append({"path": relative, "sha256": sha256(path), "network_markers": markers, "secret_log_markers": secrets})
        if markers or secrets:
            findings.append({"path": relative, "network_markers": markers, "secret_log_markers": secrets})
    report = {
        "report_version": 1,
        "release_version": args.version,
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "scope": "static boundary audit for import, workspace, retrieval, and embedding paths",
        "status": "passed" if not findings else "failed",
        "files": files,
        "findings": findings,
        "limitations": ["Runtime packet interception is performed by the release-host acceptance matrix, not inferred by this static report."],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return 0 if not findings else 1


if __name__ == "__main__":
    raise SystemExit(main())
