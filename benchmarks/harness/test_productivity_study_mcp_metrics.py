#!/usr/bin/env python3
"""Tests for session-scoped daemon measurements and missing-data handling."""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_mcp_metrics as metrics


def write_events(path: Path, rows: tuple[str, ...]) -> None:
    path.write_text("\n".join(rows) + "\n", encoding="utf-8")


def test_agent_events_exclude_control_session_and_preserve_percentiles() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-mcp-") as raw_tmp:
        path = Path(raw_tmp) / "telemetry.jsonl"
        write_events(
            path,
            (
                '{"session_id":"control","elapsed_ms":999,"tokens":999}',
                '{"session_id":"agent","elapsed_ms":10,"tokens":5}',
                '{"session_id":"agent","elapsed_ms":40,"tokens":7}',
                '{"session_id":"agent","elapsed_ms":20,"tokens":3}',
            ),
        )

        result = metrics.aggregate_agent_metrics(
            path,
            ("control", "health"),
            lambda _: {"session": {"handle_reuse_count": 2}},
            {"daemon_cpu_ms": 8, "peak_rss_bytes": 100},
            12,
        )

    assert result["status"] == "available"
    assert result["context_tokens"] == 15
    assert result["tool_latency_p50_ms"] == 20
    assert result["tool_latency_p95_ms"] == 20
    assert result["handle_reuse_count"] == 2
    assert result["agent_mcp_event_count"] == 3
    assert result["daemon_startup_ms"] == 12


def test_no_agent_session_is_unavailable_not_zero() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-mcp-") as raw_tmp:
        path = Path(raw_tmp) / "telemetry.jsonl"
        write_events(path, ('{"session_id":"control","elapsed_ms":3,"tokens":1}',))

        result = metrics.aggregate_agent_metrics(
            path,
            ("control", "health"),
            lambda _: {"session": {"handle_reuse_count": 1}},
            {"daemon_cpu_ms": 8, "peak_rss_bytes": 100},
            12,
        )

    assert result["status"] == "unavailable"
    assert result["context_tokens"] is None
    assert result["tool_latency_p50_ms"] is None
    assert result["agent_mcp_event_count"] == 0


def main() -> int:
    tests = [
        test_agent_events_exclude_control_session_and_preserve_percentiles,
        test_no_agent_session_is_unavailable_not_zero,
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
