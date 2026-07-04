#!/usr/bin/env python3
from __future__ import annotations

import tempfile
from pathlib import Path

from analyze_tool_usage_rollout_fixtures import (
    codelens_call,
    codelens_result,
    codex_function_call,
    codex_function_output,
    external_tool_call,
    external_tool_result,
    run_rollout_analyzer,
    write_jsonl,
)


def test_codex_rollout_report_counts_recommended_entrypoint_followthrough() -> None:
    with tempfile.TemporaryDirectory() as tempdir:
        rollout_path = Path(tempdir) / "rollout-test.jsonl"
        write_jsonl(
            rollout_path,
            [
                {"type": "session_meta", "payload": {"id": "session-a"}},
                codelens_call("prepare_harness_session", '{"project":"/repo"}'),
                codelens_result(
                    '{"success":true,"routing":{"recommended_entrypoint":"review_architecture"}}'
                ),
                codelens_call("review_architecture", '{"path":"scripts"}'),
                codelens_result('{"success":true}'),
            ],
        )

        report = run_rollout_analyzer(rollout_path)

    behavior = report["behavior"]
    assert behavior["total_events"] == 2
    assert behavior["session_count"] == 1
    assert behavior["suggestion_events"] == 1
    assert behavior["suggestions_followed"] == 1
    assert behavior["suggestions_missed"] == 0


def test_codex_rollout_report_labels_missed_followup_routes() -> None:
    with tempfile.TemporaryDirectory() as tempdir:
        rollout_path = Path(tempdir) / "rollout-test.jsonl"
        write_jsonl(
            rollout_path,
            [
                {"type": "session_meta", "payload": {"id": "session-b"}},
                codelens_call("prepare_harness_session", '{"project":"/repo"}'),
                codelens_result(
                    '{"success":true,"routing":{"recommended_entrypoint":"explore_codebase"}}'
                ),
                codelens_call("review_architecture", '{"path":"scripts"}'),
                codelens_result('{"success":true}'),
                codelens_call("prepare_harness_session", '{"project":"/repo"}'),
                codelens_result(
                    '{"success":true,"routing":{"recommended_entrypoint":"review_changes"}}'
                ),
                codelens_call(
                    "prepare_harness_session",
                    '{"project":"/repo","profile":"planner-readonly"}',
                ),
                codelens_result('{"success":true}'),
            ],
        )

        report = run_rollout_analyzer(rollout_path)

    behavior = report["behavior"]
    assert behavior["suggestion_events"] == 2
    assert behavior["suggestions_followed"] == 0
    assert behavior["suggestions_missed"] == 2
    assert behavior["missed_label_counts"] == [
        ["workflow_alternative", 1],
        ["rebootstrap_or_health_check", 1],
    ]
    assert [row["route_label"] for row in behavior["missed_suggestions"]] == [
        "workflow_alternative",
        "rebootstrap_or_health_check",
    ]


def test_codex_rollout_report_labels_user_clarification_after_suggestion() -> None:
    with tempfile.TemporaryDirectory() as tempdir:
        rollout_path = Path(tempdir) / "rollout-test.jsonl"
        write_jsonl(
            rollout_path,
            [
                {"type": "session_meta", "payload": {"id": "session-c"}},
                codelens_call("prepare_harness_session", '{"project":"/repo"}'),
                codelens_result(
                    '{"success":true,"routing":{"recommended_entrypoint":"review_architecture"}}'
                ),
                external_tool_call("AskUserQuestion", '{"questions":[]}'),
            ],
        )

        report = run_rollout_analyzer(rollout_path)

    behavior = report["behavior"]
    assert behavior["missed_label_counts"] == [["user_clarification", 1]]
    assert behavior["missed_suggestions"][0]["next_external_tools"] == [
        "AskUserQuestion"
    ]


def test_codex_rollout_report_labels_native_fallback_after_suggestion() -> None:
    with tempfile.TemporaryDirectory() as tempdir:
        rollout_path = Path(tempdir) / "rollout-test.jsonl"
        write_jsonl(
            rollout_path,
            [
                {"type": "session_meta", "payload": {"id": "session-d"}},
                codelens_call("prepare_harness_session", '{"project":"/repo"}'),
                codelens_result(
                    '{"success":true,"routing":{"recommended_entrypoint":"explore_codebase"}}'
                ),
                external_tool_call("Bash", '{"cmd":"rg CodeLens"}'),
            ],
        )

        report = run_rollout_analyzer(rollout_path)

    behavior = report["behavior"]
    assert behavior["missed_label_counts"] == [["native_fallback", 1]]
    assert behavior["missed_suggestions"][0]["next_external_tools"] == ["Bash"]


def test_codex_rollout_report_estimates_external_transfer_tokens() -> None:
    with tempfile.TemporaryDirectory() as tempdir:
        rollout_path = Path(tempdir) / "rollout-test.jsonl"
        write_jsonl(
            rollout_path,
            [
                {"type": "session_meta", "payload": {"id": "session-cost"}},
                codelens_call("prepare_harness_session", '{"project":"/repo"}'),
                codelens_result(
                    '{"success":true,"routing":{"recommended_entrypoint":"explore_codebase"}}'
                ),
                external_tool_call("Bash", '{"cmd":"rg CodeLens scripts"}'),
                external_tool_result("short focused output"),
            ],
        )

        report = run_rollout_analyzer(rollout_path)

    transfer = report["behavior"]["missed_suggestions"][0]["external_transfer"]
    assert transfer["tool_count"] == 1
    assert transfer["total_chars"] > len("short focused output")
    assert transfer["estimated_tokens"] > 0
    assert transfer["efficiency_band"] == "compact"


def test_codex_rollout_report_flags_external_transfer_overflow() -> None:
    with tempfile.TemporaryDirectory() as tempdir:
        rollout_path = Path(tempdir) / "rollout-test.jsonl"
        write_jsonl(
            rollout_path,
            [
                {"type": "session_meta", "payload": {"id": "session-overflow"}},
                codelens_call("prepare_harness_session", '{"project":"/repo"}'),
                codelens_result(
                    '{"success":true,"routing":{"recommended_entrypoint":"review_changes"}}'
                ),
                external_tool_call("Bash", '{"cmd":"cat huge.log"}'),
                external_tool_result(
                    "Error: result (81,320 characters across 1 line) exceeds "
                    "maximum allowed tokens. Output has been saved to /tmp/out.txt."
                ),
            ],
        )

        report = run_rollout_analyzer(rollout_path)

    transfer = report["behavior"]["missed_suggestions"][0]["external_transfer"]
    assert transfer["overflow_count"] == 1
    assert transfer["efficiency_band"] == "overflow"


def test_codex_rollout_report_labels_dynamic_workflow_after_suggestion() -> None:
    with tempfile.TemporaryDirectory() as tempdir:
        rollout_path = Path(tempdir) / "rollout-test.jsonl"
        write_jsonl(
            rollout_path,
            [
                {"type": "session_meta", "payload": {"id": "session-e"}},
                codelens_call("prepare_harness_session", '{"project":"/repo"}'),
                codelens_result(
                    '{"success":true,"routing":{"recommended_entrypoint":"review_changes"}}'
                ),
                external_tool_call("TaskCreate", '{"task":"inspect prior sessions"}'),
                external_tool_call("TaskUpdate", '{"status":"in_progress"}'),
            ],
        )

        report = run_rollout_analyzer(rollout_path)

    behavior = report["behavior"]
    assert behavior["missed_label_counts"] == [["dynamic_workflow", 1]]
    assert behavior["missed_suggestions"][0]["next_external_tools"] == [
        "TaskCreate",
        "TaskUpdate",
    ]


def test_codex_rollout_report_classifies_claude_branch_transfer() -> None:
    with tempfile.TemporaryDirectory() as tempdir:
        rollout_path = Path(tempdir) / "rollout-test.jsonl"
        write_jsonl(
            rollout_path,
            [
                {"type": "session_meta", "payload": {"id": "session-claude"}},
                codelens_call("prepare_harness_session", '{"project":"/repo"}'),
                codelens_result(
                    '{"success":true,"routing":{"recommended_entrypoint":"review_changes"}}'
                ),
                external_tool_call("Bash", '{"cmd":"rg CodeLens"}'),
                external_tool_result("grep output"),
            ],
        )

        report = run_rollout_analyzer(rollout_path)

    behavior = report["behavior"]
    row = behavior["missed_suggestions"][0]
    assert behavior["missed_branch_counts"] == [["claude", 1]]
    assert row["agent_branch"] == "claude"
    assert row["branch_transfers"]["claude"]["tool_count"] == 1
    assert row["branch_transfers"]["codex"]["tool_count"] == 0


def test_codex_rollout_report_classifies_codex_response_item_branch() -> None:
    with tempfile.TemporaryDirectory() as tempdir:
        rollout_path = Path(tempdir) / "rollout-test.jsonl"
        write_jsonl(
            rollout_path,
            [
                {"type": "session_meta", "payload": {"id": "session-codex"}},
                codelens_call("prepare_harness_session", '{"project":"/repo"}'),
                codelens_result(
                    '{"success":true,"routing":{"recommended_entrypoint":"explore_codebase"}}'
                ),
                codex_function_call("exec_command", '{"cmd":"rg CodeLens"}'),
                codex_function_output("short output"),
            ],
        )

        report = run_rollout_analyzer(rollout_path)

    behavior = report["behavior"]
    row = behavior["missed_suggestions"][0]
    assert behavior["missed_branch_counts"] == [["codex", 1]]
    assert row["agent_branch"] == "codex"
    assert row["route_label"] == "native_fallback"
    assert row["next_external_tools"] == ["exec_command"]
    assert row["branch_transfers"]["codex"]["tool_count"] == 1
    assert row["branch_transfers"]["claude"]["tool_count"] == 0


def test_codex_rollout_report_classifies_mixed_branch_transfer() -> None:
    with tempfile.TemporaryDirectory() as tempdir:
        rollout_path = Path(tempdir) / "rollout-test.jsonl"
        write_jsonl(
            rollout_path,
            [
                {"type": "session_meta", "payload": {"id": "session-mixed"}},
                codelens_call("prepare_harness_session", '{"project":"/repo"}'),
                codelens_result(
                    '{"success":true,"routing":{"recommended_entrypoint":"review_changes"}}'
                ),
                external_tool_call("TaskCreate", '{"task":"inspect"}'),
                codex_function_call("exec_command", '{"cmd":"rg CodeLens"}'),
                codex_function_output("short output"),
            ],
        )

        report = run_rollout_analyzer(rollout_path)

    behavior = report["behavior"]
    row = behavior["missed_suggestions"][0]
    assert behavior["missed_branch_counts"] == [["mixed", 1]]
    assert row["agent_branch"] == "mixed"
    assert row["branch_transfers"]["claude"]["tool_count"] == 1
    assert row["branch_transfers"]["codex"]["tool_count"] == 1


def main() -> int:
    tests = [
        test_codex_rollout_report_counts_recommended_entrypoint_followthrough,
        test_codex_rollout_report_labels_missed_followup_routes,
        test_codex_rollout_report_labels_user_clarification_after_suggestion,
        test_codex_rollout_report_labels_native_fallback_after_suggestion,
        test_codex_rollout_report_estimates_external_transfer_tokens,
        test_codex_rollout_report_flags_external_transfer_overflow,
        test_codex_rollout_report_labels_dynamic_workflow_after_suggestion,
        test_codex_rollout_report_classifies_claude_branch_transfer,
        test_codex_rollout_report_classifies_codex_response_item_branch,
        test_codex_rollout_report_classifies_mixed_branch_transfer,
    ]
    for test in tests:
        try:
            test()
            print(f"PASS  {test.__name__}")
        except AssertionError as exc:
            print(f"FAIL  {test.__name__}: {exc}")
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
