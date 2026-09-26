from __future__ import annotations

import csv
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

from release_rights_gate import GateError, validate_release  # noqa: E402


class ReleaseRightsGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.temp = Path(self.temporary.name)
        self.manifest = self.temp / "manifest.json"
        self.matrix = self.temp / "matrix.csv"
        self.components = self.temp / "components.json"
        shutil.copyfile(ROOT / "corpus/normalized/manifest.json", self.manifest)
        shutil.copyfile(
            ROOT / "licenses/corpus-redistribution-rights-matrix.csv", self.matrix
        )
        shutil.copyfile(ROOT / "licenses/component-clearance.json", self.components)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def validate(self, stage_root: Path | None = None) -> dict[str, object]:
        return validate_release(
            root=ROOT,
            manifest_path=self.manifest,
            matrix_path=self.matrix,
            components_path=self.components,
            database_path=ROOT / "data/ilia.sqlite3",
            stage_root=stage_root,
        )

    def rewrite_matrix(self, mutate) -> None:
        with self.matrix.open("r", encoding="utf-8-sig", newline="") as stream:
            rows = list(csv.DictReader(stream))
            fields = list(rows[0])
        mutate(rows[0])
        with self.matrix.open("w", encoding="utf-8", newline="") as stream:
            writer = csv.DictWriter(stream, fieldnames=fields, lineterminator="\n")
            writer.writeheader()
            writer.writerows(rows)

    def test_current_release_passes(self) -> None:
        report = self.validate()
        self.assertEqual(report["packaged_normalized_documents"], 49)
        self.assertEqual(report["source_pdfs_packaged"], 0)

    def test_red_packaged_text_is_rejected(self) -> None:
        self.rewrite_matrix(
            lambda row: row.__setitem__("normalized_text_rating", "red")
        )
        with self.assertRaisesRegex(GateError, "packaged normalized text is red"):
            self.validate()

    def test_yellow_without_approved_treatment_is_rejected(self) -> None:
        self.rewrite_matrix(
            lambda row: row.__setitem__(
                "v1_0_1_packaging_decision", "package_source_pdf"
            )
        )
        with self.assertRaisesRegex(GateError, "approved no-PDF treatment"):
            self.validate()

    def test_pending_component_is_rejected(self) -> None:
        content = json.loads(self.components.read_text(encoding="utf-8"))
        first = next(iter(content["components"].values()))
        first["public_distribution"] = "pending"
        self.components.write_text(json.dumps(content), encoding="utf-8")
        with self.assertRaisesRegex(GateError, "pending"):
            self.validate()

    def test_pdf_in_stage_is_rejected(self) -> None:
        stage = self.temp / "stage"
        stage.mkdir()
        (stage / "source.PDF").write_bytes(b"not a real PDF")
        with self.assertRaisesRegex(GateError, "source PDFs found"):
            self.validate(stage)


if __name__ == "__main__":
    unittest.main()
