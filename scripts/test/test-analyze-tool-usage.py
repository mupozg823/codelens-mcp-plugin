#!/usr/bin/env python3
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
ANALYZER = REPO_ROOT / "scripts" / "analyze-tool-usage.py"


def run_analyzer(telemetry_path: Path) -> dict:
    proc = subprocess.run(
        [
            sys.executable,
            str(ANALYZER),
            "--telemetry-path",
            str(telemetry_path),
            "--format",
            "json",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert proc.returncode == 0, (
        f"analyzer should pass: stdout={proc.stdout} stderr={proc.stderr}"
    )
    return json.loads(proc.stdout)


def write_jsonl(path: Path, events: list[dict]) -> None:
    path.write_text(
        "\n".join(json.dumps(event) for event in events) + "\n",
        encoding="utf-8",
    )


def test_behavior_report_counts_suggestions_and_handoff_consumption() -> None:
    with tempfile.TemporaryDirectory() as tempdir:
        telemetry_path = Path(tempdir) / "tool_usage.jsonl"
        write_jsonl(
            telemetry_path,
            [
                {
                    "timestamp_ms": 1000,
                    "tool": "prepare_harness_session",
                    "surface": "builder-minimal",
                    "elapsed_ms": 10,
                    "tokens": 100,
                    "success": True,
                    "truncated": False,
                    "session_id": "planner",
                    "suggested_next_tools": ["review_changes"],
                },
                {
                    "timestamp_ms": 1001,
                    "tool": "review_changes",
                    "surface": "planner-readonly",
                    "elapsed_ms": 15,
                    "tokens": 130,
                    "success": True,
                    "truncated": False,
                    "session_id": "planner",
                },
                {
                    "timestamp_ms": 1002,
                    "tool": "safe_rename_report",
                    "surface": "refactor-full",
                    "elapsed_ms": 20,
                    "tokens": 200,
                    "success": True,
                    "truncated": False,
                    "session_id": "planner",
                    "suggested_next_tools": [
                        "delegate_to_codex_builder",
                        "rename_symbol",
                    ],
                    "delegate_hint_trigger": "preferred_executor_boundary",
                    "delegate_target_tool": "rename_symbol",
                    "delegate_handoff_id": "codelens-handoff-test",
                },
                {
                    "timestamp_ms": 1003,
                    "tool": "rename_symbol",
                    "surface": "refactor-full",
                    "elapsed_ms": 25,
                    "tokens": 260,
                    "success": True,
                    "truncated": False,
                    "session_id": "builder",
                    "handoff_id": "codelens-handoff-test",
                },
                {
                    "timestamp_ms": 1004,
                    "tool": "get_symbols_overview",
                    "surface": "builder-minimal",
                    "elapsed_ms": 9,
                    "tokens": 90,
                    "success": True,
                    "truncated": False,
                    "session_id": "planner",
                    "suggested_next_tools": ["find_referencing_symbols"],
                },
            ],
        )

        report = run_analyzer(telemetry_path)

    behavior = report["behavior"]
    assert behavior["total_events"] == 5
    assert behavior["suggestion_events"] == 3
    assert behavior["suggestions_followed"] == 2
    assert behavior["suggestions_missed"] == 1
    assert behavior["delegate_emissions"] == 1
    assert behavior["delegate_handoffs_consumed"] == 1
    assert behavior["missed_label_counts"] == [["no_codelens_followup", 1]]
    assert behavior["missed_suggestions"][0]["route_label"] == "no_codelens_followup"
    assert behavior["handoff_correlations"] == [
        {
            "handoff_id": "codelens-handoff-test",
            "delegate_target_tool": "rename_symbol",
            "emitting_session": "planner",
            "consuming_session": "builder",
            "consuming_tool": "rename_symbol",
        }
    ]


def test_json_output_handles_missing_default_input() -> None:
    with tempfile.TemporaryDirectory() as tempdir:
        output_path = Path(tempdir) / "empty-report.json"
        proc = subprocess.run(
            [
                sys.executable,
                str(ANALYZER),
                "--format",
                "json",
                "--output",
                str(output_path),
            ],
            input="",
            capture_output=True,
            text=True,
            check=False,
            cwd=tempdir,
        )

        assert proc.returncode == 0, (
            f"empty default analyzer input should pass: stdout={proc.stdout} "
            f"stderr={proc.stderr}"
        )
        report = json.loads(output_path.read_text(encoding="utf-8"))

    assert report["behavior"]["total_events"] == 0
    assert report["behavior"]["suggestion_events"] == 0


def test_behavior_report_marks_legacy_rows_as_unverified() -> None:
    with tempfile.TemporaryDirectory() as tempdir:
        telemetry_path = Path(tempdir) / "tool_usage.jsonl"
        write_jsonl(
            telemetry_path,
            [
                {
                    "timestamp_ms": 1000,
                    "tool": "find_symbol",
                    "surface": "primitive",
                    "elapsed_ms": 10,
                    "tokens": 100,
                    "success": True,
                    "truncated": False,
                    "session_id": "legacy-session",
                }
            ],
        )

        report = run_analyzer(telemetry_path)

    assert report["behavior"]["provenance"] == {
        "status": "unverified",
        "runtime_events": 0,
        "legacy_unverified_events": 1,
    }


def main() -> int:
    tests = [
        test_behavior_report_counts_suggestions_and_handoff_consumption,
        test_json_output_handles_missing_default_input,
        test_behavior_report_marks_legacy_rows_as_unverified,
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
