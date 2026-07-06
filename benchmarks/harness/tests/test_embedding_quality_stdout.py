import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPT = REPO_ROOT / "benchmarks" / "embedding-quality.py"


def write_single_row_dataset(path):
    path.write_text(
        json.dumps(
            [{"query": "target query", "query_type": "natural_language", "expected_symbol": "target_symbol"}]
        ),
        encoding="utf-8",
    )


def write_success_fake_binary(path):
    path.write_text(
        "#!/usr/bin/env python3\n"
        "import json, sys\n"
        "cmd = sys.argv[sys.argv.index('--cmd') + 1]\n"
        "payloads = {\n"
        "  'get_capabilities': {'success': True, 'data': {'embedding_model': 'fake'}},\n"
        "  'index_embeddings': {'success': True, 'data': {'indexed': True}},\n"
        "  'semantic_search': {'success': True, 'data': {'results': [{'symbol_name': 'target_symbol', 'file_path': 'target.rs'}]}},\n"
        "  'get_ranked_context': {'success': True, 'data': {'symbols': [{'name': 'target_symbol', 'file': 'target.rs'}], 'retrieval': {'cache_hit_tier': 'exact'}}},\n"
        "  'bm25_symbol_search': {'success': True, 'data': {'results': [{'name': 'target_symbol', 'file_path': 'target.rs'}]}},\n"
        "}\n"
        "print(json.dumps(payloads.get(cmd, {'success': False, 'error': cmd})))\n",
        encoding="utf-8",
    )
    path.chmod(0o755)


class EmbeddingQualityStdoutTests(unittest.TestCase):
    def test_embedding_quality_stdout_summary_avoids_full_json_payload(self):
        with tempfile.TemporaryDirectory() as tempdir:
            temp_path = Path(tempdir)
            dataset = temp_path / "dataset.json"
            output = temp_path / "results.json"
            fake_binary = temp_path / "codelens-fake"
            write_single_row_dataset(dataset)
            write_success_fake_binary(fake_binary)

            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    str(REPO_ROOT),
                    "--binary",
                    str(fake_binary),
                    "--dataset",
                    str(dataset),
                    "--output",
                    str(output),
                    "--stdout",
                    "summary",
                    "--tool-timeout",
                    "5",
                ],
                cwd=REPO_ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=10,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("Embedding-quality summary:", result.stdout)
            self.assertIn("dataset_size=1", result.stdout)
            self.assertNotIn('"methods"', result.stdout)
            self.assertLess(len(result.stdout), 1000)
            payload = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(payload["dataset_size"], 1)
            self.assertIn("methods", payload)


if __name__ == "__main__":
    unittest.main()
