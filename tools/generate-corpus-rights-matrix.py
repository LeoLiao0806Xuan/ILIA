#!/usr/bin/env python3
"""Generate the v1 corpus redistribution matrix from the frozen inventory.

This is a release-risk classification, not a statement that copyright exists or
does not exist in any particular jurisdiction.  Green must be supported by an
express redistribution grant or written permission; no v1 item currently meets
that threshold.
"""

from __future__ import annotations

import csv
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
INVENTORY = ROOT / "corpus" / "manifests" / "v1_corpus_inventory.csv"
OUTPUT = ROOT / "licenses" / "corpus-redistribution-rights-matrix.csv"

UN_TERMS = "https://www.un.org/en/about-us/terms-of-use"
UNTC_TERMS = "https://treaties.un.org/pages/Overview.aspx?path=overview/usageAgreement/page1_en.xml"
ICRC_TERMS = "https://www.icrc.org/en/copyright-and-terms-use"
ICJ_CONTACT = "https://www.icj-cij.org/contact-the-court"


def classify(row: dict[str, str]) -> dict[str, str]:
    document_id = row["document_id"]
    authority = row["source_authority"]
    if document_id == "icrc-cihl-rules":
        return {
            "rights_group": "ICRC authored publication",
            "normalized_text_rating": "red",
            "source_pdf_rating": "yellow",
            "v1_0_1_packaging_decision": "excluded_from_release_corpus",
            "basis": "Excluded by product-owner decision; no PDF, normalized text, metadata entry or vector is included in the v1.0.1 release corpus.",
            "terms_url": ICRC_TERMS,
            "permission_contact": "ICRC copyright/permissions",
        }
    if authority == "ICRC":
        return {
            "rights_group": "Treaty text in ICRC publication",
            "normalized_text_rating": "yellow",
            "source_pdf_rating": "yellow",
            "v1_0_1_packaging_decision": "package_ilia_normalized_treaty_text_without_source_pdf",
            "basis": "Product-owner decision: package only mechanically normalized treaty text with attribution and an official-source link; exclude publication layout, logos, images and the source PDF. External jurisdiction-specific review remains recommended.",
            "terms_url": ICRC_TERMS,
            "permission_contact": "ICRC copyright/permissions",
        }
    if authority == "International Court of Justice":
        return {
            "rights_group": "ICJ official judicial decision",
            "normalized_text_rating": "yellow",
            "source_pdf_rating": "red",
            "v1_0_1_packaging_decision": "package_ilia_normalized_judicial_text_without_source_pdf",
            "basis": "Product-owner decision: package only mechanically normalized judicial text with provenance and an official-source link; exclude the Court's PDF edition and visual matter. No express worldwide redistribution grant was located, so external jurisdiction-specific review remains recommended.",
            "terms_url": ICJ_CONTACT,
            "permission_contact": "ICJ Registry",
        }
    return {
        "rights_group": "UN legal instrument or official text",
        "normalized_text_rating": "yellow",
        "source_pdf_rating": "red",
        "v1_0_1_packaging_decision": "package_ilia_normalized_legal_text_without_source_pdf",
        "basis": "Product-owner decision: package only mechanically normalized legal text with provenance and an official-source link; exclude protected edition, layout, logos and the source PDF. External jurisdiction-specific review remains recommended.",
        "terms_url": UNTC_TERMS if "Treaty" in authority or document_id in {"un-charter-1945", "icj-statute-1945", "rome-statute-1998"} else UN_TERMS,
        "permission_contact": "United Nations Publications rights and permissions",
    }


def main() -> None:
    with INVENTORY.open("r", encoding="utf-8-sig", newline="") as stream:
        inventory = list(csv.DictReader(stream))
    if len(inventory) != 50:
        raise RuntimeError(f"Expected frozen 50-document inventory, found {len(inventory)}")
    fields = [
        "document_id",
        "canonical_title",
        "document_type",
        "source_authority",
        "rights_group",
        "normalized_text_rating",
        "source_pdf_rating",
        "v1_0_1_packaging_decision",
        "basis",
        "terms_url",
        "permission_contact",
        "reviewed_at",
    ]
    rows = []
    for item in inventory:
        rows.append(
            {
                "document_id": item["document_id"],
                "canonical_title": item["canonical_title"],
                "document_type": item["document_type"],
                "source_authority": item["source_authority"],
                **classify(item),
                "reviewed_at": "2026-09-24",
            }
        )
    with OUTPUT.open("w", encoding="utf-8", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)
    print(f"Wrote {len(rows)} rows to {OUTPUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
