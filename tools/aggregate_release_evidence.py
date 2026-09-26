#!/usr/bin/env python3
"""Aggregate versioned release evidence without converting missing evidence into a pass."""
from __future__ import annotations

import argparse
import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path


EXPECTED = (
    "validation_report_{version}.json",
    "retrieval_eval_report_{version}.json",
    "citation_eval_report_{version}.json",
    "privacy_offline_report.json",
    "runtime-offline-network.json",
    "installer-verification.json",
    "windows-installer-smoke.json",
    "release-matrix.json",
    "update-rollback-report.json",
    "SHA256SUMS.txt",
)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--allow-incomplete", action="store_true")
    args = parser.parse_args()
    assets = []
    missing = []
    for template in EXPECTED:
        name = template.format(version=args.version)
        path = args.evidence_dir / name
        if not path.is_file():
            missing.append(name)
            continue
        data = path.read_bytes()
        assets.append({"name": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
    report = {
        "report_version": 1,
        "release_version": args.version,
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "status": "complete" if not missing else "incomplete",
        "assets": assets,
        "missing": missing,
    }
    output = args.evidence_dir / "release-evidence-summary.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return 0 if not missing or args.allow_incomplete else 1


if __name__ == "__main__":
    raise SystemExit(main())
