import importlib.util
import subprocess
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


EXTERNAL_SMOKE = load_script_module(
    "external_project_smoke_test",
    "external-project-smoke.py",
)


class ExternalProjectSmokeTests(unittest.TestCase):
    def test_expected_search_error_accepts_ranked_symbol(self):
        search_step = {
            "stdout": {
                "data": {
                    "results": [
                        {
                            "file_path": "registry.py",
                            "symbol_name": "parse_line",
                        }
                    ]
                }
            }
        }

        error = EXTERNAL_SMOKE.expected_search_error(
            search_step,
            {
                "file_path": "registry.py",
                "symbol_name": "parse_line",
                "max_rank": 1,
            },
        )

        self.assertIsNone(error)

    def test_expected_search_error_reports_missing_symbol(self):
        search_step = {
            "stdout": {
                "data": {
                    "results": [
                        {
                            "file_path": "registry.py",
                            "symbol_name": "on_event",
                        }
                    ]
                }
            }
        }

        error = EXTERNAL_SMOKE.expected_search_error(
            search_step,
            {
                "file_path": "registry.py",
                "symbol_name": "parse_line",
                "max_rank": 1,
            },
        )

        self.assertIn("parse_line", error)

    def test_degraded_search_error_reports_a_failed_lane(self):
        # next-learn passed its top-3 check for months while every run said
        # `lexical_lane: failed` (an FTS5 syntax error on "next.js").
        search_step = {
            "stdout": {
                "data": {
                    "results": [{"file_path": "login-form.tsx", "symbol_name": "LoginForm"}],
                    "retrieval": {"semantic_lane": "ok", "lexical_lane": "failed"},
                    "degraded_reason": "lexical lane failed; results are embedding-only: fts5: syntax error",
                }
            }
        }

        error = EXTERNAL_SMOKE.degraded_search_error(search_step)

        self.assertIn("lexical_lane", error)
        self.assertIn("fts5: syntax error", error)

    def test_degraded_search_error_accepts_healthy_lanes(self):
        search_step = {
            "stdout": {
                "data": {
                    "results": [],
                    "retrieval": {"semantic_lane": "ok", "lexical_lane": "ok"},
                }
            }
        }

        self.assertIsNone(EXTERNAL_SMOKE.degraded_search_error(search_step))

    def _search_step(self, symbols):
        return {
            "ok": True,
            "stdout": {
                "data": {
                    "results": [{"file_path": "src/ser.rs", "symbol_name": name} for name in symbols],
                    "retrieval": {"semantic_lane": "ok", "lexical_lane": "ok"},
                }
            },
        }

    def test_known_gap_miss_is_reported_without_failing(self):
        expected = {
            "file_path": "src/ser.rs",
            "symbol_name": "to_string",
            "max_rank": 3,
            "known_gap": {"reason": "embedding model misses this NL query"},
        }

        step = EXTERNAL_SMOKE.judge_search_step(self._search_step(["to_writer"]), expected)

        self.assertTrue(step["ok"])
        self.assertIn("to_string", step["known_gap_miss"])
        self.assertEqual(step["known_gap"], "embedding model misses this NL query")

    def test_known_gap_that_now_passes_fails_until_removed(self):
        expected = {
            "file_path": "src/ser.rs",
            "symbol_name": "to_string",
            "max_rank": 3,
            "known_gap": {"reason": "embedding model misses this NL query"},
        }

        step = EXTERNAL_SMOKE.judge_search_step(self._search_step(["to_string"]), expected)

        self.assertFalse(step["ok"])
        self.assertIn("remove known_gap", step["quality_error"])

    def test_known_gap_needs_a_reason(self):
        expected = {"symbol_name": "to_string", "known_gap": {}}

        step = EXTERNAL_SMOKE.judge_search_step(self._search_step(["to_writer"]), expected)

        self.assertFalse(step["ok"])
        self.assertIn("reason", step["quality_error"])

    def test_known_gap_does_not_cover_a_degraded_lane(self):
        expected = {
            "symbol_name": "to_string",
            "known_gap": {"reason": "embedding model misses this NL query"},
        }
        search_step = self._search_step(["to_writer"])
        search_step["stdout"]["data"]["retrieval"]["lexical_lane"] = "failed"

        step = EXTERNAL_SMOKE.judge_search_step(search_step, expected)

        self.assertFalse(step["ok"])
        self.assertIn("lexical_lane", step["degraded_error"])

    def test_check_mode_requires_expected_search_labels(self):
        missing = EXTERNAL_SMOKE.projects_missing_expected_search(
            [
                {"name": "labeled", "expected_search": {"symbol_name": "parse_line"}},
                {"name": "unlabeled"},
            ]
        )

        self.assertEqual(missing, ["unlabeled"])

    def test_materialize_project_clones_pinned_git_revision(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            source = Path(tmpdir) / "source"
            source.mkdir()
            subprocess.run(["git", "init", "--quiet"], cwd=source, check=True)
            (source / "app.py").write_text("def health():\n    return 'ok'\n")
            subprocess.run(["git", "add", "app.py"], cwd=source, check=True)
            subprocess.run(
                [
                    "git",
                    "-c",
                    "user.email=test@example.com",
                    "-c",
                    "user.name=Test User",
                    "commit",
                    "--quiet",
                    "-m",
                    "init",
                ],
                cwd=source,
                check=True,
            )
            revision = subprocess.check_output(
                ["git", "rev-parse", "HEAD"],
                cwd=source,
                text=True,
            ).strip()

            project, cleanup = EXTERNAL_SMOKE.materialize_project(
                {
                    "name": "local-git",
                    "git_url": str(source),
                    "revision": revision,
                    "clone_depth": 0,
                },
                keep=False,
                timeout=30,
            )
            try:
                self.assertTrue((project / "app.py").is_file())
                checked_out = subprocess.check_output(
                    ["git", "rev-parse", "HEAD"],
                    cwd=project,
                    text=True,
                ).strip()
                self.assertEqual(checked_out, revision)
            finally:
                cleanup.cleanup()

    def test_materialize_project_fetches_revision_missing_from_shallow_clone(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            source = Path(tmpdir) / "source"
            source.mkdir()
            subprocess.run(["git", "init", "--quiet"], cwd=source, check=True)
            (source / "app.py").write_text("def health():\n    return 'ok'\n")
            subprocess.run(["git", "add", "app.py"], cwd=source, check=True)
            subprocess.run(
                [
                    "git",
                    "-c",
                    "user.email=test@example.com",
                    "-c",
                    "user.name=Test User",
                    "commit",
                    "--quiet",
                    "-m",
                    "init",
                ],
                cwd=source,
                check=True,
            )
            first_revision = subprocess.check_output(
                ["git", "rev-parse", "HEAD"],
                cwd=source,
                text=True,
            ).strip()
            (source / "app.py").write_text("def health():\n    return 'ready'\n")
            subprocess.run(["git", "add", "app.py"], cwd=source, check=True)
            subprocess.run(
                [
                    "git",
                    "-c",
                    "user.email=test@example.com",
                    "-c",
                    "user.name=Test User",
                    "commit",
                    "--quiet",
                    "-m",
                    "update",
                ],
                cwd=source,
                check=True,
            )

            project, cleanup = EXTERNAL_SMOKE.materialize_project(
                {
                    "name": "local-git",
                    "git_url": source.as_uri(),
                    "revision": first_revision,
                    "clone_depth": 1,
                },
                keep=False,
                timeout=30,
            )
            try:
                checked_out = subprocess.check_output(
                    ["git", "rev-parse", "HEAD"],
                    cwd=project,
                    text=True,
                ).strip()
                self.assertEqual(checked_out, first_revision)
                self.assertIn("return 'ok'", (project / "app.py").read_text())
            finally:
                cleanup.cleanup()


if __name__ == "__main__":
    unittest.main()
