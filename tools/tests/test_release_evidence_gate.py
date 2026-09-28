import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "aggregate_release_evidence", ROOT / "tools" / "aggregate_release_evidence.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC and SPEC.loader
SPEC.loader.exec_module(MODULE)


class ReleaseEvidenceGateTests(unittest.TestCase):
    def write_report(self, name: str, value: dict) -> Path:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        path = Path(temporary.name) / name
        path.write_text(json.dumps(value), encoding="utf-8")
        return path

    def test_rejects_citation_report_below_quality_threshold(self):
        path = self.write_report(
            "citation_eval_report_1.1.4.json",
            {
                "release_version": "1.1.4",
                "passed": False,
                "accuracy": 0.75,
                "safety_boundary_accuracy": 1.0,
                "unsafe_false_accepts": 0,
            },
        )
        errors = MODULE.validate_report(path.name, path, "1.1.4")
        self.assertTrue(any("citation gate" in error for error in errors))
        self.assertTrue(any("below 0.85" in error for error in errors))

    def test_requires_complete_ordered_update_lifecycle(self):
        path = self.write_report(
            "update-progress-e2e.json",
            {
                "release_version": "1.1.4",
                "status": "passed",
                "phase": "applied",
                "phases": ["checking", "applying", "applied"],
            },
        )
        errors = MODULE.validate_report(path.name, path, "1.1.4")
        self.assertIn(
            "update-progress-e2e.json lacks the ordered lifecycle phases", errors
        )


if __name__ == "__main__":
    unittest.main()
