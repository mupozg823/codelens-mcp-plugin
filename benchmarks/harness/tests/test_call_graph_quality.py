import importlib.util
import sys
import unittest
from pathlib import Path


BENCHMARKS_DIR = Path(__file__).resolve().parents[2]
SCRIPT_PATH = BENCHMARKS_DIR / "call-graph-quality.py"


def load_script_module():
    added_path = False
    if str(BENCHMARKS_DIR) not in sys.path:
        sys.path.insert(0, str(BENCHMARKS_DIR))
        added_path = True
    spec = importlib.util.spec_from_file_location("call_graph_quality_test", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    try:
        spec.loader.exec_module(module)
    finally:
        if added_path:
            sys.path.remove(str(BENCHMARKS_DIR))
    return module


CALL_GRAPH = load_script_module()


class CallGraphQualityTests(unittest.TestCase):
    def test_matches_edge_uses_name_and_file_suffix(self):
        edge = {
            "function": "handle_request",
            "file": "crates/codelens-mcp/src/server/router.rs",
            "confidence": 0.75,
        }

        self.assertTrue(
            CALL_GRAPH.matches_edge(
                {
                    "name": "handle_request",
                    "file_suffix": "src/server/router.rs",
                },
                edge,
            )
        )
        self.assertFalse(
            CALL_GRAPH.matches_edge(
                {
                    "name": "handle_request",
                    "file_suffix": "src/server/other.rs",
                },
                edge,
            )
        )

    def test_evaluate_row_counts_expected_and_forbidden_failures(self):
        row = {
            "id": "tsx-register-page",
            "repo_id": "app",
            "tool": "get_callees",
            "function_name": "RegisterPage",
            "expected_edges": [
                {
                    "name": "safeEdge",
                    "file_suffix": "src/safe.ts",
                    "min_confidence": 0.4,
                }
            ],
            "forbidden_high_confidence_edges": [
                {
                    "name": "handleSubmit",
                    "resolved_file_contains": "CommentSection",
                    "min_confidence": 0.61,
                }
            ],
        }
        result = {
            "returncode": 0,
            "elapsed_ms": 12.0,
            "stderr": "",
            "payload": {
                "success": True,
                "data": {
                    "callees": [
                        {
                            "name": "safeEdge",
                            "resolved_file": "src/safe.ts",
                            "confidence": 0.7,
                            "resolution": "unique_name",
                        },
                        {
                            "name": "handleSubmit",
                            "resolved_file": "src/components/CommentSection.tsx",
                            "confidence": 0.75,
                            "resolution": "unique_name",
                        },
                    ]
                },
            },
        }

        scored = CALL_GRAPH.evaluate_row(row, result)

        self.assertEqual(scored["expected_found_count"], 1)
        self.assertEqual(scored["edge_recall_at_k"], 1.0)
        self.assertEqual(len(scored["forbidden_high_confidence_failures"]), 1)
        self.assertEqual(scored["status"], "failed")

    def test_confidence_honesty_flags_unresolved_high_confidence(self):
        failures = CALL_GRAPH.confidence_honesty_failures(
            [
                {"name": "unknown", "confidence": 0.4, "resolution": "unresolved"},
                {"name": "nearby", "confidence": 0.5, "resolution": "path_proximity"},
            ]
        )

        self.assertEqual(len(failures), 1)
        self.assertIn("unresolved", failures[0]["reason"])


if __name__ == "__main__":
    unittest.main()
