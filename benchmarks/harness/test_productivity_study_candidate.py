#!/usr/bin/env python3
"""Integration tests for base-only candidate repositories."""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_candidate as candidate
import productivity_study_runner as runner


def git(repo: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args],
        cwd=repo,
        check=check,
        capture_output=True,
        text=True,
    )


def test_candidate_checkout_hides_future_history_wip_and_source_refs() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-candidate-") as raw_tmp:
        root = Path(raw_tmp)
        source = root / "source"
        source.mkdir()
        git(source, "init", "--quiet")
        git(source, "config", "user.email", "study@example.test")
        git(source, "config", "user.name", "Study Test")
        (source / "app.py").write_text(
            "def answer():\n    return 'wrong'\n", encoding="utf-8"
        )
        git(source, "add", "app.py")
        git(source, "commit", "--quiet", "-m", "base")
        base_sha = git(source, "rev-parse", "HEAD").stdout.strip()
        hidden = source / "tests" / "test_hidden.py"
        hidden.parent.mkdir()
        hidden.write_text(
            "from app import answer\nassert answer() == 'right'\n", encoding="utf-8"
        )
        git(source, "add", "tests/test_hidden.py")
        git(source, "commit", "--quiet", "-m", "target")
        target_sha = git(source, "rev-parse", "HEAD").stdout.strip()
        (source / "app.py").write_text("source WIP\n", encoding="utf-8")
        (source / "untracked-wip.txt").write_text("source WIP\n", encoding="utf-8")
        run_root = root / "runs"
        request = candidate.CandidateCheckoutRequest(
            source_repo=source,
            base_sha=base_sha,
            target_sha=target_sha,
            run_root=run_root,
            run_id="candidate",
        )

        previous_git_dir = os.environ.get("GIT_DIR")
        os.environ["GIT_DIR"] = str(source / ".git")
        try:
            with candidate.disposable_candidate_checkout(request) as checkout:
                if previous_git_dir is None:
                    del os.environ["GIT_DIR"]
                else:
                    os.environ["GIT_DIR"] = previous_git_dir
                assert git(checkout, "rev-parse", "HEAD").stdout.strip() == base_sha
                assert (checkout / "app.py").read_text(
                    encoding="utf-8"
                ) == "def answer():\n    return 'wrong'\n"
                assert (checkout / "untracked-wip.txt").exists() is False
                assert (
                    git(
                        checkout,
                        "cat-file",
                        "-e",
                        f"{target_sha}^{{commit}}",
                        check=False,
                    ).returncode
                    != 0
                )
                assert git(checkout, "remote").stdout.strip() == ""
                assert (
                    git(checkout, "for-each-ref", "--format=%(refname)").stdout.strip()
                    == ""
                )
                (checkout / "app.py").write_text(
                    "def answer():\n    return 'right'\n", encoding="utf-8"
                )
                evaluator_request = runner.WorktreeRequest(
                    source, target_sha, run_root, "evaluator"
                )
                with runner.disposable_worktree(evaluator_request) as evaluator:
                    grade = runner.grade_candidate(
                        checkout,
                        evaluator,
                        hidden_test_paths=("tests/test_hidden.py",),
                        verification_commands=(
                            "PYTHONPATH=. python3 tests/test_hidden.py",
                        ),
                        allowed_paths=("app.py",),
                    )
        finally:
            if previous_git_dir is None:
                os.environ.pop("GIT_DIR", None)
            else:
                os.environ["GIT_DIR"] = previous_git_dir

        assert grade.accepted is True
        assert (run_root / "candidate").exists() is False


def main() -> int:
    test_candidate_checkout_hides_future_history_wip_and_source_refs()
    print("PASS  test_candidate_checkout_hides_future_history_wip_and_source_refs")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
