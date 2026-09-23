#!/usr/bin/env python3
"""Acquire the frozen ILIA V1 corpus from official sources and expand its manifest."""

from __future__ import annotations

import csv
import hashlib
import json
import shutil
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MANIFEST_PATH = ROOT / "corpus" / "manifests" / "prototype_manifest.json"
INVENTORY_PATH = ROOT / "corpus" / "manifests" / "v1_corpus_inventory.csv"
ACCESS_DATE = "2026-09-22"

ICJ_BASE = "https://www.icj-cij.org/sites/default/files/case-related"
ICRC_GENEVA = "https://www.icrc.org/sites/default/files/external/doc/en/assets/files/publications/icrc-002-0173.pdf"
ICRC_PROTOCOLS = "https://www.icrc.org/en/doc/assets/files/other/icrc_002_0321.pdf"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def treaty(
    document_id: str,
    short_title: str,
    authority: str,
    url: str,
    adopted: str | None,
    effective: str | None,
    expected: int,
    *,
    document_type: str = "treaty",
    legal_status: str = "in_force_treaty",
    parser: str = "articles",
    extraction_options: dict | None = None,
    copy_from: str | None = None,
) -> dict:
    value = {
        "id": document_id,
        "short_title": short_title,
        "document_type": document_type,
        "issuing_body": authority,
        "adoption_date": adopted,
        "entry_into_force_date": effective,
        "legal_status": legal_status,
        "official_source_url": url,
        "source_authority": authority,
        "parser": parser,
        "expected_units": expected,
    }
    if extraction_options:
        value["extraction_options"] = extraction_options
    if copy_from:
        value["copy_from"] = copy_from
    return value


NORMATIVE = [
    treaty(
        "icj-statute-1945", "ICJ Statute", "International Court of Justice",
        "https://treaties.un.org/doc/Publication/CTC/uncharter-all-lang.pdf",
        "1945-06-26", "1945-10-24", 70,
        document_type="statute", legal_status="constitutive_statute_in_force",
        extraction_options={
            "page_start": 23,
            "page_end": 35,
            "layout": "two_columns",
            "regex_replacements": [
                {"page": 24, "pattern": r"^\s*Article 96\s*$", "replacement": "Article 12"},
                {"page": 31, "pattern": r"^\s*Article 96\s*$", "replacement": "Article 60"},
            ],
        },
        copy_from="corpus/sources/un-charter-1945/source.pdf",
    ),
    treaty(
        "rome-statute-1998", "Rome Statute", "United Nations Treaty Collection",
        "https://treaties.un.org/doc/Treaties/1998/07/19980717%2006-33%20PM/Ch_XVIII_10p.pdf",
        "1998-07-17", "2002-07-01", 128,
        extraction_options={
            "page_start": 92,
            "page_end": 196,
            "article_period_headings": True,
            "regex_replacements": [
                {"page": 112, "pattern": r"^Non-retroactivity ratione personae$", "replacement": "Article 24\nNon-retroactivity ratione personae"},
                {"page": 131, "pattern": r"^Salaries, allowances and expenses$", "replacement": "Article 49\nSalaries, allowances and expenses"},
                {"page": 168, "pattern": r"^Article S8$", "replacement": "Article 88"},
                {"page": 168, "pattern": r"^Surrender of persons to the Court$", "replacement": "Article 89\nSurrender of persons to the Court"},
            ],
        },
    ),
    treaty("cat-1984", "CAT", "United Nations OHCHR", "https://www.ohchr.org/sites/default/files/Documents/ProfessionalInterest/cat.pdf", "1984-12-10", "1987-06-26", 33),
    treaty("icerd-1965", "ICERD", "United Nations OHCHR", "https://www.ohchr.org/sites/default/files/Documents/ProfessionalInterest/cerd.pdf", "1965-12-21", "1969-01-04", 25),
    treaty("cedaw-1979", "CEDAW", "United Nations OHCHR", "https://www.ohchr.org/sites/default/files/Documents/ProfessionalInterest/cedaw.pdf", "1979-12-18", "1981-09-03", 30),
    treaty("crc-1989", "CRC", "United Nations OHCHR", "https://www.ohchr.org/sites/default/files/Documents/ProfessionalInterest/crc.pdf", "1989-11-20", "1990-09-02", 54),
    treaty("gc-i-1949", "GC I", "International Committee of the Red Cross", ICRC_GENEVA, "1949-08-12", "1950-10-21", 64, extraction_options={"page_start": 39, "page_end": 64}),
    treaty("gc-ii-1949", "GC II", "International Committee of the Red Cross", ICRC_GENEVA, "1949-08-12", "1950-10-21", 63, extraction_options={"page_start": 65, "page_end": 84}),
    treaty("gc-iii-1949", "GC III", "International Committee of the Red Cross", ICRC_GENEVA, "1949-08-12", "1950-10-21", 143, extraction_options={"page_start": 85, "page_end": 154}),
    treaty(
        "gc-iv-1949", "GC IV", "International Committee of the Red Cross", ICRC_GENEVA,
        "1949-08-12", "1950-10-21", 159,
        extraction_options={
            "page_start": 155,
            "page_end": 223,
            "regex_replacements": [
                {"pattern": r"^\s*Article\s+70\s+arrest\s*$", "replacement": "Article 70"},
                {"pattern": r"^\s*Article\s+80\s+provisions\s*$", "replacement": "Article 80"},
            ],
        },
    ),
    treaty("ap-i-1977", "AP I", "International Committee of the Red Cross", ICRC_PROTOCOLS, "1977-06-08", "1978-12-07", 102, extraction_options={"page_start": 14, "page_end": 73}),
    treaty("ap-ii-1977", "AP II", "International Committee of the Red Cross", ICRC_PROTOCOLS, "1977-06-08", "1978-12-07", 28, extraction_options={"page_start": 88, "page_end": 97}),
    treaty(
        "icrc-cihl-rules", "ICRC Customary IHL Rules", "International Committee of the Red Cross",
        "https://www.icrc.org/eng/assets/files/other/icrc_002_0860.pdf",
        "2005-01-01", None, 161,
        document_type="customary_rule", legal_status="icrc_customary_law_study", parser="rules",
    ),
]


# id, case number, decision date, kind, official PDF stem, citation prefix
CASE_ROWS = [
    ("icj-corfu-channel-1949-merits", "1", "1949-04-09", "judgment", "001-19490409-JUD-01-00-EN.pdf", "Corfu Channel (Merits)"),
    ("icj-reparation-injuries-1949", "4", "1949-04-11", "advisory_opinion", "004-19490411-ADV-01-00-EN.pdf", "Reparation for Injuries"),
    ("icj-asylum-1950", "7", "1950-11-20", "judgment", "007-19501120-JUD-01-00-EN.pdf", "Asylum"),
    ("icj-genocide-reservations-1951", "12", "1951-05-28", "advisory_opinion", "012-19510528-ADV-01-00-EN.pdf", "Genocide Reservations"),
    ("icj-nottebohm-1955", "18", "1955-04-06", "judgment", "018-19550406-JUD-01-00-EN.pdf", "Nottebohm (Second Phase)"),
    ("icj-right-passage-1960", "32", "1960-04-12", "judgment", "032-19600412-JUD-01-00-EN.pdf", "Right of Passage (Merits)"),
    ("icj-temple-preah-vihear-1962", "45", "1962-06-15", "judgment", "045-19620615-JUD-01-00-EN.pdf", "Temple of Preah Vihear (Merits)"),
    ("icj-north-sea-1969", "52", "1969-02-20", "judgment", "052-19690220-JUD-01-00-EN.pdf", "North Sea Continental Shelf"),
    ("icj-barcelona-traction-1970", "50", "1970-02-05", "judgment", "050-19700205-JUD-01-00-EN.pdf", "Barcelona Traction (Second Phase)"),
    ("icj-namibia-1971", "53", "1971-06-21", "advisory_opinion", "053-19710621-ADV-01-00-EN.pdf", "Namibia Advisory Opinion"),
    ("icj-nuclear-tests-1974", "58", "1974-12-20", "judgment", "058-19741220-JUD-01-00-EN.pdf", "Nuclear Tests"),
    ("icj-western-sahara-1975", "61", "1975-10-16", "advisory_opinion", "061-19751016-ADV-01-00-EN.pdf", "Western Sahara"),
    ("icj-continental-shelf-tunisia-libya-1982", "63", "1982-02-24", "judgment", "063-19820224-JUD-01-00-EN.pdf", "Continental Shelf (Tunisia/Libya)"),
    ("icj-frontier-dispute-1986", "69", "1986-12-22", "judgment", "069-19861222-JUD-01-00-EN.pdf", "Frontier Dispute"),
    ("icj-elsi-1989", "76", "1989-07-20", "judgment", "076-19890720-JUD-01-00-EN.pdf", "Elettronica Sicula (ELSI)"),
    ("icj-nauru-preliminary-1992", "80", "1992-06-26", "judgment", "080-19920626-JUD-01-00-EN.pdf", "Certain Phosphate Lands in Nauru (Preliminary Objections)"),
    ("icj-bosnian-genocide-preliminary-1996", "91", "1996-07-11", "judgment", "091-19960711-JUD-01-00-EN.pdf", "Bosnian Genocide (Preliminary Objections)"),
    ("icj-nuclear-weapons-1996", "95", "1996-07-08", "advisory_opinion", "095-19960708-ADV-01-00-EN.pdf", "Nuclear Weapons Advisory Opinion"),
    ("icj-gabcikovo-1997", "92", "1997-09-25", "judgment", "092-19970925-JUD-01-00-EN.pdf", "Gabčíkovo-Nagymaros Project"),
    ("icj-lagrand-2001", "104", "2001-06-27", "judgment", "104-20010627-JUD-01-00-EN.pdf", "LaGrand"),
    ("icj-arrest-warrant-2002", "121", "2002-02-14", "judgment", "121-20020214-JUD-01-00-EN.pdf", "Arrest Warrant"),
    ("icj-wall-2004", "131", "2004-07-09", "advisory_opinion", "131-20040709-ADV-01-00-EN.pdf", "Wall Advisory Opinion"),
    ("icj-armed-activities-2005", "116", "2005-12-19", "judgment", "116-20051219-JUD-01-00-EN.pdf", "Armed Activities (DRC v. Uganda)"),
    ("icj-bosnian-genocide-2007", "91", "2007-02-26", "judgment", "091-20070226-JUD-01-00-EN.pdf", "Bosnian Genocide (Merits)"),
    ("icj-kosovo-2010", "141", "2010-07-22", "advisory_opinion", "141-20100722-ADV-01-00-EN.pdf", "Kosovo Advisory Opinion"),
    ("icj-jurisdictional-immunities-2012", "143", "2012-02-03", "judgment", "143-20120203-JUD-01-00-EN.pdf", "Jurisdictional Immunities"),
    ("icj-whaling-2014", "148", "2014-03-31", "judgment", "148-20140331-JUD-01-00-EN.pdf", "Whaling in the Antarctic"),
    ("icj-chagos-2019", "169", "2019-02-25", "advisory_opinion", "169-20190225-ADV-01-00-EN.pdf", "Chagos Advisory Opinion"),
    ("icj-gambia-myanmar-preliminary-2022", "178", "2022-07-22", "judgment", "178-20220722-JUD-01-00-EN.pdf", "The Gambia v. Myanmar (Preliminary Objections)"),
]


def case_spec(row: tuple[str, str, str, str, str, str]) -> dict:
    document_id, number, date, kind, filename, citation = row
    official_url = f"{ICJ_BASE}/{int(number)}/{filename}"
    case_number = "General List Nos. 51 and 52" if document_id == "icj-north-sea-1969" else f"General List No. {number}"
    return {
        "id": document_id,
        "short_title": citation,
        "document_type": kind,
        "issuing_body": "International Court of Justice",
        "adoption_date": date,
        "entry_into_force_date": None,
        "legal_status": "authoritative_advisory_opinion" if kind == "advisory_opinion" else "binding_between_parties",
        "official_source_url": official_url,
        # ICJ's production CDN challenges non-browser clients.  This is the
        # Court's public cloud content node serving the identical file path.
        "acquisition_url": official_url.replace("www.icj-cij.org", "icj-web.leman.un-icc.cloud"),
        "source_authority": "International Court of Justice",
        "parser": "case_auto",
        "expected_units": None,
        "case_number": case_number,
        "decision_kind": kind,
        "citation_prefix": citation,
    }


def download_pdf(url: str, target: Path) -> None:
    request = urllib.request.Request(
        url,
        headers={
            "User-Agent": "Mozilla/5.0 (compatible; ILIA-Corpus-Importer/1.0; research corpus)",
            "Accept": "application/pdf,*/*;q=0.8",
        },
    )
    with urllib.request.urlopen(request, timeout=120) as response:
        data = response.read()
    if not data.startswith(b"%PDF-"):
        raise RuntimeError(f"Official source did not return a PDF: {url} ({data[:32]!r})")
    target.parent.mkdir(parents=True, exist_ok=True)
    temporary = target.with_suffix(".download")
    temporary.write_bytes(data)
    temporary.replace(target)


def main() -> None:
    manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    original_ids = {document["id"] for document in manifest["documents"]}
    with INVENTORY_PATH.open("r", encoding="utf-8-sig", newline="") as stream:
        inventory = {row["document_id"]: row for row in csv.DictReader(stream)}

    specs = NORMATIVE + [case_spec(row) for row in CASE_ROWS]
    if len(specs) != 42:
        raise RuntimeError(f"Expected 42 additions, found {len(specs)}")
    missing = [spec["id"] for spec in specs if spec["id"] not in inventory]
    if missing:
        raise RuntimeError(f"Additions missing from frozen inventory: {missing}")

    cache: dict[str, Path] = {}
    additions: list[dict] = []
    for index, raw_spec in enumerate(specs, 1):
        spec = dict(raw_spec)
        row = inventory[spec["id"]]
        target = ROOT / "corpus" / "sources" / spec["id"] / "source.pdf"
        copy_from = spec.pop("copy_from", None)
        download_url = spec.get("acquisition_url", spec["official_source_url"])
        if target.exists() and target.read_bytes()[:5] == b"%PDF-":
            cache.setdefault(download_url, target)
        elif copy_from:
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / copy_from, target)
        elif download_url in cache:
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(cache[download_url], target)
        else:
            print(f"[{index:02d}/42] Downloading {spec['id']}", flush=True)
            download_pdf(download_url, target)
            cache[download_url] = target

        document = {
            "canonical_title": row["canonical_title"],
            "title_zh": row["title_zh"],
            **spec,
            "source_accessed_at": ACCESS_DATE,
            "local_file_path": target.relative_to(ROOT).as_posix(),
            "sha256": sha256_file(target),
            "mime_type": "application/pdf",
            "language": "en",
            "is_official_text": True,
            "translation_type": "authentic_text",
            "license_review": "pending",
        }
        additions.append(document)
        print(f"[{index:02d}/42] Acquired {spec['id']} ({target.stat().st_size:,} bytes)", flush=True)

    if original_ids.intersection(document["id"] for document in additions):
        raise RuntimeError("Addition overlaps the existing prototype manifest")
    manifest["database_version"] = "public-international-law-v1-2026.09.22"
    manifest["generated_at"] = ACCESS_DATE
    manifest["documents"].extend(additions)
    MANIFEST_PATH.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"Expanded manifest to {len(manifest['documents'])} documents")


if __name__ == "__main__":
    main()
