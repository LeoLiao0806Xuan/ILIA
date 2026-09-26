#!/usr/bin/env python3
"""Run updater tests and persist the exact machine-readable result."""
from __future__ import annotations

import argparse
import json
import platform
import subprocess
from datetime import datetime, timezone
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    command = ["cargo", "+stable-x86_64-pc-windows-gnu", "test", "-p", "ilia-updater"]
    completed = subprocess.run(command, text=True, capture_output=True, check=False)
    report = {
        "report_version": 1,
        "release_version": args.version,
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "host": {"platform": platform.platform(), "python": platform.python_version()},
        "command": command,
        "exit_code": completed.returncode,
        "status": "passed" if completed.returncode == 0 else "failed",
        "covered": ["signature", "hash", "path traversal", "transactional rollback", "local .ilia package", "partial resume", "SQLite integrity"],
        "stdout_tail": completed.stdout[-8000:],
        "stderr_tail": completed.stderr[-8000:],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
