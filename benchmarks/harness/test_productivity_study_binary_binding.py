#!/usr/bin/env python3
"""Tests for inode-bound CodeLens daemon executable snapshots."""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_binary_snapshot as binary_snapshot
import productivity_study_provenance as provenance


def git(repo: Path, *args: str) -> str:
    completed = subprocess.run(
        ["git", *args], cwd=repo, check=True, capture_output=True, text=True
    )
    return completed.stdout.strip()


def make_provenance(root: Path) -> provenance.CodelensBinaryProvenance:
    repo = root / "repo"
    repo.mkdir()
    git(repo, "init", "--quiet")
    git(repo, "config", "user.email", "study@example.test")
    git(repo, "config", "user.name", "Study Test")
    (repo / "README.md").write_text("study\n", encoding="utf-8")
    git(repo, "add", "README.md")
    git(repo, "commit", "--quiet", "-m", "base")
    head_sha = git(repo, "rev-parse", "HEAD")
    binary = root / "codelens-mcp"
    binary.write_text(
        "#!/bin/sh\n"
        f"printf '%s\\n' 'codelens-mcp 1.13.34 (git {head_sha[:7]}, dirty false, built 2026-07-11T00:00:00Z)'\n",
        encoding="utf-8",
    )
    binary.chmod(0o755)
    return provenance.inspect_codelens_binary(
        provenance.CodelensBinaryRequest(repo, binary, root / "artifacts")
    )


def test_binding_rejects_snapshot_path_replaced_after_provenance() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-binding-") as raw_tmp:
        result = make_provenance(Path(raw_tmp))
        result.snapshot_path.unlink()
        result.snapshot_path.write_text("#!/bin/sh\nexit 99\n", encoding="utf-8")
        result.snapshot_path.chmod(0o500)

        try:
            with binary_snapshot.bound_binary_snapshot(
                result.snapshot_path, result.content_sha256
            ):
                raise AssertionError("replaced snapshot reached execution")
        except binary_snapshot.BinarySnapshotError as error:
            assert "hash" in str(error)
        else:
            raise AssertionError("accepted a replaced post-provenance snapshot")


def test_bound_inode_survives_snapshot_path_unlink_and_replacement() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-binding-") as raw_tmp:
        result = make_provenance(Path(raw_tmp))
        expected_bytes = result.snapshot_path.read_bytes()

        with binary_snapshot.bound_binary_snapshot(
            result.snapshot_path, result.content_sha256
        ) as bound:
            bound_identity = (bound.stat().st_dev, bound.stat().st_ino)
            result.snapshot_path.unlink()
            result.snapshot_path.write_text("replacement\n", encoding="utf-8")
            result.snapshot_path.chmod(0o500)

            assert bound.read_bytes() == expected_bytes
            assert (bound.stat().st_dev, bound.stat().st_ino) == bound_identity

        assert bound.exists() is False


def test_bound_executable_mutation_fails_context_postcheck() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-binding-") as raw_tmp:
        result = make_provenance(Path(raw_tmp))

        try:
            with binary_snapshot.bound_binary_snapshot(
                result.snapshot_path, result.content_sha256
            ) as bound:
                bound.chmod(0o700)
                bound.write_text("mutated\n", encoding="utf-8")
        except binary_snapshot.BinarySnapshotError as error:
            assert "mutated" in str(error)
        else:
            raise AssertionError("bound executable mutation escaped the postcheck")


def main() -> int:
    tests = (
        test_binding_rejects_snapshot_path_replaced_after_provenance,
        test_bound_inode_survives_snapshot_path_unlink_and_replacement,
        test_bound_executable_mutation_fails_context_postcheck,
    )
    for test in tests:
        test()
        print(f"PASS  {test.__name__}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
