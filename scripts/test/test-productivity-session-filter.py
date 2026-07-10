#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# uv run scripts/test/test-productivity-session-filter.py
# ------------------

from __future__ import annotations

import json
import subprocess
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
LOOP_SCRIPT = REPO_ROOT / "scripts" / "run-productivity-proof-loop.sh"


def runtime_event(session_id: str, tool: str) -> dict[str, object]:
    return {
        "timestamp_ms": 1000,
        "tool": tool,
        "surface": "review",
        "elapsed_ms": 1,
        "tokens": 100,
        "success": True,
        "truncated": False,
        "session_id": session_id,
        "client_name": "CodexHarness",
        "recording_origin": "runtime",
    }


def test_productivity_loop_scopes_report_to_selected_session() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-productivity-session-") as raw_tmp:
        temp_root = Path(raw_tmp)
        telemetry_path = temp_root / "tool_usage.jsonl"
        telemetry_path.write_text(
            "\n".join(
                json.dumps(event)
                for event in [
                    runtime_event("selected-session", "tools/list"),
                    runtime_event("selected-session", "prepare_harness_session"),
                    runtime_event("selected-session", "review"),
                    runtime_event("other-session", "tools/list"),
                    runtime_event("other-session", "search"),
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        output_dir = temp_root / "reports"
        proc = subprocess.run(
            [
                "bash",
                str(LOOP_SCRIPT),
                str(REPO_ROOT),
                "--telemetry-path",
                str(telemetry_path),
                "--session-id",
                "selected-session",
                "--output-dir",
                str(output_dir),
                "--run-id",
                "selected",
                "--skip-audit",
            ],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )

        assert proc.returncode == 0, proc.stderr
        report = json.loads(
            (output_dir / "runs" / "selected" / "tool-usage.json").read_text(
                encoding="utf-8"
            )
        )
        index = (output_dir / "runs" / "selected" / "productivity-proof-loop.md").read_text(
            encoding="utf-8"
        )

    behavior = report["behavior"]
    assert behavior["total_events"] == 3
    assert behavior["session_count"] == 1
    assert behavior["tool_counts"] == [
        ["tools/list", 1],
        ["prepare_harness_session", 1],
        ["review", 1],
    ]
    assert behavior["provenance"]["evidence_status"] == "task_observed"
    assert "Session ID: `selected-session`" in index


def main() -> int:
    try:
        test_productivity_loop_scopes_report_to_selected_session()
        print("PASS  test_productivity_loop_scopes_report_to_selected_session")
    except AssertionError as exc:
        print(f"FAIL  test_productivity_loop_scopes_report_to_selected_session: {exc}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
