import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
DATASET = REPO_ROOT / "benchmarks" / "embedding-quality-dataset-self.json"
ROLE_DATASET = REPO_ROOT / "benchmarks" / "role-retrieval-dataset.json"
SCRIPT = REPO_ROOT / "benchmarks" / "embedding-quality.py"


def write_single_row_dataset(path, query="target query", expected_symbol="target_symbol"):
    path.write_text(
        json.dumps(
            [{"query": query, "query_type": "natural_language", "expected_symbol": expected_symbol}]
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

    def test_embedding_quality_exposes_candidate_missing_gate(self):
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--help"],
            cwd=REPO_ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--max-hybrid-candidate-missing-rate", result.stdout)
        self.assertIn("--triage-output", result.stdout)
        self.assertIn("--stdout", result.stdout)

    def test_embedding_quality_reports_tool_timeout(self):
        with tempfile.TemporaryDirectory() as tempdir:
            fake_binary = Path(tempdir) / "codelens-fake"
            fake_binary.write_text(
                "#!/usr/bin/env python3\n"
                "import time\n"
                "time.sleep(5)\n",
                encoding="utf-8",
            )
            fake_binary.chmod(0o755)
            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    str(REPO_ROOT),
                    "--binary",
                    str(fake_binary),
                    "--tool-timeout",
                    "1",
                ],
                cwd=REPO_ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=10,
                check=False,
            )

        self.assertNotEqual(result.returncode, 0)
        combined = f"{result.stdout}\n{result.stderr}"
        self.assertIn("get_capabilities failed", combined)
        self.assertIn("tool_timeout", combined)
        self.assertIn("timeout_seconds", combined)

    def test_embedding_quality_applies_timeout_to_query_tools(self):
        with tempfile.TemporaryDirectory() as tempdir:
            temp_path = Path(tempdir)
            dataset = temp_path / "dataset.json"
            dataset.write_text(
                json.dumps(
                    [
                        {
                            "query": "slow semantic query",
                            "query_type": "natural_language",
                            "expected_symbol": "slow_symbol",
                        }
                    ]
                ),
                encoding="utf-8",
            )
            fake_binary = temp_path / "codelens-fake"
            fake_binary.write_text(
                "#!/usr/bin/env python3\n"
                "import json\n"
                "import subprocess\n"
                "import sys\n"
                "import time\n"
                "cmd = sys.argv[sys.argv.index('--cmd') + 1]\n"
                "if cmd == 'get_capabilities':\n"
                "    print(json.dumps({'success': True, 'data': {'embedding_model': 'fake'}}))\n"
                "elif cmd == 'index_embeddings':\n"
                "    print(json.dumps({'success': True, 'data': {'indexed': True}}))\n"
                "else:\n"
                "    subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])\n"
                "    time.sleep(30)\n",
                encoding="utf-8",
            )
            fake_binary.chmod(0o755)
            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    str(REPO_ROOT),
                    "--binary",
                    str(fake_binary),
                    "--dataset",
                    str(dataset),
                    "--tool-timeout",
                    "1",
                ],
                cwd=REPO_ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=4,
                check=False,
            )

        self.assertNotEqual(result.returncode, 0)
        combined = f"{result.stdout}\n{result.stderr}"
        self.assertIn("semantic_search failed", combined)
        self.assertIn("context=slow semantic query", combined)
        self.assertIn("tool_timeout", combined)
        self.assertIn("timeout_seconds", combined)

    def test_embedding_quality_reports_p95_tokens_and_query_cache_probe(self):
        with tempfile.TemporaryDirectory() as tempdir:
            temp_path = Path(tempdir)
            dataset = temp_path / "dataset.json"
            output = temp_path / "results.json"
            triage_output = temp_path / "triage.json"
            write_single_row_dataset(dataset)
            fake_binary = temp_path / "codelens-fake"
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
                    "--triage-output",
                    str(triage_output),
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
            payload = json.loads(output.read_text(encoding="utf-8"))
            hybrid = next(
                method
                for method in payload["methods"]
                if method["method"] == "get_ranked_context"
            )
            self.assertIn("p95_estimated_response_tokens", hybrid)
            self.assertIn(
                "p95_estimated_response_tokens",
                hybrid["by_query_type"]["natural_language"],
            )
            self.assertEqual(
                payload["query_cache_probe"]["second_cache_hit_tier"],
                "exact",
            )
            self.assertTrue(payload["query_cache_probe"]["cache_hit_observed"])
            triage = json.loads(triage_output.read_text(encoding="utf-8"))
            self.assertEqual(triage["schema_version"], 1)
            self.assertEqual(triage["dataset_size"], 1)
            self.assertEqual(triage["candidate_missing"]["count"], 0)
            self.assertEqual(
                triage["semantic_hit_dropped_by_hybrid"]["count"],
                0,
            )
            self.assertEqual(triage["hybrid_demoted_semantic_hit"]["count"], 0)
            self.assertIn("p95_response_tokens", triage["token_budget"])
            self.assertTrue(triage["query_cache_probe"]["cache_hit_observed"])

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
