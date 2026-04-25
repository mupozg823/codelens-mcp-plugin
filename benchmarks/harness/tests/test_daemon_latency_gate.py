import importlib.util
import json
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


DAEMON_GATE = load_script_module(
    "daemon_latency_gate_test",
    BENCH_DIR / "daemon-latency-gate.py",
)


class DaemonLatencyGateTests(unittest.TestCase):
    def test_load_distinct_queries_accepts_dataset_rows_and_dedupes(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            dataset = Path(tmpdir) / "queries.json"
            dataset.write_text(
                json.dumps(
                    [
                        {"query": "find rename implementation"},
                        {"query": "find rename implementation"},
                        {"query": "semantic search hot path"},
                    ]
                )
            )

            queries = DAEMON_GATE.load_distinct_queries(str(dataset), "fallback", 3)

        self.assertEqual(
            queries,
            [
                "find rename implementation",
                "semantic search hot path",
                "fallback distinct 3",
            ],
        )

    def test_load_distinct_queries_accepts_plain_string_rows(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            dataset = Path(tmpdir) / "queries.json"
            dataset.write_text(json.dumps(["first", "second"]))

            queries = DAEMON_GATE.load_distinct_queries(str(dataset), "fallback", 2)

        self.assertEqual(queries, ["first", "second"])


if __name__ == "__main__":
    unittest.main()
