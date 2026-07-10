#!/usr/bin/env python3
"""Tests for study execution artifacts that must not reveal treatment identity."""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_execution as execution
import productivity_study_runtime as runtime
from productivity_study_contract import Agent, Condition
from productivity_study_runner import PlannedRun, StudyTask


def task(
    repo_id: str = "repo", task_kind: str = "multi-file-impact-review"
) -> StudyTask:
    return StudyTask(
        task_id="repo::impact::001",
        repo_id=repo_id,
        repo_path=Path("/tmp/repo"),
        task_kind=task_kind,
        base_sha="a" * 40,
        target_sha="b" * 40,
        read_only=True,
        prompt="Review the change.",
        verification_commands=(),
        allowed_paths=(),
        hidden_test_paths=(),
        hidden_rubric=("Names the impact.", "States the regression."),
    )


def test_blind_review_packet_omits_agent_and_condition() -> None:
    planned = PlannedRun(task(), Agent.CODEX, Condition.ROUTED, 7)

    packet = execution.build_blind_review_packet("run-007", planned, "Reviewed two files.")

    assert packet["review_id"] == execution.blind_review_id_for("run-007")
    assert packet["task_kind"] == "multi-file-impact-review"
    assert packet["response"] == "Reviewed two files."
    assert "run_id" not in packet
    assert "agent" not in packet
    assert "condition" not in packet


def test_run_id_is_deterministic_and_includes_latin_order() -> None:
    planned = PlannedRun(task(), Agent.CLAUDE, Condition.NAIVE, 7)

    run_id = execution.run_id_for(planned)

    assert run_id == "007-repo-impact-001-claude-naive-on"


def test_dedicated_daemon_command_binds_only_the_candidate_worktree() -> None:
    command = execution.build_daemon_command(
        Path("/tmp/codelens-mcp"), Path("/tmp/candidate"), 17839
    )

    assert command[0] == "/tmp/codelens-mcp"
    assert command[1] == "/tmp/candidate"
    assert command[2:4] == ("--preset", "full")
    assert command[-6:] == (
        "--listen",
        "127.0.0.1",
        "--port",
        "17839",
        "--auth",
        "off",
    )


def test_process_cpu_parser_handles_minutes_and_days() -> None:
    assert execution.runtime_cpu_millis("01:02") == 62_000
    assert execution.runtime_cpu_millis("1-00:00:01") == 86_401_000
    assert execution.runtime_cpu_millis("00:00.04") == 40


def test_metrics_envelope_unwraps_compact_data_without_defaulting_to_zero() -> None:
    payload = runtime.unwrap_metrics_payload(
        {"success": True, "data": {"session": {"total_calls": 3}, "token_bill": {"total_tokens": 8}}}
    )

    assert payload["session"]["total_calls"] == 3
    assert payload["token_bill"]["total_tokens"] == 8


def test_fixed_policy_routes_both_simple_lookup_tasks_to_native_only() -> None:
    policy_path = Path(__file__).with_name("productivity-study-routing-policy-v1.json")
    assert policy_path.is_file(), f"missing fixed study policy: {policy_path}"

    excerpts = tuple(
        execution.policy_excerpt(
            policy_path,
            PlannedRun(task(repo_id, "simple-local-lookup"), Agent.CODEX, Condition.ROUTED, 0),
        )
        for repo_id in ("codelens-mcp-plugin", "signaturestudio")
    )

    assert all(excerpt.startswith("avoid_codelens_for_simple_local_lookup:") for excerpt in excerpts)
    assert all("native repository tools only" in excerpt.lower() for excerpt in excerpts)
    assert all("do not bootstrap or call codelens" in excerpt.lower() for excerpt in excerpts)


def test_fixed_policy_keeps_codelens_first_for_complex_treatments() -> None:
    policy_path = Path(__file__).with_name("productivity-study-routing-policy-v1.json")
    assert policy_path.is_file(), f"missing fixed study policy: {policy_path}"

    excerpts = tuple(
        execution.policy_excerpt(
            policy_path,
            PlannedRun(task("codelens-mcp-plugin", task_kind), Agent.CODEX, Condition.ROUTED, 0),
        )
        for task_kind in ("multi-file-impact-review", "safe-refactor")
    )

    assert all(excerpt.startswith("prefer_codelens_after_bootstrap:") for excerpt in excerpts)
    assert all("switch to CodeLens" in excerpt for excerpt in excerpts)


def main() -> int:
    tests = [
        test_blind_review_packet_omits_agent_and_condition,
        test_run_id_is_deterministic_and_includes_latin_order,
        test_dedicated_daemon_command_binds_only_the_candidate_worktree,
        test_process_cpu_parser_handles_minutes_and_days,
        test_metrics_envelope_unwraps_compact_data_without_defaulting_to_zero,
        test_fixed_policy_routes_both_simple_lookup_tasks_to_native_only,
        test_fixed_policy_keeps_codelens_first_for_complex_treatments,
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
