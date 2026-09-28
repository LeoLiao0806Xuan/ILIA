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
    "update-progress-e2e.json",
    "SHA256SUMS.txt",
)


def read_json(path: Path) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"invalid JSON in {path.name}: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"expected an object in {path.name}")
    return value


def validate_report(name: str, path: Path, version: str) -> list[str]:
    if name == "SHA256SUMS.txt":
        lines = [line for line in path.read_text(encoding="utf-8-sig").splitlines() if line.strip()]
        return [] if len(lines) >= 4 else ["SHA256SUMS.txt must contain all four installer files"]
    report = read_json(path)
    errors = []
    reported_version = report.get("release_version", report.get("version"))
    if reported_version is not None and str(reported_version) != version:
        errors.append(f"{name} reports version {reported_version}, expected {version}")
    if name.startswith("validation_report_") and report.get("status") != "passed":
        errors.append(f"{name} validation status is not passed")
    elif name.startswith("retrieval_eval_report_"):
        if report.get("failed") != 0 or report.get("passed") != report.get("total"):
            errors.append(f"{name} does not pass every retrieval case")
        if float(report.get("recall_at_10", 0)) < 1.0:
            errors.append(f"{name} Recall@10 is below 1.0")
    elif name.startswith("citation_eval_report_"):
        if report.get("passed") is not True:
            errors.append(f"{name} citation gate is not passed")
        if int(report.get("unsafe_false_accepts", 1)) != 0:
            errors.append(f"{name} contains unsafe false accepts")
        if float(report.get("safety_boundary_accuracy", 0)) < 1.0:
            errors.append(f"{name} safety boundary accuracy is below 1.0")
        if float(report.get("accuracy", 0)) < 0.85:
            errors.append(f"{name} overall accuracy is below 0.85")
    elif name == "release-matrix.json" and report.get("passed") is not True:
        errors.append("release-matrix.json is not passed")
    elif name == "update-progress-e2e.json":
        required = ["checking", "ready_to_apply", "applying", "applied"]
        phases = report.get("phases", [])
        if report.get("status") != "passed" or report.get("phase") != "applied":
            errors.append("update-progress-e2e.json did not pass or reach applied")
        cursor = 0
        for phase in phases:
            if cursor < len(required) and phase == required[cursor]:
                cursor += 1
        if cursor != len(required):
            errors.append("update-progress-e2e.json lacks the ordered lifecycle phases")
    elif name in {
        "privacy_offline_report.json",
        "runtime-offline-network.json",
        "installer-verification.json",
        "windows-installer-smoke.json",
        "update-rollback-report.json",
    } and report.get("status") != "passed":
        errors.append(f"{name} status is not passed")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--allow-incomplete", action="store_true")
    args = parser.parse_args()
    assets = []
    missing = []
    invalid = []
    for template in EXPECTED:
        name = template.format(version=args.version)
        path = args.evidence_dir / name
        if not path.is_file():
            missing.append(name)
            continue
        data = path.read_bytes()
        assets.append({"name": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
        try:
            invalid.extend(validate_report(name, path, args.version))
        except ValueError as error:
            invalid.append(str(error))
    report = {
        "report_version": 1,
        "release_version": args.version,
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "status": "passed" if not missing and not invalid else "failed",
        "assets": assets,
        "missing": missing,
        "invalid": invalid,
    }
    output = args.evidence_dir / "release-evidence-summary.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return 0 if (not missing and not invalid) or args.allow_incomplete else 1


if __name__ == "__main__":
    raise SystemExit(main())
