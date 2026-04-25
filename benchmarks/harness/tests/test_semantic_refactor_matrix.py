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

    def test_expected_payload_does_not_count_unsupported_as_success(self):
        payload = {
            "success": False,
            "data": {
                "status": "unsupported",
                "support": "unsupported",
                "blocker_reason": "no authoritative backend",
            },
        }

        self.assertFalse(
            SEMANTIC_REFACTOR.expected_payload(
                payload,
                {
                    "success": True,
                    "equals": {"data.support": "authoritative_apply"},
                },
            )
        )
        self.assertTrue(
            SEMANTIC_REFACTOR.expected_payload(
                payload,
                {
                    "success": False,
                    "unsupported": True,
                    "equals": {"data.support": "unsupported"},
                },
            )
        )
        self.assertFalse(
            SEMANTIC_REFACTOR.expected_payload(
                {
                    "success": True,
                    "data": {
                        "status": "unsupported",
                        "support": "unsupported",
                        "blocker_reason": "no authoritative backend",
                    },
                },
                {"success": True},
            )
        )

    def test_missing_required_command_marks_project_skipped(self):
        item = {
            "name": "java",
            "kind": "java",
            "path": ".",
            "required_command": "definitely-not-a-codelens-test-command",
            "operations": [
                {"tool": "resolve_symbol_target", "args": {"file_path": "x.java"}}
            ],
        }

        result = SEMANTIC_REFACTOR.run_project(
            item,
            Path("/bin/false"),
            timeout=1,
            keep_workdirs=False,
            env={},
            default_preset="full",
            default_profile=None,
        )

        self.assertTrue(result["ok"])
        self.assertTrue(result["skipped"])
        self.assertIn("missing command", result["skip_reason"])

    def test_run_tool_adds_preset_and_profile(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            binary = Path(tmpdir) / "fake-codelens"
            capture = Path(tmpdir) / "argv.json"
            binary.write_text(
                "#!/usr/bin/env python3\n"
                "import json, os, sys\n"
                f"open({str(capture)!r}, 'w').write(json.dumps(sys.argv[1:]))\n"
                "print(json.dumps({'success': True}))\n"
            )
            binary.chmod(0o755)

            result = SEMANTIC_REFACTOR.run_tool(
                binary,
                Path(tmpdir),
                {"tool": "refactor_extract_function", "args": {}, "expect": {"success": True}},
                timeout=5,
                env={},
                default_preset="full",
                default_profile="refactor-full",
            )

            argv = json.loads(capture.read_text())
            self.assertTrue(result["ok"])
            self.assertIn("--preset", argv)
            self.assertIn("full", argv)
            self.assertIn("--profile", argv)
            self.assertIn("refactor-full", argv)

    def test_run_tool_merges_operation_env(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            binary = Path(tmpdir) / "fake-codelens"
            capture = Path(tmpdir) / "env.json"
            binary.write_text(
                "#!/usr/bin/env python3\n"
                "import json, os\n"
                f"open({str(capture)!r}, 'w').write(json.dumps({{'adapter': os.environ.get('CODELENS_ROSLYN_ADAPTER_CMD')}}))\n"
                "print(json.dumps({'success': True}))\n"
            )
            binary.chmod(0o755)

            result = SEMANTIC_REFACTOR.run_tool(
                binary,
                Path(tmpdir),
                {
                    "tool": "rename_symbol",
                    "args": {},
                    "env": {"CODELENS_ROSLYN_ADAPTER_CMD": "dotnet"},
                    "expect": {"success": True},
                },
                timeout=5,
                env={},
                default_preset="full",
                default_profile=None,
            )

            observed = json.loads(capture.read_text())
            self.assertTrue(result["ok"])
            self.assertEqual(observed["adapter"], "dotnet")

    def test_run_tool_retries_transient_lsp_content_modified(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            binary = Path(tmpdir) / "fake-codelens"
            attempts = Path(tmpdir) / "attempts.txt"
            binary.write_text(
                "#!/usr/bin/env python3\n"
                "import json, pathlib, sys\n"
                f"attempts = pathlib.Path({str(attempts)!r})\n"
                "count = int(attempts.read_text()) if attempts.exists() else 0\n"
                "attempts.write_text(str(count + 1))\n"
                "if count == 0:\n"
                "    print(json.dumps({'success': False, 'error': 'LSP error: content modified'}))\n"
                "else:\n"
                "    print(json.dumps({'success': True}))\n"
            )
            binary.chmod(0o755)

            result = SEMANTIC_REFACTOR.run_tool(
                binary,
                Path(tmpdir),
                {"tool": "resolve_symbol_target", "args": {}, "expect": {"success": True}},
                timeout=5,
                env={},
                default_preset="full",
                default_profile=None,
            )

            self.assertTrue(result["ok"])
            self.assertEqual(result["attempts"], 2)


if __name__ == "__main__":
    unittest.main()
