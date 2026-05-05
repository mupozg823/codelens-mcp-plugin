import json
import subprocess
import sys
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
DATASET = REPO_ROOT / "benchmarks" / "embedding-quality-dataset-self.json"
ROLE_DATASET = REPO_ROOT / "benchmarks" / "role-retrieval-dataset.json"
SCRIPT = REPO_ROOT / "benchmarks" / "embedding-quality.py"


class EmbeddingQualityDatasetTests(unittest.TestCase):
    def test_self_dataset_expected_files_exist(self):
        rows = json.loads(DATASET.read_text(encoding="utf-8"))
        missing = [
            (index, row.get("expected_file_suffix"))
            for index, row in enumerate(rows, start=1)
            if row.get("expected_file_suffix")
            and not (REPO_ROOT / row["expected_file_suffix"]).exists()
        ]

        self.assertEqual(missing, [])

    def test_role_dataset_expected_files_exist(self):
        payload = json.loads(ROLE_DATASET.read_text(encoding="utf-8"))
        missing = [
            (index, row.get("expected_file_suffix"))
            for index, row in enumerate(payload["rows"], start=1)
            if row.get("expected_file_suffix")
            and not (REPO_ROOT / row["expected_file_suffix"]).exists()
        ]

        self.assertEqual(missing, [])

    def test_embedding_quality_exposes_ranked_context_token_budget(self):
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--help"],
            cwd=REPO_ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--ranked-context-max-tokens", result.stdout)

    def test_promotion_retrieval_scripts_expose_ranked_context_token_budget(self):
        for script_name in ("external-retrieval.py", "role-retrieval.py"):
            with self.subTest(script=script_name):
                result = subprocess.run(
                    [sys.executable, str(REPO_ROOT / "benchmarks" / script_name), "--help"],
                    cwd=REPO_ROOT,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    check=False,
                )

                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("--ranked-context-max-tokens", result.stdout)


if __name__ == "__main__":
    unittest.main()
