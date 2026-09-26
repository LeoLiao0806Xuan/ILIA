#!/usr/bin/env python3
"""Fail a release build when packaged corpus or components violate rights policy."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import sqlite3
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = ROOT / "corpus" / "normalized" / "manifest.json"
DEFAULT_MATRIX = ROOT / "licenses" / "corpus-redistribution-rights-matrix.csv"
DEFAULT_COMPONENTS = ROOT / "licenses" / "component-clearance.json"
DEFAULT_DATABASE = ROOT / "data" / "ilia.sqlite3"

ALLOWED_COMPONENT_STATES = {
    "cleared",
    "cleared_with_provenance_notice",
    "conditional",
}
ALLOWED_YELLOW_DECISIONS = {
    "package_ilia_normalized_judicial_text_without_source_pdf",
    "package_ilia_normalized_legal_text_without_source_pdf",
    "package_ilia_normalized_treaty_text_without_source_pdf",
}


class GateError(RuntimeError):
    pass


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def validate_release(
    *,
    root: Path,
    manifest_path: Path,
    matrix_path: Path,
    components_path: Path,
    database_path: Path,
    stage_root: Path | None = None,
) -> dict[str, object]:
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    with matrix_path.open("r", encoding="utf-8-sig", newline="") as stream:
        matrix_rows = list(csv.DictReader(stream))
    components = json.loads(components_path.read_text(encoding="utf-8"))["components"]

    artifacts = {item["document_id"]: item for item in manifest["artifacts"]}
    excluded = set(manifest["excluded_documents"])
    if len(artifacts) != len(manifest["artifacts"]):
        raise GateError("normalized manifest contains duplicate document IDs")
    if manifest["document_count"] != len(artifacts):
        raise GateError("normalized manifest document_count does not match artifacts")

    rights = {row["document_id"]: row for row in matrix_rows}
    if len(rights) != len(matrix_rows):
        raise GateError("rights matrix contains duplicate document IDs")
    expected_rights = set(artifacts) | excluded
    if set(rights) != expected_rights:
        missing = sorted(expected_rights - set(rights))
        extra = sorted(set(rights) - expected_rights)
        raise GateError(f"rights matrix coverage mismatch: missing={missing} extra={extra}")

    for document_id, artifact in artifacts.items():
        row = rights[document_id]
        rating = row["normalized_text_rating"].strip().lower()
        decision = row["v1_0_1_packaging_decision"].strip()
        if rating in {"red", "pending"}:
            raise GateError(f"packaged normalized text is {rating}: {document_id}")
        if rating == "yellow" and decision not in ALLOWED_YELLOW_DECISIONS:
            raise GateError(
                f"yellow normalized text lacks an approved no-PDF treatment: {document_id}"
            )
        if rating not in {"green", "yellow"}:
            raise GateError(f"unknown normalized-text rating {rating!r}: {document_id}")

        relative = Path(artifact["path"])
        normalized_prefix = Path("corpus") / "normalized" / document_id
        if relative.suffix.lower() != ".txt" or normalized_prefix not in relative.parents:
            raise GateError(f"release artifact is not normalized text: {artifact['path']}")
        artifact_path = root / relative
        if not artifact_path.is_file():
            raise GateError(f"normalized artifact is missing: {relative}")
        if artifact_path.stat().st_size != artifact["byte_length"]:
            raise GateError(f"normalized artifact size mismatch: {relative}")
        if sha256_file(artifact_path) != artifact["sha256"]:
            raise GateError(f"normalized artifact SHA-256 mismatch: {relative}")

    for document_id in excluded:
        row = rights[document_id]
        if row["v1_0_1_packaging_decision"] != "excluded_from_release_corpus":
            raise GateError(f"excluded document lacks exclusion decision: {document_id}")

    blocked_components = []
    for component, clearance in components.items():
        state = clearance["public_distribution"].strip().lower()
        if state not in ALLOWED_COMPONENT_STATES:
            blocked_components.append(f"{component}:{state}")
        if state == "conditional" and not clearance.get("condition", "").strip():
            blocked_components.append(f"{component}:missing_condition")
    if blocked_components:
        raise GateError(
            "component rights gate blocked release: " + ", ".join(blocked_components)
        )

    connection = sqlite3.connect(f"{database_path.resolve().as_uri()}?mode=ro", uri=True)
    try:
        database_documents = {
            row[0] for row in connection.execute("SELECT id FROM documents")
        }
        excluded_rows = 0
        if excluded:
            excluded_rows = connection.execute(
                "SELECT count(*) FROM chunks WHERE document_id IN ({})".format(
                    ",".join("?" for _ in excluded)
                ),
                tuple(sorted(excluded)),
            ).fetchone()[0]
    finally:
        connection.close()
    if database_documents != set(artifacts):
        missing = sorted(set(artifacts) - database_documents)
        extra = sorted(database_documents - set(artifacts))
        raise GateError(f"release database coverage mismatch: missing={missing} extra={extra}")
    if excluded_rows:
        raise GateError("excluded documents still have chunks in the release database")

    packaged_pdfs: list[str] = []
    if stage_root is not None:
        packaged_pdfs = [
            str(path.relative_to(stage_root)).replace("\\", "/")
            for path in stage_root.rglob("*")
            if path.is_file() and path.suffix.lower() == ".pdf"
        ]
        if packaged_pdfs:
            raise GateError(f"source PDFs found in release stage: {packaged_pdfs}")

    return {
        "status": "passed",
        "packaged_normalized_documents": len(artifacts),
        "excluded_documents": sorted(excluded),
        "yellow_documents": sum(
            rights[item]["normalized_text_rating"].strip().lower() == "yellow"
            for item in artifacts
        ),
        "source_pdfs_packaged": len(packaged_pdfs),
        "rights_matrix_sha256": sha256_file(matrix_path),
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--matrix", type=Path, default=DEFAULT_MATRIX)
    parser.add_argument("--components", type=Path, default=DEFAULT_COMPONENTS)
    parser.add_argument("--database", type=Path, default=DEFAULT_DATABASE)
    parser.add_argument("--stage-root", type=Path)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    try:
        report = validate_release(
            root=args.root.resolve(),
            manifest_path=args.manifest.resolve(),
            matrix_path=args.matrix.resolve(),
            components_path=args.components.resolve(),
            database_path=args.database.resolve(),
            stage_root=args.stage_root.resolve() if args.stage_root else None,
        )
    except (GateError, FileNotFoundError, KeyError, ValueError, sqlite3.Error) as error:
        raise SystemExit(f"release rights gate failed: {error}") from error
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
