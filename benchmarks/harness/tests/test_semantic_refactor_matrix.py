import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


BENCHMARKS_DIR = Path(__file__).resolve().parents[2]


def load_script_module(module_name: str, filename: str):
    path = BENCHMARKS_DIR / filename
    added_path = False
    if str(BENCHMARKS_DIR) not in sys.path:
        sys.path.insert(0, str(BENCHMARKS_DIR))
        added_path = True
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    try:
        spec.loader.exec_module(module)
    finally:
        if added_path:
            sys.path.remove(str(BENCHMARKS_DIR))
    return module


SEMANTIC_REFACTOR = load_script_module(
    "semantic_refactor_matrix_test",
    "semantic-refactor-matrix.py",
)


class SemanticRefactorMatrixTests(unittest.TestCase):
    def test_load_matrix_accepts_valid_refactor_operations(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            fixture = Path(tmpdir) / "fixture"
            fixture.mkdir()
            matrix = Path(tmpdir) / "matrix.json"
            matrix.write_text(
                json.dumps(
                    {
                        "projects": [
                            {
                                "name": "fixture",
                                "path": str(fixture),
                                "operations": [
                                    {
                                        "tool": "refactor_extract_function",
                                        "args": {
                                            "file_path": "src/main.ts",
                                            "start_line": 1,
                                            "end_line": 1,
                                            "new_name": "extracted",
                                            "semantic_edit_backend": "lsp",
                                            "dry_run": True,
                                        },
                                        "expect": {"success": True},
                                    }
                                ],
                            }
                        ]
                    }
                )
            )

            projects = SEMANTIC_REFACTOR.load_matrix(matrix)

            self.assertEqual(projects[0]["name"], "fixture")

    def test_load_matrix_rejects_unsupported_tool(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            fixture = Path(tmpdir) / "fixture"
            fixture.mkdir()
            matrix = Path(tmpdir) / "matrix.json"
            matrix.write_text(
                json.dumps(
                    {
                        "projects": [
                            {
                                "name": "fixture",
                                "path": str(fixture),
                                "operations": [{"tool": "read_file", "args": {}}],
                            }
                        ]
                    }
                )
            )

            with self.assertRaises(ValueError):
                SEMANTIC_REFACTOR.load_matrix(matrix)

    def test_expected_payload_checks_nested_fields(self):
        payload = {
            "success": True,
            "data": {"operation": "extract_function", "targets": [{"file_path": "a.rs"}]},
        }

        self.assertTrue(
            SEMANTIC_REFACTOR.expected_payload(
                payload,
                {
                    "success": True,
                    "equals": {"data.operation": "extract_function"},
                    "present": ["data.targets.0.file_path"],
                },
            )
        )


if __name__ == "__main__":
    unittest.main()
