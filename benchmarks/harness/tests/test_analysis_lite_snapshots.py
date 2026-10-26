"""Controlled local Git fixtures; no original project writes."""

import hashlib
import importlib
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))


class SnapshotTests(unittest.TestCase):
    def test_archive_isolation_exclusions_and_frozen_hashes(self):
        module = importlib.import_module("analysis_lite_snapshots")
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); repo = root / "original"; repo.mkdir()
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            files = {"lib/source.ts": "export const value = 1;\n", ".env": "fixture-only",
                     "docs/answers.md": "prior answer", "gold.json": "grader fixture",
                     "package.json": "{}", "public/large.zip": "binary fixture",
                     "AGENTS.md": "applicable source instructions", "lib/CLAUDE.md": "nested instructions",
                     ".gitignore": "dist/\n", ".ignore": "generated/\n", "lib/.codelensignore": "cache/\n",
                     "lib/module.mts": "export const esm = 1;", "lib/module.cts": "export const cjs = 1;"}
            for name, content in files.items():
                p = repo / name; p.parent.mkdir(parents=True, exist_ok=True); p.write_text(content)
            (repo / "lib/link.ts").symlink_to(root / "outside-gold")
            subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
            subprocess.run(["git", "-C", str(repo), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.test",
                            "commit", "-qm", "fixture"], check=True)
            sha = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
            spec = {"repo": str(repo), "base_sha": sha,
                    "tree_sha": subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD^{tree}"], text=True).strip(),
                    "file_sha256": {"lib/source.ts": hashlib.sha256(files["lib/source.ts"].encode()).hexdigest()}}
            (repo / "lib/source.ts").write_text("uncommitted WIP")
            (repo / "lib/untracked.ts").write_text("untracked WIP")
            receipt = module.prepare_snapshot(spec, root / "run")
            self.assertEqual((root / "run/lib/source.ts").read_text(), files["lib/source.ts"])
            self.assertFalse((root / "run/.git").exists())
            self.assertEqual((root / "run/AGENTS.md").read_text(), "applicable source instructions")
            self.assertEqual((root / "run/lib/CLAUDE.md").read_text(), "nested instructions")
            for name in (".gitignore", ".ignore", "lib/.codelensignore", "lib/module.mts", "lib/module.cts"):
                self.assertEqual((root / "run" / name).read_text(), files[name])
            for forbidden in (".env", "docs/answers.md", "gold.json", "lib/link.ts", "lib/untracked.ts", "public/large.zip"):
                self.assertFalse((root / "run" / forbidden).exists(), forbidden)
            self.assertEqual((repo / "lib/source.ts").read_text(), "uncommitted WIP")
            self.assertIn("lib/link.ts", [row["path"] for row in receipt["excluded"]])
            with self.assertRaises(FileExistsError):
                module.prepare_snapshot(spec, root / "run")
            spec["file_sha256"]["lib/source.ts"] = "0" * 64
            with self.assertRaises(ValueError):
                module.prepare_snapshot(spec, root / "drift")


if __name__ == "__main__":
    unittest.main()
