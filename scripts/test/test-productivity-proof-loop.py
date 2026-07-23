#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# 1. Install uv (if not installed):
#      curl -LsSf https://astral.sh/uv/install.sh | sh
# 2. Run directly:
#      uv run scripts/test/test-productivity-proof-loop.py
# 3. CI can also run it with system Python:
#      python3 scripts/test/test-productivity-proof-loop.py
# ------------------

from __future__ import annotations

import json
import subprocess
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
LOOP_SCRIPT = REPO_ROOT / "scripts" / "run-productivity-proof-loop.sh"
EXPORT_SCRIPT = REPO_ROOT / "scripts" / "export-eval-session-audit.sh"
TREND_SCRIPT = REPO_ROOT / "scripts" / "summarize-productivity-proof-runs.py"


def run_command(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        timeout=5,
        check=False,
    )


def test_print_plan_honors_explicit_telemetry_path_without_writing() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-productivity-loop-") as raw_tmp:
        temp_root = Path(raw_tmp)
        telemetry_path = (
            REPO_ROOT
            / "crates"
            / "codelens-mcp"
            / ".codelens"
            / "telemetry"
            / "tool_usage.jsonl"
        )
        output_dir = temp_root / "reports"

        # The telemetry path is untracked runtime state. Pass it explicitly
        # so a live repo-level telemetry file cannot change this test's plan.
        created_telemetry = not telemetry_path.exists()
        if created_telemetry:
            telemetry_path.parent.mkdir(parents=True, exist_ok=True)
            telemetry_path.touch()

        try:
            proc = run_command(
                [
                    "bash",
                    str(LOOP_SCRIPT),
                    str(REPO_ROOT),
                    "--output-dir",
                    str(output_dir),
                    "--run-id",
                    "test-run",
                    "--telemetry-path",
                    str(telemetry_path),
                    "--print-plan",
                ]
            )

            assert proc.returncode == 0, (
                "print-plan should resolve paths without running the daemon: "
                f"stdout={proc.stdout} stderr={proc.stderr}"
            )
            assert f"repo_root={REPO_ROOT}" in proc.stdout
            assert f"telemetry_path={telemetry_path}" in proc.stdout
            assert f"run_dir={output_dir / 'runs' / 'test-run'}" in proc.stdout
            assert not output_dir.exists()
        finally:
            if created_telemetry:
                telemetry_path.unlink(missing_ok=True)


def test_export_audit_default_matches_repo_local_readonly_daemon() -> None:
    proc = run_command(["bash", str(EXPORT_SCRIPT), "--help"])

    assert proc.returncode == 0, (
        f"help should render: stdout={proc.stdout} stderr={proc.stderr}"
    )
    assert "default: http://127.0.0.1:7838/mcp" in proc.stdout


def write_tool_usage(path: Path, total_events: int, follow_rate: float) -> None:
    path.parent.mkdir(parents=True)
    path.write_text(
        (
            '{"behavior":{'
            f'"total_events":{total_events},'
            '"session_count":2,'
            '"suggestion_events":4,'
            f'"suggestion_follow_rate":{follow_rate},'
            '"suggestions_followed":2,'
            '"suggestions_missed":2,'
            '"suggestions_diverted":0,'
            '"suggestions_unresolved":0,'
            '"delegate_emissions":1,'
            '"delegate_handoffs_consumed":1,'
            '"mutation_tool_events":1,'
            '"provenance":{"status":"verified","evidence_status":"task_observed","runtime_events":4,"host_runtime_events":4,"unattributed_runtime_events":0,"legacy_unverified_events":0}'
            "}}\n"
        ),
        encoding="utf-8",
    )


def test_trend_summary_reports_latest_delta_against_previous_run() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-productivity-trend-") as raw_tmp:
        temp_root = Path(raw_tmp)
        runs_dir = temp_root / "runs"
        output_path = temp_root / "summary.md"
        write_tool_usage(runs_dir / "20260707-100000" / "tool-usage.json", 40, 0.50)
        write_tool_usage(runs_dir / "20260707-110000" / "tool-usage.json", 31, 0.25)

        proc = run_command(
            [
                "python3",
                str(TREND_SCRIPT),
                "--input-dir",
                str(runs_dir),
                "--output",
                str(output_path),
            ]
        )

        assert proc.returncode == 0, (
            "trend summary should compare latest and previous runs: "
            f"stdout={proc.stdout} stderr={proc.stderr}"
        )
        rendered = output_path.read_text(encoding="utf-8")
        assert "Runs analyzed: `2`" in rendered
        assert "Latest run: `20260707-110000`" in rendered
        assert "Tool events: `31` (`-9`)" in rendered
        assert "Suggestions followed/diverted/unresolved/missed: `2` / `0` / `0` / `2` (`+0` missed)" in rendered
        assert "Direct suggestion follow rate: `25.0%` (`-25.0pp`)" in rendered
        assert "Latest run had no external-fallback regression" in rendered


def test_trend_summary_bridges_tool_usage_and_runtime_audit_coverage() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-productivity-bridge-") as raw_tmp:
        temp_root = Path(raw_tmp)
        runs_dir = temp_root / "runs"
        history_dir = temp_root / "history"
        output_path = temp_root / "summary.md"
        write_tool_usage(runs_dir / "20260707-110000" / "tool-usage.json", 31, 0.0)
        history_dir.mkdir()
        history_dir.joinpath("eval-session-audit-20260707-110000.json").write_text(
            (
                '{"audit_pass_rate":{'
                '"builder_session_count":0,'
                '"planner_session_count":2,'
                '"top_failed_checks":[{"code":"read_side_evidence","count":1}]'
                "}}\n"
            ),
            encoding="utf-8",
        )

        proc = run_command(
            [
                "python3",
                str(TREND_SCRIPT),
                "--input-dir",
                str(runs_dir),
                "--audit-history-dir",
                str(history_dir),
                "--output",
                str(output_path),
            ]
        )

        assert proc.returncode == 0, (
            "trend summary should explain audit/tool-usage coverage mismatch: "
            f"stdout={proc.stdout} stderr={proc.stderr}"
        )
        rendered = output_path.read_text(encoding="utf-8")
        assert "Runtime builder audit sessions: `0`" in rendered
        assert "Telemetry mutation tool events: `1`" in rendered
        assert (
            "Build-lane signal mismatch: telemetry saw mutation events, "
            "but runtime audit saw no applicable builder session."
        ) in rendered
        assert "Top audit check: `read_side_evidence` in `1` session(s)" in rendered


def test_trend_summary_rejects_legacy_telemetry_as_productivity_evidence() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-productivity-provenance-") as raw_tmp:
        temp_root = Path(raw_tmp)
        runs_dir = temp_root / "runs"
        output_path = temp_root / "summary.md"
        usage_path = runs_dir / "20260707-110000" / "tool-usage.json"
        write_tool_usage(usage_path, 31, 0.0)
        report = json.loads(usage_path.read_text(encoding="utf-8"))
        report["behavior"]["provenance"] = {
            "status": "unverified",
            "runtime_events": 0,
            "legacy_unverified_events": 31,
        }
        usage_path.write_text(json.dumps(report), encoding="utf-8")

        proc = run_command(
            [
                "python3",
                str(TREND_SCRIPT),
                "--input-dir",
                str(runs_dir),
                "--output",
                str(output_path),
            ]
        )

        assert proc.returncode == 0, (
            "trend summary should render legacy telemetry provenance: "
            f"stdout={proc.stdout} stderr={proc.stderr}"
        )
        rendered = output_path.read_text(encoding="utf-8")

    assert "Attribution status: `unverified`" in rendered
    assert "cannot support a productivity claim" in rendered


def test_trend_summary_rejects_runtime_smoke_only_as_productivity_evidence() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-productivity-smoke-only-") as raw_tmp:
        temp_root = Path(raw_tmp)
        runs_dir = temp_root / "runs"
        output_path = temp_root / "summary.md"
        usage_path = runs_dir / "20260707-110000" / "tool-usage.json"
        write_tool_usage(usage_path, 0, 0.0)
        report = json.loads(usage_path.read_text(encoding="utf-8"))
        report["behavior"]["provenance"] = {
            "status": "smoke_only",
            "evidence_status": "smoke_only",
            "runtime_events": 2,
            "host_runtime_events": 0,
            "unattributed_runtime_events": 2,
            "host_runtime_event_counts": [],
            "legacy_unverified_events": 0,
        }
        usage_path.write_text(json.dumps(report), encoding="utf-8")

        proc = run_command(
            [
                "python3",
                str(TREND_SCRIPT),
                "--input-dir",
                str(runs_dir),
                "--output",
                str(output_path),
            ]
        )

        assert proc.returncode == 0, (
            "trend summary should render runtime smoke provenance: "
            f"stdout={proc.stdout} stderr={proc.stderr}"
        )
        rendered = output_path.read_text(encoding="utf-8")

    assert "Attribution status: `smoke_only`" in rendered
    assert "unattributed runtime activity" in rendered
    assert "cannot support a productivity claim" in rendered


def main() -> int:
    tests = [
        test_print_plan_honors_explicit_telemetry_path_without_writing,
        test_export_audit_default_matches_repo_local_readonly_daemon,
        test_trend_summary_reports_latest_delta_against_previous_run,
        test_trend_summary_bridges_tool_usage_and_runtime_audit_coverage,
        test_trend_summary_rejects_legacy_telemetry_as_productivity_evidence,
        test_trend_summary_rejects_runtime_smoke_only_as_productivity_evidence,
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
