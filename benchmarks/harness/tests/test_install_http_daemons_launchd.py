import os
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPT = REPO_ROOT / "scripts" / "install-http-daemons-launchd.sh"


class InstallHttpDaemonsLaunchdTests(unittest.TestCase):
    def test_script_syntax(self):
        result = subprocess.run(
            ["bash", "-n", str(SCRIPT)],
            cwd=REPO_ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_help_documents_semantic_controls(self):
        result = subprocess.run(
            ["bash", str(SCRIPT), "--help"],
            cwd=REPO_ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--model-dir DIR", result.stdout)
        self.assertIn("--no-semantic", result.stdout)

    def test_print_only_plist_includes_semantic_model_dir_by_default(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            repo = root / "repo"
            (repo / "crates" / "codelens-mcp").mkdir(parents=True)
            (repo / "Cargo.toml").write_text("[workspace]\n")
            (repo / "crates" / "codelens-mcp" / "Cargo.toml").write_text("[package]\n")
            bin_path = root / "bin" / "codelens-mcp-http"
            bin_path.parent.mkdir()
            bin_path.write_text("#!/usr/bin/env bash\n")
            bin_path.chmod(bin_path.stat().st_mode | 0o111)
            model_dir = root / "models"
            model_dir.mkdir()

            result = subprocess.run(
                [
                    "bash",
                    str(SCRIPT),
                    str(repo),
                    "--no-build",
                    "--print-only",
                    "--bin-path",
                    str(bin_path),
                    "--launch-agents-dir",
                    str(root / "agents"),
                    "--model-dir",
                    str(model_dir),
                ],
                cwd=REPO_ROOT,
                env={**os.environ, "HOME": str(root)},
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("<key>CODELENS_MODEL_DIR</key>", result.stdout)
            self.assertIn(str(model_dir), result.stdout)


if __name__ == "__main__":
    unittest.main()
