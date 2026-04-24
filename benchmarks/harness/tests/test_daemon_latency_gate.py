import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


HARNESS_DIR = Path(__file__).resolve().parents[1]
BENCH_DIR = HARNESS_DIR.parent
if str(BENCH_DIR) not in sys.path:
    sys.path.insert(0, str(BENCH_DIR))


def load_script_module(module_name: str, path: Path):
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


DAEMON = load_script_module(
    "daemon_latency_gate_test",
    BENCH_DIR / "daemon-latency-gate.py",
)


class DaemonLatencyGateTests(unittest.TestCase):
    def test_load_distinct_queries_dedupes_dataset_order(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            dataset = Path(tmpdir) / "queries.json"
            dataset.write_text(
                '[{"query":"alpha"},{"query":"beta"},{"query":"alpha"},{"query":""}]',
                encoding="utf-8",
            )

            queries = DAEMON.load_distinct_queries(dataset, 10)

        self.assertEqual(queries, ["alpha", "beta"])

    def test_render_markdown_reports_hot_cold_and_prewarmed_sections(self):
        result = {
            "project": "/tmp/project",
            "binary": "/tmp/codelens-mcp",
            "model_dir": "/tmp/models",
            "runtime": {
                "embedding_runtime_backend": "coreml",
                "embedding_runtime_preference": "coreml_preferred",
            },
            "query_cache": {"enabled": True, "entries": 3, "max_entries": 4096, "prewarmed": 2},
            "tools": {
                "semantic_search": {"p50_ms": 10, "p95_ms": 20, "max_ms": 30, "bytes": 100},
            },
            "cold_distinct": {
                "semantic_search": {"p50_ms": 300, "p95_ms": 600, "max_ms": 700, "bytes": 100},
            },
            "prewarmed_distinct": {
                "semantic_search": {"p50_ms": 40, "p95_ms": 80, "max_ms": 100, "bytes": 100},
            },
            "gate": {"passed": True, "failures": []},
        }

        markdown = DAEMON.render_markdown(result)

        self.assertIn("Hot Path", markdown)
        self.assertIn("Cold Distinct", markdown)
        self.assertIn("Prewarmed Distinct", markdown)
        self.assertIn("Query cache", markdown)


if __name__ == "__main__":
    unittest.main()
