#!/usr/bin/env python3
"""Build the 49-document normalized-text corpus and release database.

The reviewed v1.0.0 database is treated as the immutable extraction baseline.
This script does not delete source PDFs. It creates separate ILIA text artifacts,
removes the ICRC Customary IHL Rules from the release database, and rewrites
source-file metadata to point at the normalized artifacts.
"""

from __future__ import annotations

import hashlib
import json
import re
import shutil
import sqlite3
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BASELINE_DB = ROOT / "data" / "ilia_prototype.sqlite3"
RELEASE_DB = ROOT / "data" / "ilia.sqlite3"
OUTPUT_ROOT = ROOT / "corpus" / "normalized"
MANIFEST_PATH = OUTPUT_ROOT / "manifest.json"
BASELINE_EVAL = ROOT / "tests" / "eval" / "retrieval_baseline.jsonl"
RELEASE_EVAL = ROOT / "tests" / "eval" / "retrieval_normalized.jsonl"
EXCLUDED_DOCUMENTS = {"icrc-cihl-rules"}
DATABASE_VERSION = "public-international-law-v1.0.1-normalized-2026.09.24"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def strip_edition_matter(text: str) -> str:
    kept: list[str] = []
    skip_following_year = False
    for line in text.splitlines():
        stripped = line.strip()
        if re.search(r"(?i)copyright\s*(?:©|\(c\))|all rights reserved|^isbn\b", stripped):
            skip_following_year = True
            continue
        if skip_following_year and re.fullmatch(r"(?:19|20)\d{2}", stripped):
            skip_following_year = False
            continue
        skip_following_year = False
        kept.append(line)
    return "\n".join(kept).strip()


def normalized_artifact(document: sqlite3.Row, pages: list[sqlite3.Row]) -> str:
    title_zh = document["title_zh"] or ""
    header = [
        "ILIA NORMALIZED LEGAL TEXT",
        "",
        f"Document ID: {document['id']}",
        f"Title: {document['canonical_title']}",
        f"Chinese title: {title_zh}",
        f"Issuing body: {document['issuing_body']}",
        f"Official source: {document['official_source_url']}",
        f"Database version: {DATABASE_VERSION}",
        "",
        "NOTICE",
        "This is an ILIA-generated normalized text for offline legal research.",
        "It is not an official edition. Source-edition page markers are retained",
        "only as factual verification locators. Verify quotations against the",
        "official source linked above.",
        "",
        "=" * 72,
        "",
    ]
    body: list[str] = []
    for page in pages:
        text = strip_edition_matter(page["normalized_text"])
        if not text:
            continue
        body.extend([f"[Source edition page {page['pdf_page']}]", "", text, ""])
    return "\n".join(header + body).rstrip() + "\n"


def delete_document(connection: sqlite3.Connection, document_id: str) -> None:
    version_ids = [
        row[0]
        for row in connection.execute(
            "SELECT id FROM document_versions WHERE document_id = ?", (document_id,)
        )
    ]
    chunk_ids = [
        row[0]
        for row in connection.execute("SELECT id FROM chunks WHERE document_id = ?", (document_id,))
    ]
    for chunk_id in chunk_ids:
        connection.execute("DELETE FROM chunks_fts WHERE chunk_id = ?", (chunk_id,))
    for version_id in version_ids:
        connection.execute("DELETE FROM document_versions WHERE id = ?", (version_id,))
    connection.execute("DELETE FROM cases WHERE document_id = ?", (document_id,))
    connection.execute("DELETE FROM source_files WHERE document_id = ?", (document_id,))
    connection.execute("DELETE FROM document_aliases WHERE document_id = ?", (document_id,))
    connection.execute("DELETE FROM documents WHERE id = ?", (document_id,))


def main() -> None:
    if not BASELINE_DB.is_file():
        raise RuntimeError(f"Baseline database is missing: {BASELINE_DB}")
    OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)

    source = sqlite3.connect(BASELINE_DB)
    source.row_factory = sqlite3.Row
    documents = source.execute(
        "SELECT id, canonical_title, title_zh, document_type, issuing_body, official_source_url "
        "FROM documents ORDER BY id"
    ).fetchall()
    included = [document for document in documents if document["id"] not in EXCLUDED_DOCUMENTS]
    if len(documents) != 50 or len(included) != 49:
        raise RuntimeError(f"Expected 50 baseline and 49 included documents, got {len(documents)} and {len(included)}")

    artifacts: list[dict[str, object]] = []
    for document in included:
        locator_range = source.execute(
            "SELECT min(page_start), max(page_end) FROM chunks WHERE document_id = ?",
            (document["id"],),
        ).fetchone()
        if locator_range[0] is None or locator_range[1] is None:
            raise RuntimeError(f"No content locators for {document['id']}")
        first_page = None if document["document_type"] not in {"judgment", "advisory_opinion", "order"} else locator_range[0]
        pages = source.execute(
            "SELECT sp.pdf_page, sp.normalized_text "
            "FROM source_pages sp JOIN document_versions dv ON dv.id = sp.document_version_id "
            "WHERE dv.document_id = ? AND (? IS NULL OR sp.pdf_page >= ?) AND sp.pdf_page <= ? "
            "ORDER BY sp.pdf_page",
            (document["id"], first_page, first_page, locator_range[1]),
        ).fetchall()
        if not pages:
            raise RuntimeError(f"No normalized source segments for {document['id']}")
        directory = OUTPUT_ROOT / document["id"]
        directory.mkdir(parents=True, exist_ok=True)
        target = directory / "source.txt"
        target.write_text(normalized_artifact(document, pages), encoding="utf-8", newline="\n")
        artifacts.append(
            {
                "document_id": document["id"],
                "path": target.relative_to(ROOT).as_posix(),
                "mime_type": "text/plain; charset=utf-8",
                "byte_length": target.stat().st_size,
                "sha256": sha256_file(target),
                "source_locator_count": len(pages),
                "official_source_url": document["official_source_url"],
                "artifact_status": "ilia_normalized_not_official_edition",
            }
        )
    source.close()

    manifest = {
        "format_version": 1,
        "database_version": DATABASE_VERSION,
        "document_count": len(artifacts),
        "excluded_documents": sorted(EXCLUDED_DOCUMENTS),
        "artifacts": artifacts,
    }
    MANIFEST_PATH.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")

    temporary = RELEASE_DB.with_suffix(".sqlite3.new")
    if temporary.exists():
        temporary.unlink()
    shutil.copyfile(BASELINE_DB, temporary)
    connection = sqlite3.connect(temporary)
    connection.execute("PRAGMA foreign_keys = ON")
    connection.execute("BEGIN IMMEDIATE")
    try:
        for document_id in EXCLUDED_DOCUMENTS:
            delete_document(connection, document_id)
        connection.execute(
            "CREATE TABLE IF NOT EXISTS normalized_document_texts ("
            "document_id TEXT PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE, "
            "text TEXT NOT NULL, sha256 TEXT NOT NULL)"
        )
        connection.execute("DELETE FROM normalized_document_texts")
        for artifact in artifacts:
            connection.execute(
                "UPDATE source_files SET local_file_path = ?, mime_type = ?, byte_length = ?, "
                "sha256 = ?, license_review = 'restricted' WHERE document_id = ?",
                (
                    artifact["path"],
                    artifact["mime_type"],
                    artifact["byte_length"],
                    artifact["sha256"],
                    artifact["document_id"],
                ),
            )
            artifact_path = ROOT / str(artifact["path"])
            connection.execute(
                "INSERT INTO normalized_document_texts(document_id, text, sha256) VALUES (?, ?, ?)",
                (
                    artifact["document_id"],
                    artifact_path.read_text(encoding="utf-8"),
                    artifact["sha256"],
                ),
            )
        connection.execute("UPDATE documents SET database_version = ?", (DATABASE_VERSION,))
        connection.execute("UPDATE document_versions SET version_label = ?", (DATABASE_VERSION,))
        connection.commit()
    except Exception:
        connection.rollback()
        raise
    foreign_key_errors = connection.execute("PRAGMA foreign_key_check").fetchall()
    integrity = connection.execute("PRAGMA integrity_check").fetchone()[0]
    counts = {
        "documents": connection.execute("SELECT count(*) FROM documents").fetchone()[0],
        "chunks": connection.execute("SELECT count(*) FROM chunks").fetchone()[0],
        "embeddings": connection.execute("SELECT count(*) FROM chunk_embeddings").fetchone()[0],
        "fts_rows": connection.execute("SELECT count(*) FROM chunks_fts").fetchone()[0],
        "embedded_texts": connection.execute(
            "SELECT count(*) FROM normalized_document_texts"
        ).fetchone()[0],
    }
    connection.close()
    if (
        foreign_key_errors
        or integrity != "ok"
        or counts["documents"] != 49
        or counts["embedded_texts"] != 49
    ):
        raise RuntimeError(
            f"Release database validation failed: foreign_keys={foreign_key_errors}, "
            f"integrity={integrity}, counts={counts}"
        )
    temporary.replace(RELEASE_DB)

    eval_cases = [
        json.loads(line)
        for line in BASELINE_EVAL.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    release_cases = [
        case for case in eval_cases if case.get("expected_document_id") not in EXCLUDED_DOCUMENTS
    ]
    RELEASE_EVAL.write_text(
        "".join(json.dumps(case, ensure_ascii=False, separators=(",", ":")) + "\n" for case in release_cases),
        encoding="utf-8",
        newline="\n",
    )
    if len(eval_cases) != 200 or len(release_cases) != 197:
        raise RuntimeError(
            f"Expected 200 baseline and 197 release evaluation cases, got {len(eval_cases)} and {len(release_cases)}"
        )
    print(
        json.dumps(
            {
                "database": str(RELEASE_DB),
                "manifest": str(MANIFEST_PATH),
                "evaluation_cases": str(RELEASE_EVAL),
                "evaluation_case_count": len(release_cases),
                **counts,
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
