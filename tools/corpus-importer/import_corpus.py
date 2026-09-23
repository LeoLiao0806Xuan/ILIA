#!/usr/bin/env python3
"""Build the ILIA SQLite/FTS5 prototype from immutable source PDFs."""

from __future__ import annotations

import hashlib
import json
import os
import re
import sqlite3
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

import pdfplumber


ROOT = Path(__file__).resolve().parents[2]
MANIFEST_PATH = ROOT / "corpus" / "manifests" / "prototype_manifest.json"
SCHEMA_PATH = ROOT / "corpus" / "schemas" / "001_initial.sql"
DB_PATH = Path(os.environ.get("ILIA_DB_PATH", ROOT / "data" / "ilia_prototype.sqlite3"))
IMPORTER_VERSION = "0.2.0"


@dataclass(frozen=True)
class Page:
    number: int
    raw_text: str
    clean_text: str
    extraction_method: str
    review_status: str


@dataclass(frozen=True)
class Unit:
    number: int
    page_start: int
    page_end: int
    text: str
    heading: str | None = None


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def normalize_text(text: str) -> str:
    text = text.replace("\u00ad", "").replace("\r\n", "\n").replace("\r", "\n")
    text = re.sub(r"(?i)\bArticle\s*\n\s*(\d+)\b", r"Article \1", text)
    text = re.sub(r"(?<=\w)-\n(?=[a-z])", "", text)
    text = re.sub(r"[ \t]+", " ", text)
    text = re.sub(r" *\n *", "\n", text)
    text = re.sub(r"\n{3,}", "\n\n", text)
    return text.strip()


def clean_page(text: str, document_id: str) -> str:
    lines = [line.rstrip() for line in text.replace("\r", "").split("\n")]
    kept: list[str] = []
    for line in lines:
        stripped = line.strip()
        if document_id == "icj-nicaragua-1986-merits":
            if re.fullmatch(r"\d+\s+MILITARY AND PARAMILITARY ACTIVITIES \(JUDGMENT\)", stripped, re.I):
                continue
            if re.fullmatch(r"\d+", stripped):
                continue
        if stripped in {
            "Vienna Convention on the Law of Treaties",
            "Responsibility of States for Internationally Wrongful Acts",
        }:
            continue
        kept.append(line)
    cleaned = normalize_text("\n".join(kept))
    if document_id == "un-charter-1945":
        cleaned = cleaned.replace("Ünited Nations", "United Nations")
        cleaned = cleaned.replace("selfdefense", "self-defense")
        cleaned = cleaned.replace("CHAPTER VIH", "CHAPTER VIII")
    return cleaned


def load_overrides(source_path: Path) -> dict[int, dict[str, str]]:
    path = source_path.parent / "page_text_overrides.json"
    if not path.exists():
        return {}
    raw = json.loads(path.read_text(encoding="utf-8"))
    return {int(page): value for page, value in raw.items()}


def find_column_split(pdf_page) -> float:
    """Find the page-specific gutter; the scanned Charter shifts columns by page."""
    lower = int(pdf_page.width * 0.35)
    upper = int(pdf_page.width * 0.65)
    occupied = {
        int(character["x0"])
        for character in pdf_page.chars
        if character.get("text", "").strip()
    }
    gaps: list[tuple[int, int, int]] = []
    gap_start: int | None = None
    for position in range(lower, upper + 1):
        if position not in occupied and gap_start is None:
            gap_start = position
        elif position in occupied and gap_start is not None:
            gaps.append((position - gap_start, gap_start, position - 1))
            gap_start = None
    if gap_start is not None:
        gaps.append((upper - gap_start + 1, gap_start, upper))
    longest = max(gaps, default=(0, 0, 0))
    if longest[0] < 8:
        return pdf_page.width / 2
    return (longest[1] + longest[2]) / 2


def extract_pages(source_path: Path, document: dict) -> tuple[list[Page], int]:
    overrides = load_overrides(source_path)
    document_id = document["id"]
    options = document.get("extraction_options", {})
    page_start = int(options.get("page_start", 1))
    page_end = options.get("page_end")
    replacements = options.get("regex_replacements", [])
    pages: list[Page] = []
    with pdfplumber.open(source_path) as pdf:
        pdf_page_count = len(pdf.pages)
        selected_end = int(page_end or pdf_page_count)
        for page_number, pdf_page in enumerate(pdf.pages, 1):
            if not page_start <= page_number <= selected_end:
                continue
            if options.get("layout") == "two_columns":
                midpoint = find_column_split(pdf_page)
                left = pdf_page.crop((0, 0, midpoint, pdf_page.height)).extract_text() or ""
                right = pdf_page.crop((midpoint, 0, pdf_page.width, pdf_page.height)).extract_text() or ""
                extracted = f"{left}\n{right}"
                default_method = "pdf_text_layer_two_columns"
            else:
                extracted = pdf_page.extract_text() or ""
                default_method = "pdf_text_layer"
            for replacement in replacements:
                if replacement.get("page") not in (None, page_number):
                    continue
                extracted = re.sub(
                    replacement["pattern"], replacement["replacement"], extracted, flags=re.MULTILINE
                )
            if page_number in overrides:
                override = overrides[page_number]
                raw_text = override["text"]
                method = override["method"]
                status = override["review_status"]
            else:
                raw_text = extracted
                method = default_method
                status = "auto_pass" if raw_text.strip() else "needs_review"
            pages.append(
                Page(
                    number=page_number,
                    raw_text=raw_text,
                    clean_text=clean_page(raw_text, document_id),
                    extraction_method=method,
                    review_status=status,
                )
            )
    return pages, pdf_page_count


def flatten_pages(pages: list[Page]) -> tuple[str, list[tuple[int, int, int]]]:
    parts: list[str] = []
    spans: list[tuple[int, int, int]] = []
    cursor = 0
    for page in pages:
        page_text = page.clean_text + "\n"
        parts.append(page_text)
        spans.append((cursor, cursor + len(page_text), page.number))
        cursor += len(page_text)
    return "".join(parts), spans


def page_for_offset(spans: list[tuple[int, int, int]], offset: int) -> int:
    for start, end, page_number in spans:
        if start <= offset < end:
            return page_number
    return spans[-1][2]


def select_consecutive(candidates: list[tuple[int, int]], expected_count: int) -> list[tuple[int, int]]:
    selected: list[tuple[int, int]] = []
    expected = 1
    for number, offset in candidates:
        if number == expected:
            selected.append((number, offset))
            expected += 1
            if expected > expected_count:
                break
    return selected


def longest_consecutive(candidates: list[tuple[int, int]]) -> list[tuple[int, int]]:
    """Pick the longest monotonically located 1..N run from repeated PDF labels."""
    best: list[tuple[int, int]] = []
    for start_index, (number, _) in enumerate(candidates):
        if number != 1:
            continue
        selected: list[tuple[int, int]] = []
        expected = 1
        for candidate in candidates[start_index:]:
            if candidate[0] == expected:
                selected.append(candidate)
                expected += 1
        if len(selected) > len(best):
            best = selected
    return best


def article_number(value: str) -> int:
    cleaned = value.strip().lower().replace("i", "1").replace("l", "1")
    return int(cleaned)


def parse_articles(pages: list[Page], expected_count: int, *, allow_period_headings: bool = False) -> list[Unit]:
    text, spans = flatten_pages(pages)
    # Official treaty PDFs use both a standalone label ("Article 1") and an
    # inline heading ("Article 1 — Scope").  Restrict the optional suffix to
    # heading punctuation so prose references do not become false candidates.
    heading_separator = r"[.—–-]" if allow_period_headings else r"[—–-]"
    pattern = re.compile(
        rf"(?im)^\s*Article\s+([0-9Il]+)(?:\s*{heading_separator}\s*[^\n]+)?\s*$"
    )
    candidates = [(article_number(match.group(1)), match.start()) for match in pattern.finditer(text)]
    selected = select_consecutive(candidates, expected_count)
    units: list[Unit] = []
    for index, (number, start) in enumerate(selected):
        end = selected[index + 1][1] if index + 1 < len(selected) else len(text)
        segment = normalize_text(text[start:end])
        after_label = re.sub(r"(?is)^\s*Article\s+[0-9Il]+\s*", "", segment, count=1).strip()
        heading = next((line.strip() for line in after_label.splitlines() if line.strip()), None)
        units.append(
            Unit(
                number=number,
                page_start=page_for_offset(spans, start),
                page_end=page_for_offset(spans, max(start, end - 1)),
                text=segment,
                heading=heading,
            )
        )
    return units


def parse_case_paragraphs(pages: list[Page], expected_count: int | None) -> list[Unit]:
    text, spans = flatten_pages(pages)
    pattern = re.compile(r"(?m)^\s*((?:\d\s*){1,3})\.\s+")
    candidates: list[tuple[int, int]] = []
    for match in pattern.finditer(text):
        number = int(re.sub(r"\s", "", match.group(1)))
        candidates.append((number, match.start()))
    selected = (
        select_consecutive(candidates, expected_count)
        if expected_count is not None
        else longest_consecutive(candidates)
    )
    units: list[Unit] = []
    for index, (number, start) in enumerate(selected):
        end = selected[index + 1][1] if index + 1 < len(selected) else len(text)
        segment = normalize_text(text[start:end])
        segment = re.sub(r"^\s*(?:\d\s*){1,3}\.\s*", f"{number}. ", segment, count=1)
        units.append(
            Unit(
                number=number,
                page_start=page_for_offset(spans, start),
                page_end=page_for_offset(spans, max(start, end - 1)),
                text=segment,
            )
        )
    return units


def parse_case_passages(pages: list[Page]) -> list[Unit]:
    units: list[Unit] = []
    for page in pages:
        text = normalize_text(page.clean_text)
        if len(text) < 200:
            continue
        units.append(
            Unit(
                number=len(units) + 1,
                page_start=page.number,
                page_end=page.number,
                text=text,
                heading=f"PDF page {page.number}",
            )
        )
    return units


def parse_rules(pages: list[Page], expected_count: int) -> list[Unit]:
    text, spans = flatten_pages(pages)
    pattern = re.compile(r"(?im)^\s*Rule\s+(\d{1,3})\.?\s*(.*)$")
    candidates = [(int(match.group(1)), match.start()) for match in pattern.finditer(text)]
    selected = select_consecutive(candidates, expected_count)
    units: list[Unit] = []
    for index, (number, start) in enumerate(selected):
        end = selected[index + 1][1] if index + 1 < len(selected) else len(text)
        segment = normalize_text(text[start:end])
        units.append(
            Unit(
                number=number,
                page_start=page_for_offset(spans, start),
                page_end=page_for_offset(spans, max(start, end - 1)),
                text=segment,
                heading=next((line.strip() for line in segment.splitlines()[1:] if line.strip()), None),
            )
        )
    return units


def insert_document(
    connection: sqlite3.Connection,
    manifest: dict,
    document: dict,
    manifest_sha256: str,
    import_run_id: int,
) -> tuple[int, list[Page], list[Unit]]:
    source_path = ROOT / document["local_file_path"]
    actual_hash = sha256_file(source_path)
    if actual_hash != document["sha256"]:
        raise ValueError(f"SHA-256 mismatch for {document['id']}: {actual_hash}")

    pages, pdf_page_count = extract_pages(source_path, document)
    parser = document["parser"]
    expected_raw = document.get("expected_units")
    expected = int(expected_raw) if expected_raw is not None else None
    if parser == "articles":
        units = parse_articles(
            pages,
            expected or 0,
            allow_period_headings=bool(document.get("extraction_options", {}).get("article_period_headings")),
        )
    elif parser == "rules":
        units = parse_rules(pages, expected or 0)
    elif parser == "case_paragraphs":
        units = parse_case_paragraphs(pages, expected)
    elif parser == "case_auto":
        paragraphs = parse_case_paragraphs(pages, expected)
        substantive_pages = sum(1 for page in pages if len(page.clean_text) >= 200)
        minimum_numbered = max(
            int(document.get("minimum_numbered_paragraphs", 10)),
            int(substantive_pages * float(document.get("numbered_paragraph_page_ratio", 1.5))),
        )
        if len(paragraphs) >= minimum_numbered:
            units = paragraphs
            parser = "case_paragraphs"
        else:
            units = parse_case_passages(pages)
            parser = "case_passages"
    elif parser == "case_passages":
        units = parse_case_passages(pages)
    else:
        raise ValueError(f"Unsupported parser for {document['id']}: {parser}")

    connection.execute(
        """INSERT INTO documents
        (id, canonical_title, title_zh, short_title, document_type, issuing_body,
         adoption_date, entry_into_force_date, legal_status, official_source_url, database_version)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
        (
            document["id"], document["canonical_title"], document.get("title_zh"),
            document.get("short_title"), document["document_type"], document["issuing_body"],
            document.get("adoption_date"), document.get("entry_into_force_date"),
            document["legal_status"], document["official_source_url"], manifest["database_version"],
        ),
    )
    for language, alias, kind in (
        ("en", document["canonical_title"], "canonical_title"),
        ("zh-CN", document.get("title_zh"), "translated_title"),
        ("en", document.get("short_title"), "abbreviation"),
    ):
        if alias:
            connection.execute(
                "INSERT INTO document_aliases (document_id, language, alias, alias_kind) VALUES (?, ?, ?, ?)",
                (document["id"], language, alias, kind),
            )
    for alias in document.get("aliases", []):
        connection.execute(
            "INSERT INTO document_aliases (document_id, language, alias, alias_kind) VALUES (?, ?, ?, ?)",
            (document["id"], "en", alias, "short_name"),
        )

    source_cursor = connection.execute(
        """INSERT INTO source_files
        (document_id, source_authority, official_source_url, acquisition_url, source_accessed_at,
         local_file_path, mime_type, byte_length, sha256, page_count, license_review)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
        (
            document["id"], document["source_authority"], document["official_source_url"],
            document.get("acquisition_url"), document["source_accessed_at"],
            document["local_file_path"], document["mime_type"], source_path.stat().st_size,
            actual_hash, pdf_page_count, document["license_review"],
        ),
    )
    source_file_id = int(source_cursor.lastrowid)
    version_cursor = connection.execute(
        """INSERT INTO document_versions
        (document_id, source_file_id, language, is_official_text, translation_type, version_label, valid_from)
        VALUES (?, ?, ?, ?, ?, ?, ?)""",
        (
            document["id"], source_file_id, document["language"], int(document["is_official_text"]),
            document["translation_type"], manifest["database_version"], document.get("adoption_date"),
        ),
    )
    version_id = int(version_cursor.lastrowid)

    for page in pages:
        connection.execute(
            """INSERT INTO source_pages
            (document_version_id, pdf_page, raw_text, normalized_text, extraction_method, review_status)
            VALUES (?, ?, ?, ?, ?, ?)""",
            (version_id, page.number, page.raw_text, page.clean_text, page.extraction_method, page.review_status),
        )
        if page.review_status in {"needs_review", "needs_second_review"}:
            connection.execute(
                """INSERT INTO qa_findings
                (import_run_id, document_id, severity, check_code, message)
                VALUES (?, ?, 'warning', ?, ?)""",
                (
                    import_run_id, document["id"], "PAGE_REVIEW_REQUIRED",
                    f"PDF page {page.number} uses {page.extraction_method} and is marked {page.review_status}.",
                ),
            )

    if document["document_type"] in {"judgment", "advisory_opinion", "order"}:
        case_id = document["id"]
        connection.execute(
            """INSERT INTO cases
            (id, document_id, case_number, case_name, decision_date, decision_kind,
             applicant, respondent, official_report_citation)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            (
                case_id, document["id"], document.get("case_number"), document["canonical_title"],
                document["adoption_date"], document.get("decision_kind", "judgment"),
                document.get("applicant"), document.get("respondent"),
                document.get("official_report_citation"),
            ),
        )

    for unit in units:
        if parser in {"articles", "rules"}:
            label = "Rule" if parser == "rules" else "Article"
            citation = f"{document['short_title']}, {label} {unit.number}"
            cursor = connection.execute(
                """INSERT INTO provisions
                (document_version_id, article_number, heading, text_original, text_normalized,
                 page_start, page_end, citation_label)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)""",
                (
                    version_id, str(unit.number), unit.heading, unit.text, normalize_text(unit.text),
                    unit.page_start, unit.page_end, citation,
                ),
            )
            locator_type = "article"
            slug = "rule" if parser == "rules" else "art"
            chunk_id = f"{document['id']}-{slug}-{unit.number}-{document['language']}"
        elif parser == "case_paragraphs":
            citation = f"{document.get('citation_prefix', document['short_title'])}, para. {unit.number}"
            cursor = connection.execute(
                """INSERT INTO case_paragraphs
                (case_id, document_version_id, document_kind, paragraph_number, page_start, page_end,
                 language, text_original, text_normalized, citation_label, source_url)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (
                    document["id"], version_id, document["document_type"], unit.number,
                    unit.page_start, unit.page_end, document["language"], unit.text,
                    normalize_text(unit.text), citation, document["official_source_url"],
                ),
            )
            locator_type = "case_paragraph"
            chunk_id = f"{document['id']}-para-{unit.number}-{document['language']}"
        else:
            citation = f"{document.get('citation_prefix', document['short_title'])}, PDF p. {unit.page_start}"
            cursor = connection.execute(
                """INSERT INTO case_passages
                (case_id, document_version_id, passage_number, page_start, page_end, language,
                 text_original, text_normalized, citation_label, source_url)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (
                    document["id"], version_id, unit.number, unit.page_start, unit.page_end,
                    document["language"], unit.text, normalize_text(unit.text), citation,
                    document["official_source_url"],
                ),
            )
            locator_type = "case_passage"
            chunk_id = f"{document['id']}-page-{unit.page_start}-{document['language']}"
        locator_id = int(cursor.lastrowid)
        normalized = normalize_text(unit.text)
        connection.execute(
            """INSERT INTO chunks
            (id, document_id, document_version_id, locator_type, locator_id, citation_label,
             page_start, page_end, language, text_original, text_normalized, token_count_estimate)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            (
                chunk_id, document["id"], version_id, locator_type, locator_id, citation,
                unit.page_start, unit.page_end, document["language"], unit.text, normalized,
                max(1, len(normalized) // 4),
            ),
        )
        connection.execute(
            """INSERT INTO chunks_fts
            (chunk_id, document_id, canonical_title, citation_label, text)
            VALUES (?, ?, ?, ?, ?)""",
            (chunk_id, document["id"], document["canonical_title"], citation, normalized),
        )

    if expected is not None and len(units) != expected:
        connection.execute(
            """INSERT INTO qa_findings
            (import_run_id, document_id, severity, check_code, message)
            VALUES (?, ?, 'error', 'UNIT_COUNT_MISMATCH', ?)""",
            (import_run_id, document["id"], f"Expected {expected} units, parsed {len(units)}."),
        )
    elif expected is None:
        connection.execute(
            """INSERT INTO qa_findings
            (import_run_id, document_id, severity, check_code, message)
            VALUES (?, ?, 'warning', 'AUTO_UNIT_COUNT_NOT_FROZEN', ?)""",
            (import_run_id, document["id"], f"Automatically parsed {len(units)} {parser} units; manual count review required."),
        )
    return version_id, pages, units


def main() -> None:
    manifest_bytes = MANIFEST_PATH.read_bytes()
    manifest = json.loads(manifest_bytes.decode("utf-8"))
    manifest_sha256 = hashlib.sha256(manifest_bytes).hexdigest()
    DB_PATH.parent.mkdir(parents=True, exist_ok=True)
    if DB_PATH.exists():
        DB_PATH.unlink()

    connection = sqlite3.connect(DB_PATH)
    connection.execute("PRAGMA foreign_keys = ON")
    connection.executescript(SCHEMA_PATH.read_text(encoding="utf-8"))
    connection.execute(
        "INSERT INTO schema_metadata (schema_version, applied_at) VALUES ('001', ?)",
        (utc_now(),),
    )
    run_cursor = connection.execute(
        """INSERT INTO import_runs
        (database_version, started_at, status, manifest_sha256, importer_version)
        VALUES (?, ?, 'running', ?, ?)""",
        (manifest["database_version"], utc_now(), manifest_sha256, IMPORTER_VERSION),
    )
    import_run_id = int(run_cursor.lastrowid)

    try:
        for document in manifest["documents"]:
            _, pages, units = insert_document(connection, manifest, document, manifest_sha256, import_run_id)
            print(f"Imported {document['id']}: {len(pages)} pages, {len(units)} units")
        errors = connection.execute(
            "SELECT COUNT(*) FROM qa_findings WHERE import_run_id = ? AND severity = 'error'",
            (import_run_id,),
        ).fetchone()[0]
        status = "failed" if errors else "passed"
        connection.execute(
            "UPDATE import_runs SET completed_at = ?, status = ? WHERE id = ?",
            (utc_now(), status, import_run_id),
        )
        connection.commit()
        if errors:
            raise RuntimeError(f"Import completed with {errors} error finding(s)")
    except Exception as exc:
        connection.execute(
            "UPDATE import_runs SET completed_at = ?, status = 'failed', notes = ? WHERE id = ?",
            (utc_now(), str(exc), import_run_id),
        )
        connection.commit()
        raise
    finally:
        connection.close()

    print(f"Database: {DB_PATH}")


if __name__ == "__main__":
    main()
