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
                {"name": "local-git", "git_url": str(source), "revision": revision},
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


if __name__ == "__main__":
    unittest.main()
