#!/usr/bin/env python3
"""Validate the distributable ILIA normalized corpus and database."""

from __future__ import annotations

import hashlib
import json
import os
import sqlite3
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MANIFEST_PATH = ROOT / "corpus" / "manifests" / "prototype_manifest.json"
NORMALIZED_MANIFEST_PATH = ROOT / "corpus" / "normalized" / "manifest.json"
DB_PATH = Path(os.environ.get("ILIA_DB_PATH", ROOT / "data" / "ilia.sqlite3"))
REPORT_PATH = Path(os.environ.get("ILIA_VALIDATION_REPORT", ROOT / "data" / "validation_report.json"))


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def main() -> None:
    manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    normalized_manifest = json.loads(NORMALIZED_MANIFEST_PATH.read_text(encoding="utf-8"))
    excluded = set(normalized_manifest["excluded_documents"])
    normalized_artifacts = {
        artifact["document_id"]: artifact for artifact in normalized_manifest["artifacts"]
    }
    checks: list[dict] = []

    def record(name: str, passed: bool, detail: str, severity: str = "error") -> None:
        checks.append({"name": name, "passed": passed, "severity": severity, "detail": detail})

    connection = sqlite3.connect(DB_PATH)
    connection.row_factory = sqlite3.Row

    record(
        "normalized:document_count",
        normalized_manifest["document_count"] == 49 and len(normalized_artifacts) == 49,
        f"declared={normalized_manifest['document_count']} artifacts={len(normalized_artifacts)}",
    )
    record(
        "normalized:excluded_documents",
        excluded == {"icrc-cihl-rules"},
        f"excluded={sorted(excluded)}",
    )
    embedded_texts = connection.execute(
        "SELECT count(*) FROM normalized_document_texts"
    ).fetchone()[0]
    record(
        "normalized:embedded_texts",
        embedded_texts == 49,
        f"embedded_texts={embedded_texts}",
    )

    for document in manifest["documents"]:
        document_id = document["id"]
        if document_id in excluded:
            db_doc = connection.execute(
                "SELECT 1 FROM documents WHERE id = ?", (document_id,)
            ).fetchone()
            record(f"{document_id}:excluded", db_doc is None, "excluded from distributable database")
            continue
        artifact = normalized_artifacts[document_id]
        source_path = ROOT / artifact["path"]
        actual_hash = sha256_file(source_path)
        record(
            f"{document_id}:sha256",
            actual_hash == artifact["sha256"],
            f"expected={artifact['sha256']} actual={actual_hash}",
        )
        db_doc = connection.execute(
            "SELECT document_type, legal_status FROM documents WHERE id = ?", (document_id,)
        ).fetchone()
        record(f"{document_id}:document_row", db_doc is not None, "document metadata exists")
        if db_doc is not None:
            record(
                f"{document_id}:legal_type",
                db_doc["document_type"] == document["document_type"],
                f"document_type={db_doc['document_type']} legal_status={db_doc['legal_status']}",
            )

        if document["parser"] in {"articles", "rules"}:
            numbers = [
                int(row[0])
                for row in connection.execute(
                    """SELECT p.article_number FROM provisions p
                    JOIN document_versions v ON v.id = p.document_version_id
                    WHERE v.document_id = ? ORDER BY CAST(p.article_number AS INTEGER)""",
                    (document_id,),
                )
            ]
        elif document["parser"] in {"case_paragraphs", "case_auto"}:
            numbers = [
                int(row[0])
                for row in connection.execute(
                    "SELECT paragraph_number FROM case_paragraphs WHERE case_id = ? ORDER BY paragraph_number",
                    (document_id,),
                )
            ]
            if not numbers and document["parser"] == "case_auto":
                numbers = [
                    int(row[0])
                    for row in connection.execute(
                        "SELECT passage_number FROM case_passages WHERE case_id = ? ORDER BY passage_number",
                        (document_id,),
                    )
                ]
        else:
            numbers = [
                int(row[0])
                for row in connection.execute(
                    "SELECT passage_number FROM case_passages WHERE case_id = ? ORDER BY passage_number",
                    (document_id,),
                )
            ]
        expected_count = document.get("expected_units")
        record(
            f"{document_id}:unit_count_frozen",
            expected_count is not None,
            f"expected_units={expected_count}",
        )
        expected_count = expected_count or len(numbers)
        expected = list(range(1, int(expected_count) + 1))
        record(
            f"{document_id}:unit_sequence",
            numbers == expected,
            f"expected=1..{expected_count} actual_count={len(numbers)} missing={sorted(set(expected) - set(numbers))}",
        )

    empty_chunks = connection.execute(
        "SELECT COUNT(*) FROM chunks WHERE trim(text_original) = '' OR trim(text_normalized) = ''"
    ).fetchone()[0]
    record("chunks:nonempty", empty_chunks == 0, f"empty_chunks={empty_chunks}")

    invalid_pages = connection.execute(
        "SELECT COUNT(*) FROM chunks WHERE page_start < 1 OR page_end < page_start"
    ).fetchone()[0]
    record("chunks:page_ranges", invalid_pages == 0, f"invalid_page_ranges={invalid_pages}")

    duplicate_citations = connection.execute(
        """SELECT COUNT(*) FROM (
        SELECT document_version_id, citation_label, COUNT(*) AS n
        FROM chunks GROUP BY document_version_id, citation_label HAVING n > 1)"""
    ).fetchone()[0]
    record("chunks:unique_citations", duplicate_citations == 0, f"duplicates={duplicate_citations}")

    embedding_model = connection.execute(
        "SELECT dimension FROM embedding_models WHERE id = ?",
        ("gpahal/bge-m3-onnx-int8",),
    ).fetchone()
    if embedding_model is not None:
        dimension = int(embedding_model["dimension"])
        embedded_count = connection.execute(
            "SELECT count(*) FROM chunk_embeddings WHERE model_id = ?",
            ("gpahal/bge-m3-onnx-int8",),
        ).fetchone()[0]
        chunk_count = connection.execute("SELECT count(*) FROM chunks").fetchone()[0]
        malformed = connection.execute(
            "SELECT count(*) FROM chunk_embeddings WHERE model_id = ? AND length(vector) != ?",
            ("gpahal/bge-m3-onnx-int8", dimension * 4),
        ).fetchone()[0]
        record(
            "embeddings:bge_m3_coverage",
            embedded_count == chunk_count,
            f"embedded={embedded_count} chunks={chunk_count} dimension={dimension}",
        )
        record(
            "embeddings:bge_m3_blob_size",
            malformed == 0,
            f"malformed_vectors={malformed} expected_bytes={dimension * 4}",
        )

    fts_expectations = [
        ("vclt-1969", '"internal law"', "VCLT, Article 27"),
        ("arsiwa-2001", '"directed or controlled"', "ARSIWA, Article 8"),
        ("icj-nicaragua-1986-merits", '"armed attack"', "Nicaragua v. United States (Merits), para. 191"),
    ]
    for document_id, query, expected_citation in fts_expectations:
        rows = connection.execute(
            """SELECT citation_label FROM chunks_fts
            WHERE chunks_fts MATCH ? AND document_id = ? LIMIT 50""",
            (query, document_id),
        ).fetchall()
        citations = [row[0] for row in rows]
        record(
            f"fts:{document_id}:{expected_citation}",
            expected_citation in citations,
            f"query={query} hits={citations[:10]}",
        )

    arsiwa_type = connection.execute(
        "SELECT document_type FROM documents WHERE id = 'arsiwa-2001'"
    ).fetchone()[0]
    record(
        "legal_classification:arsiwa_not_treaty",
        arsiwa_type == "draft_articles",
        f"document_type={arsiwa_type}",
    )

    review_pages = [
        dict(row)
        for row in connection.execute(
            """SELECT d.id AS document_id, p.pdf_page, p.extraction_method, p.review_status
            FROM source_pages p
            JOIN document_versions v ON v.id = p.document_version_id
            JOIN documents d ON d.id = v.document_id
            WHERE p.review_status NOT IN ('auto_pass', 'approved')
            ORDER BY d.id, p.pdf_page"""
        )
    ]
    record(
        "pages:review_queue_declared",
        all(row["review_status"] in {"needs_review", "needs_second_review", "unreviewed"} for row in review_pages),
        f"review_queue={review_pages}",
        severity="warning",
    )

    unfrozen_counts = connection.execute(
        "SELECT COUNT(*) FROM qa_findings WHERE check_code = 'AUTO_UNIT_COUNT_NOT_FROZEN'"
    ).fetchone()[0]
    record(
        "qa:unit_counts_frozen",
        unfrozen_counts == 0,
        f"AUTO_UNIT_COUNT_NOT_FROZEN={unfrozen_counts}",
    )

    integrity = connection.execute("PRAGMA integrity_check").fetchone()[0]
    record("sqlite:integrity", integrity == "ok", f"integrity_check={integrity}")
    connection.close()

    errors = [check for check in checks if not check["passed"] and check["severity"] == "error"]
    warnings = [
        check
        for check in checks
        if not check["passed"] and check["severity"] == "warning"
    ]
    report = {
        "validated_at": datetime.now(timezone.utc).replace(microsecond=0).isoformat(),
        "database": str(DB_PATH),
        "status": "passed" if not errors else "failed",
        "error_count": len(errors),
        "warning_count": len(warnings),
        "checks": checks,
        "manual_review_queue": review_pages,
    }
    REPORT_PATH.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    if errors:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
