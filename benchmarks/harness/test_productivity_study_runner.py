#!/usr/bin/env python3
"""Integration tests for isolated productivity-study execution state."""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_runner as runner


def git(repo: Path, *args: str) -> str:
    completed = subprocess.run(
        ["git", *args],
        cwd=repo,
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip()


def test_disposable_worktree_uses_pinned_commit_without_source_wip() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-repo-") as raw_tmp:
        repo = Path(raw_tmp) / "source"
        repo.mkdir()
        git(repo, "init")
        git(repo, "config", "user.email", "study@example.test")
        git(repo, "config", "user.name", "Study Test")
        (repo / "tracked.txt").write_text("base\n", encoding="utf-8")
        git(repo, "add", "tracked.txt")
        git(repo, "commit", "-m", "base")
        base_sha = git(repo, "rev-parse", "HEAD")
        (repo / "local-wip.txt").write_text("must not leak\n", encoding="utf-8")
        run_root = repo / "study-runs"

        request = runner.WorktreeRequest(
            source_repo=repo,
            base_sha=base_sha,
            run_root=run_root,
            run_id="run-01",
        )
        with runner.disposable_worktree(request) as worktree:
            assert git(worktree, "rev-parse", "HEAD") == base_sha
            assert (worktree / "tracked.txt").read_text(encoding="utf-8") == "base\n"
            assert (worktree / "local-wip.txt").exists() is False

        assert (run_root / "run-01").exists() is False


def test_policy_snapshot_detects_mutation_after_capture() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-policy-") as raw_tmp:
        policy_path = Path(raw_tmp) / "policy.json"
        policy_path.write_text('{"route":"routed"}\n', encoding="utf-8")
        snapshot = runner.PolicySnapshot.capture(policy_path)

        policy_path.write_text('{"route":"native"}\n', encoding="utf-8")

        assert snapshot.matches(policy_path) is False


def test_task_pack_expands_to_balanced_agent_condition_runs() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-pack-") as raw_tmp:
        pack_path = Path(raw_tmp) / "tasks.json"
        pack_path.write_text(
            """{
  "schema_version": "productivity-study-task-pack-v1",
  "tasks": [
    {
      "task_id": "codelens::lookup::01",
      "repo_id": "codelens",
      "repo_path": "/tmp/codelens",
      "task_kind": "simple local lookup",
      "base_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "target_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      "read_only": true,
      "prompt": "Find the smallest relevant symbol.",
      "verification_commands": [],
      "allowed_paths": [],
      "hidden_test_paths": []
    },
    {
      "task_id": "signature::impact::01",
      "repo_id": "signature",
      "repo_path": "/tmp/signature",
      "task_kind": "impact/reviewer",
      "base_sha": "cccccccccccccccccccccccccccccccccccccccc",
      "target_sha": "dddddddddddddddddddddddddddddddddddddddd",
      "read_only": true,
      "prompt": "Review cross-file impact.",
      "verification_commands": [],
      "allowed_paths": [],
      "hidden_test_paths": []
    }
  ]
}
""",
            encoding="utf-8",
        )

        tasks = runner.load_task_pack(pack_path)
        plan = runner.expand_run_plan(tasks, ("codex", "claude"))

    assert len(plan) == 12
    assert {item.condition.value for item in plan} == {
        "baseline",
        "naive-on",
        "routed-on",
    }
    assert [item.sequence_order for item in plan] == list(range(12))
    assert plan[0].condition.value == "baseline"
    assert plan[3].condition.value == "naive-on"


def test_hidden_test_grading_uses_separate_target_worktree_and_allowed_diff() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-grading-") as raw_tmp:
        repo = Path(raw_tmp) / "source"
        repo.mkdir()
        git(repo, "init")
        git(repo, "config", "user.email", "study@example.test")
        git(repo, "config", "user.name", "Study Test")
        (repo / "app.py").write_text("def answer():\n    return 'wrong'\n", encoding="utf-8")
        git(repo, "add", "app.py")
        git(repo, "commit", "-m", "base")
        base_sha = git(repo, "rev-parse", "HEAD")
        tests_dir = repo / "tests"
        tests_dir.mkdir()
        (tests_dir / "test_hidden.py").write_text(
            "from app import answer\nassert answer() == 'right'\n", encoding="utf-8"
        )
        git(repo, "add", "tests/test_hidden.py")
        git(repo, "commit", "-m", "hidden evaluator")
        target_sha = git(repo, "rev-parse", "HEAD")
        run_root = repo / "study-runs"

        candidate_request = runner.WorktreeRequest(repo, base_sha, run_root, "candidate")
        evaluator_request = runner.WorktreeRequest(repo, target_sha, run_root, "evaluator")
        with runner.disposable_worktree(candidate_request) as candidate:
            (candidate / "app.py").write_text("def answer():\n    return 'right'\n", encoding="utf-8")
            with runner.disposable_worktree(evaluator_request) as evaluator:
                grade = runner.grade_candidate(
                    candidate,
                    evaluator,
                    hidden_test_paths=("tests/test_hidden.py",),
                    verification_commands=("PYTHONPATH=. python3 tests/test_hidden.py",),
                    allowed_paths=("app.py",),
                )

        assert grade.accepted is True
        assert grade.allowed_diff is True
        assert grade.verification_passed is True
        assert grade.changed_paths == ("app.py",)


def test_hidden_test_grading_rejects_out_of_bounds_candidate_write() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-grading-") as raw_tmp:
        repo = Path(raw_tmp) / "source"
        repo.mkdir()
        git(repo, "init")
        git(repo, "config", "user.email", "study@example.test")
        git(repo, "config", "user.name", "Study Test")
        (repo / "app.py").write_text("value = 1\n", encoding="utf-8")
        git(repo, "add", "app.py")
        git(repo, "commit", "-m", "base")
        base_sha = git(repo, "rev-parse", "HEAD")
        (repo / "hidden.py").write_text("assert True\n", encoding="utf-8")
        git(repo, "add", "hidden.py")
        git(repo, "commit", "-m", "hidden evaluator")
        target_sha = git(repo, "rev-parse", "HEAD")
        run_root = repo / "study-runs"

        candidate_request = runner.WorktreeRequest(repo, base_sha, run_root, "candidate")
        evaluator_request = runner.WorktreeRequest(repo, target_sha, run_root, "evaluator")
        with runner.disposable_worktree(candidate_request) as candidate:
            (candidate / "unrelated.txt").write_text("out of bounds\n", encoding="utf-8")
            with runner.disposable_worktree(evaluator_request) as evaluator:
                grade = runner.grade_candidate(
                    candidate,
                    evaluator,
                    hidden_test_paths=("hidden.py",),
                    verification_commands=("python3 hidden.py",),
                    allowed_paths=("app.py",),
                )

        assert grade.accepted is False
        assert grade.allowed_diff is False
        assert grade.changed_paths == ("unrelated.txt",)


def test_pilot_task_pack_has_hidden_rubrics_for_every_task() -> None:
    pack_path = Path(__file__).with_name("productivity-study-pilot-v1.json")

    tasks = runner.load_task_pack(pack_path)

    assert len(tasks) == 8
    assert all(task.hidden_rubric for task in tasks)


def main() -> int:
    tests = [
        test_disposable_worktree_uses_pinned_commit_without_source_wip,
        test_policy_snapshot_detects_mutation_after_capture,
        test_task_pack_expands_to_balanced_agent_condition_runs,
        test_hidden_test_grading_uses_separate_target_worktree_and_allowed_diff,
        test_hidden_test_grading_rejects_out_of_bounds_candidate_write,
        test_pilot_task_pack_has_hidden_rubrics_for_every_task,
    ]
    failures = 0
    for test in tests:
        try:
            test()
            print(f"PASS  {test.__name__}")
        except Exception as error:
            failures += 1
            print(f"FAIL  {test.__name__}: {error}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
