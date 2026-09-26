#!/usr/bin/env python3
"""Build the deterministic 300-case ILIA 1.1 retrieval evaluation suite."""

from __future__ import annotations

import argparse
import json
import sqlite3
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BASELINE = ROOT / "tests" / "eval" / "retrieval_normalized.jsonl"
OUTPUT = ROOT / "tests" / "eval" / "retrieval_1.1.jsonl"
DATABASE = ROOT / "data" / "ilia.sqlite3"
TOPICS = ROOT / "corpus" / "manifests" / "document_topics.v1.json"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify that the committed suite and topic metadata are current",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    cases = [
        json.loads(line)
        for line in BASELINE.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    connection = sqlite3.connect(DATABASE)
    documents = connection.execute(
        "SELECT id, canonical_title FROM documents ORDER BY id"
    ).fetchall()
    aliases = connection.execute(
        """SELECT d.id, a.alias, a.language FROM documents d
           JOIN document_aliases a ON a.document_id = d.id
           WHERE a.alias_kind != 'canonical_title'
           ORDER BY d.id, CASE WHEN a.language LIKE 'zh%' THEN 0 ELSE 1 END, length(a.alias)"""
    ).fetchall()
    connection.close()

    document_ids = {document_id for document_id, _title in documents}
    topic_registry = json.loads(TOPICS.read_text(encoding="utf-8"))
    topic_document_ids = {
        document_id
        for topic_documents in topic_registry["topics"].values()
        for document_id in topic_documents
    }
    unknown_topic_documents = sorted(topic_document_ids - document_ids)
    if unknown_topic_documents:
        raise RuntimeError(
            "topic metadata references unknown documents: "
            + ", ".join(unknown_topic_documents)
        )

    for document_id, title in documents:
        cases.append(
            {
                "id": f"v1.1-name-{document_id}",
                "query": title,
                "expected_document_id": document_id,
                "tags": ["document_name", "en", "v1.1"],
            }
        )

    first_alias: dict[str, tuple[str, str]] = {}
    for document_id, alias, language in aliases:
        first_alias.setdefault(document_id, (alias, language))
    for document_id, _title in documents:
        alias, language = first_alias[document_id]
        cases.append(
            {
                "id": f"v1.1-alias-{document_id}",
                "query": alias,
                "expected_document_id": document_id,
                "tags": [
                    "document_name",
                    "alias",
                    "zh" if language.startswith("zh") else "en",
                    "v1.1",
                ],
            }
        )

    cases.extend(
        [
            {
                "id": "v1.1-roman-unclos-iii",
                "query": "UNCLOS Art. III",
                "expected_document_id": "unclos-1982",
                "expected_citation_label": "UNCLOS, Article 3",
                "tags": ["exact", "exact_locator", "roman", "en", "v1.1"],
            },
            {
                "id": "v1.1-roman-vclt-xxvii",
                "query": "VCLT Article XXVII",
                "expected_document_id": "vclt-1969",
                "expected_citation_label": "VCLT, Article 27",
                "tags": ["exact", "exact_locator", "roman", "en", "v1.1"],
            },
            {
                "id": "v1.1-roman-charter-li",
                "query": "UN Charter Art. LI",
                "expected_document_id": "un-charter-1945",
                "expected_citation_label": "UN Charter, Article 51",
                "tags": ["exact", "exact_locator", "roman", "en", "v1.1"],
            },
            {
                "id": "v1.1-filter-document-unclos",
                "query": "Article 3",
                "expected_document_id": "unclos-1982",
                "expected_citation_label": "UNCLOS, Article 3",
                "filters": {
                    "library": "core_only",
                    "document_keys": ["core:unclos-1982"]
                },
                "tags": ["filter", "document_filter", "exact_locator", "v1.1"],
            },
            {
                "id": "v1.1-filter-type-negative",
                "query": "Nicaragua paragraph 191",
                "expect_no_results": True,
                "filters": {
                    "library": "core_only",
                    "document_types": ["treaty"]
                },
                "tags": ["filter", "type_filter", "negative", "v1.1"],
            },
        ]
    )

    cases.extend(
        [
            {
                "id": "v1.1-typo-unclos",
                "query": "UNCLO",
                "expected_document_id": "unclos-1982",
                "tags": ["document_name", "typo", "en", "v1.1"],
            },
            {
                "id": "v1.1-typo-vclt",
                "query": "Vienna Convention on the Law of Treates",
                "expected_document_id": "vclt-1969",
                "tags": ["document_name", "typo", "en", "v1.1"],
            },
            {
                "id": "v1.1-typo-nicaragua",
                "query": "Nicarauga",
                "expected_document_id": "icj-nicaragua-1986-merits",
                "tags": ["document_name", "typo", "en", "v1.1"],
            },
            {
                "id": "v1.1-typo-iccpr",
                "query": "International Covenant on Civil and Politcal Rights",
                "expected_document_id": "iccpr-1966",
                "tags": ["document_name", "typo", "en", "v1.1"],
            },
            {
                "id": "v1.1-typo-immunities",
                "query": "Jurisdictional Immunites of the State (Germany v. Italy: Greece intervening), Judgment",
                "expected_document_id": "icj-jurisdictional-immunities-2012",
                "tags": ["document_name", "typo", "en", "v1.1"],
            },
        ]
    )

    if len(cases) != 305:
        raise RuntimeError(f"expected exactly 305 cases, generated {len(cases)}")
    ids = [case["id"] for case in cases]
    if len(ids) != len(set(ids)):
        raise RuntimeError("evaluation case IDs are not unique")
    rendered = "".join(json.dumps(case, ensure_ascii=False) + "\n" for case in cases)
    if args.check:
        if not OUTPUT.exists():
            raise RuntimeError(f"missing generated evaluation suite: {OUTPUT}")
        if OUTPUT.read_text(encoding="utf-8") != rendered:
            raise RuntimeError(
                "retrieval_1.1.jsonl is stale; run tools/build_retrieval_eval_1_1.py"
            )
        print(
            f"Verified {len(cases)} cases and {len(topic_document_ids)} topic mappings"
        )
        return

    OUTPUT.write_text(rendered, encoding="utf-8")
    print(f"Wrote {len(cases)} cases to {OUTPUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
