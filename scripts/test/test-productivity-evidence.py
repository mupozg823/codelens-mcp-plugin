#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# uv run scripts/test/test-productivity-evidence.py
# ------------------

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
ANALYZER = REPO_ROOT / "scripts" / "analyze-tool-usage.py"
TREND_SCRIPT = REPO_ROOT / "scripts" / "summarize-productivity-proof-runs.py"


def test_analyzer_marks_attributed_bootstrap_as_bootstrap_only() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-bootstrap-evidence-") as raw_tmp:
        temp_root = Path(raw_tmp)
        telemetry_path = temp_root / "tool_usage.jsonl"
        telemetry_path.write_text(
            json.dumps(
                {
                    "timestamp_ms": 1000,
                    "tool": "tools/list",
                    "surface": "review",
                    "elapsed_ms": 0,
                    "tokens": 100,
                    "success": True,
                    "truncated": False,
                    "session_id": "9a42c3aa-2741-4672-a02c-c0a3a74a00d2",
                    "client_name": "CodexHarness",
                    "recording_origin": "runtime",
                }
            )
            + "\n",
            encoding="utf-8",
        )
        proc = subprocess.run(
            [sys.executable, str(ANALYZER), "--telemetry-path", str(telemetry_path), "--format", "json"],
            capture_output=True,
            text=True,
            check=False,
        )

    assert proc.returncode == 0, proc.stderr
    provenance = json.loads(proc.stdout)["behavior"]["provenance"]
    assert provenance["status"] == "verified"
    assert provenance.get("evidence_status") == "bootstrap_only"


def test_analyzer_keeps_session_preparation_out_of_productivity_evidence() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-preparation-evidence-") as raw_tmp:
        temp_root = Path(raw_tmp)
        telemetry_path = temp_root / "tool_usage.jsonl"
        telemetry_path.write_text(
            json.dumps(
                {
                    "timestamp_ms": 1000,
                    "tool": "prepare_harness_session",
                    "surface": "review",
                    "elapsed_ms": 0,
                    "tokens": 100,
                    "success": True,
                    "truncated": False,
                    "session_id": "9a42c3aa-2741-4672-a02c-c0a3a74a00d2",
                    "client_name": "Claude Code",
                    "recording_origin": "runtime",
                }
            )
            + "\n",
            encoding="utf-8",
        )
        proc = subprocess.run(
            [sys.executable, str(ANALYZER), "--telemetry-path", str(telemetry_path), "--format", "json"],
            capture_output=True,
            text=True,
            check=False,
        )

    assert proc.returncode == 0, proc.stderr
    provenance = json.loads(proc.stdout)["behavior"]["provenance"]
    assert provenance["status"] == "verified"
    assert provenance.get("evidence_status") == "bootstrap_only"


def test_analyzer_distinguishes_alternative_codelens_task_from_missed_route() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-alternative-route-") as raw_tmp:
        temp_root = Path(raw_tmp)
        telemetry_path = temp_root / "tool_usage.jsonl"
        telemetry_path.write_text(
            "\n".join(
                json.dumps(event)
                for event in [
                    {
                        "timestamp_ms": 1000,
                        "tool": "prepare_harness_session",
                        "surface": "review",
                        "elapsed_ms": 0,
                        "tokens": 100,
                        "success": True,
                        "truncated": False,
                        "session_id": "9a42c3aa-2741-4672-a02c-c0a3a74a00d2",
                        "client_name": "CodexHarness",
                        "recording_origin": "runtime",
                        "suggested_next_tools": ["get_ranked_context"],
                    },
                    {
                        "timestamp_ms": 1001,
                        "tool": "review",
                        "surface": "review",
                        "elapsed_ms": 1,
                        "tokens": 100,
                        "success": True,
                        "truncated": False,
                        "session_id": "9a42c3aa-2741-4672-a02c-c0a3a74a00d2",
                        "client_name": "CodexHarness",
                        "recording_origin": "runtime",
                    },
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        proc = subprocess.run(
            [sys.executable, str(ANALYZER), "--telemetry-path", str(telemetry_path), "--format", "json"],
            capture_output=True,
            text=True,
            check=False,
        )

    assert proc.returncode == 0, proc.stderr
    behavior = json.loads(proc.stdout)["behavior"]
    assert behavior["suggestions_followed"] == 0
    assert behavior["suggestions_missed"] == 0
    assert behavior["suggestions_diverted"] == 1


def test_trend_summary_rejects_attributed_bootstrap_as_productivity_evidence() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-bootstrap-summary-") as raw_tmp:
        temp_root = Path(raw_tmp)
        runs_dir = temp_root / "runs"
        usage_path = runs_dir / "20260710-120000" / "tool-usage.json"
        usage_path.parent.mkdir(parents=True)
        usage_path.write_text(
            json.dumps(
                {
                    "behavior": {
                        "total_events": 1,
                        "session_count": 1,
                        "suggestion_events": 0,
                        "suggestions_followed": 0,
                        "suggestions_missed": 0,
                        "suggestion_follow_rate": 0.0,
                        "delegate_emissions": 0,
                        "delegate_handoffs_consumed": 0,
                        "codex_builder_tool_events": 0,
                        "provenance": {
                            "status": "verified",
                            "evidence_status": "bootstrap_only",
                            "runtime_events": 1,
                            "host_runtime_events": 1,
                            "unattributed_runtime_events": 0,
                            "legacy_unverified_events": 0,
                        },
                    }
                }
            ),
            encoding="utf-8",
        )
        output_path = temp_root / "summary.md"
        proc = subprocess.run(
            [
                sys.executable,
                str(TREND_SCRIPT),
                "--input-dir",
                str(runs_dir),
                "--output",
                str(output_path),
            ],
            capture_output=True,
            text=True,
            check=False,
        )

        assert proc.returncode == 0, proc.stderr
        rendered = output_path.read_text(encoding="utf-8")

    assert "Attribution status: `verified`" in rendered
    assert "Productivity evidence: `bootstrap_only`" in rendered
    assert "cannot support a productivity claim" in rendered


def main() -> int:
    tests = [
        test_analyzer_marks_attributed_bootstrap_as_bootstrap_only,
        test_analyzer_keeps_session_preparation_out_of_productivity_evidence,
        test_analyzer_distinguishes_alternative_codelens_task_from_missed_route,
        test_trend_summary_rejects_attributed_bootstrap_as_productivity_evidence,
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
