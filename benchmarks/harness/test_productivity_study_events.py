#!/usr/bin/env python3
"""Tests for native Codex and Claude study event normalization."""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_contract as contract
import productivity_study_events as events


def test_codex_stream_collects_usage_and_rework_metrics() -> None:
    raw = "\n".join(
        [
            '{"type":"item.completed","item":{"type":"mcp_tool_call","server":"codelens","tool":"review","status":"completed"}}',
            '{"type":"item.completed","item":{"type":"file_change","changes":[{"path":"src/lib.rs"},{"path":"src/lib.rs"}]}}',
            '{"type":"item.completed","item":{"type":"command_execution","command":"cargo test -p codelens-engine","exit_code":1}}',
            '{"type":"item.completed","item":{"type":"agent_message","text":"Found the module."}}',
            '{"type":"turn.completed","usage":{"input_tokens":120,"cached_input_tokens":40,"output_tokens":30,"total_tokens":150}}',
        ]
    )

    telemetry = events.parse_agent_stream(contract.Agent.CODEX, raw)

    assert telemetry.usage.status is events.MeasurementStatus.AVAILABLE
    assert telemetry.usage.input_tokens == 120
    assert telemetry.usage.cached_tokens == 40
    assert telemetry.usage.output_tokens == 30
    assert telemetry.usage.total_tokens == 150
    assert telemetry.activity.tool_calls == 1
    assert telemetry.activity.codelens_calls == 1
    assert telemetry.activity.file_write_events == 2
    assert telemetry.activity.revisited_write_paths == 1
    assert telemetry.activity.test_commands == 1
    assert telemetry.activity.failed_test_commands == 1
    assert telemetry.activity.turns == 1
    assert events.extract_final_response(contract.Agent.CODEX, raw) == "Found the module."


def test_claude_stream_marks_missing_usage_as_unavailable_instead_of_zero() -> None:
    raw = (
        '{"type":"assistant","message":{"content":[{"type":"tool_use",'
        '"name":"Edit","input":{"file_path":"src/app.ts"}}]}}\n'
    )

    telemetry = events.parse_agent_stream(contract.Agent.CLAUDE, raw)

    assert telemetry.usage.status is events.MeasurementStatus.UNAVAILABLE
    assert telemetry.usage.total_tokens is None
    assert telemetry.activity.file_write_events == 1


def test_claude_result_response_is_extracted_without_preserving_raw_stream() -> None:
    raw = '{"type":"result","result":"Reviewed the routes.","usage":{"input_tokens":5,"output_tokens":3}}\n'

    telemetry = events.parse_agent_stream(contract.Agent.CLAUDE, raw)

    assert telemetry.usage.status is events.MeasurementStatus.AVAILABLE
    assert telemetry.usage.total_tokens == 8
    assert telemetry.activity.turns == 1
    assert events.extract_final_response(contract.Agent.CLAUDE, raw) == "Reviewed the routes."


def main() -> int:
    tests = [
        test_codex_stream_collects_usage_and_rework_metrics,
        test_claude_stream_marks_missing_usage_as_unavailable_instead_of_zero,
        test_claude_result_response_is_extracted_without_preserving_raw_stream,
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
