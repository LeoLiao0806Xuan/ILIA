#!/usr/bin/env python3
"""Validate updater event telemetry and produce release-gate evidence."""
from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path


REQUIRED_PHASES = ("checking", "ready_to_apply", "applying", "applied")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--events", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    events = []
    for line_number, line in enumerate(args.events.read_text(encoding="utf-8-sig").splitlines(), 1):
        if not line.strip():
            continue
        value = json.loads(line)
        if not isinstance(value, dict) or not isinstance(value.get("phase"), str):
            raise ValueError(f"invalid updater event at line {line_number}")
        events.append(value)
    phases = [event["phase"] for event in events]
    cursor = 0
    for phase in phases:
        if cursor < len(REQUIRED_PHASES) and phase == REQUIRED_PHASES[cursor]:
            cursor += 1
    errors = []
    if cursor != len(REQUIRED_PHASES):
        errors.append(f"required ordered phases missing: {', '.join(REQUIRED_PHASES)}")
    if "failed" in phases:
        errors.append("updater emitted failed phase")
    release_ids = {event.get("release_id") for event in events if event.get("release_id")}
    expected_release = f"v{args.version}"
    if release_ids != {expected_release}:
        errors.append(f"release ids {sorted(release_ids)} do not equal {expected_release}")
    report = {
        "report_version": 1,
        "release_version": args.version,
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "status": "passed" if not errors else "failed",
        "phase": phases[-1] if phases else None,
        "phases": phases,
        "event_count": len(events),
        "errors": errors,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return 0 if not errors else 1


if __name__ == "__main__":
    raise SystemExit(main())
