#!/usr/bin/env python3
"""Tests for fail-closed CodeLens binary provenance."""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_provenance as provenance


def git(repo: Path, *args: str) -> str:
    completed = subprocess.run(
        ["git", *args], cwd=repo, check=True, capture_output=True, text=True
    )
    return completed.stdout.strip()


def make_repo(root: Path) -> tuple[Path, str]:
    repo = root / "repo"
    repo.mkdir()
    git(repo, "init", "--quiet")
    git(repo, "config", "user.email", "study@example.test")
    git(repo, "config", "user.name", "Study Test")
    (repo / "README.md").write_text("study\n", encoding="utf-8")
    git(repo, "add", "README.md")
    git(repo, "commit", "--quiet", "-m", "base")
    return repo, git(repo, "rev-parse", "HEAD")


def make_binary(
    path: Path, output: str, *, exit_code: int = 0, mutate: bool = False
) -> None:
    mutation = "printf '# mutation\\n' >> \"$0\"\n" if mutate else ""
    path.write_text(
        f"#!/bin/sh\n{mutation}printf '%s\\n' '{output}'\nexit {exit_code}\n",
        encoding="utf-8",
    )
    path.chmod(0o755)


def inspect(repo: Path, binary: Path) -> provenance.CodelensBinaryProvenance:
    return provenance.inspect_codelens_binary(
        provenance.CodelensBinaryRequest(repo=repo, binary=binary)
    )


def test_valid_binary_provenance_captures_build_and_content_identity() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-provenance-") as raw_tmp:
        root = Path(raw_tmp)
        repo, head_sha = make_repo(root)
        binary = root / "codelens-mcp"
        make_binary(
            binary,
            f"codelens-mcp 1.13.34 (git {head_sha[:7]}, dirty false, built 2026-07-11T00:00:00Z)",
        )

        result = inspect(repo, binary)

        assert result.version == "1.13.34"
        assert result.embedded_git_sha == head_sha[:7]
        assert result.dirty is False
        assert result.built_at == "2026-07-11T00:00:00Z"
        assert len(result.content_sha256) == 64
        assert result.repo_head_sha == head_sha


def test_binary_provenance_rejects_untrusted_version_evidence() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-provenance-") as raw_tmp:
        root = Path(raw_tmp)
        repo, head_sha = make_repo(root)
        cases = (
            (
                "nonzero",
                f"codelens-mcp 1.0.0 (git {head_sha[:7]}, dirty false, built 2026-07-11T00:00:00Z)",
                2,
            ),
            ("malformed", "not-codelens", 0),
            (
                "unknown",
                "codelens-mcp 1.0.0 (git unknown, dirty false, built 2026-07-11T00:00:00Z)",
                0,
            ),
            (
                "dirty",
                f"codelens-mcp 1.0.0 (git {head_sha[:7]}, dirty true, built 2026-07-11T00:00:00Z)",
                0,
            ),
            (
                "mismatch",
                "codelens-mcp 1.0.0 (git deadbee, dirty false, built 2026-07-11T00:00:00Z)",
                0,
            ),
        )
        for name, output, exit_code in cases:
            binary = root / name
            make_binary(binary, output, exit_code=exit_code)
            try:
                inspect(repo, binary)
            except provenance.CodelensBinaryProvenanceError as error:
                assert str(error)
                continue
            raise AssertionError(f"accepted untrusted provenance case: {name}")


def test_binary_provenance_rejects_non_executable_and_repo_or_binary_mutation() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-provenance-") as raw_tmp:
        root = Path(raw_tmp)
        repo, head_sha = make_repo(root)
        non_executable = root / "non-executable"
        make_binary(
            non_executable,
            f"codelens-mcp 1.0.0 (git {head_sha[:7]}, dirty false, built 2026-07-11T00:00:00Z)",
        )
        non_executable.chmod(0o644)
        try:
            inspect(repo, non_executable)
        except provenance.CodelensBinaryProvenanceError as error:
            assert "executable regular file" in str(error)
        else:
            raise AssertionError("accepted a non-executable binary")

        dirty_binary = root / "dirty-repo-binary"
        make_binary(
            dirty_binary,
            f"codelens-mcp 1.0.0 (git {head_sha[:7]}, dirty false, built 2026-07-11T00:00:00Z)",
        )
        (repo / "README.md").write_text("mutated\n", encoding="utf-8")
        try:
            inspect(repo, dirty_binary)
        except provenance.CodelensBinaryProvenanceError as error:
            assert "file mutations" in str(error)
        else:
            raise AssertionError("accepted a mutated source repository")
        git(repo, "restore", "README.md")

        self_mutating = root / "self-mutating"
        make_binary(
            self_mutating,
            f"codelens-mcp 1.0.0 (git {head_sha[:7]}, dirty false, built 2026-07-11T00:00:00Z)",
            mutate=True,
        )
        try:
            inspect(repo, self_mutating)
        except provenance.CodelensBinaryProvenanceError as error:
            assert "mutated during provenance inspection" in str(error)
        else:
            raise AssertionError("accepted a binary that mutated during inspection")


def main() -> int:
    tests = (
        test_valid_binary_provenance_captures_build_and_content_identity,
        test_binary_provenance_rejects_untrusted_version_evidence,
        test_binary_provenance_rejects_non_executable_and_repo_or_binary_mutation,
    )
    for test in tests:
        test()
        print(f"PASS  {test.__name__}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
