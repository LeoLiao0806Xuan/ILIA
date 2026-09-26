import json
import unittest
from collections import Counter
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
EVAL = ROOT / "tests" / "eval" / "citation_support_1.1.jsonl"


class CitationSupportEvalTests(unittest.TestCase):
    def test_human_reference_set_is_balanced_and_well_formed(self):
        cases = [
            json.loads(line)
            for line in EVAL.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
        self.assertGreaterEqual(len(cases), 12)
        self.assertEqual(len({case["id"] for case in cases}), len(cases))
        self.assertEqual(
            Counter(case["expected_support"] for case in cases),
            Counter({"direct": 3, "summary": 3, "unsupported": 3, "conflict": 3}),
        )
        self.assertEqual({case["language"] for case in cases}, {"en", "zh"})
        for case in cases:
            self.assertTrue(case["statement"].strip())
            self.assertTrue(case["evidence"].strip())
            self.assertTrue(case["rationale"].strip())


if __name__ == "__main__":
    unittest.main()
