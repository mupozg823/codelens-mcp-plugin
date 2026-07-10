#!/usr/bin/env python3
"""Tests for manifest-only study cohort reporting."""

from __future__ import annotations

import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_report_runner as runner


def write_manifest(
    root: Path,
    name: str,
    condition: str,
    task_kind: str,
    tokens: int | None,
    codelens_calls: int,
) -> None:
    path = root / name
    path.mkdir()
    payload = {
        "identity": {
            "study_id": "pilot-v1",
            "scenario_id": "repo::repair::001" if task_kind != "simple-local-lookup" else "repo::lookup::001",
            "task_kind": task_kind,
            "agent": "codex",
            "condition": condition,
            "index_mode": "warm",
        },
        "quality_status": "passed",
        "result": {
            "wall_ms": 800,
            "agent_usage": {"status": "available" if tokens is not None else "unavailable", "total_tokens": tokens},
            "agent_activity": {"codelens_calls": codelens_calls, "file_write_events": 1, "revisited_write_paths": 0},
            "mcp_metrics": {"context_tokens": 10, "daemon_cpu_ms": 100, "peak_rss_bytes": 100, "tool_latency_p50_ms": 12, "tool_latency_p95_ms": 20},
        },
    }
    (path / "run-manifest.json").write_text(json.dumps(payload), encoding="utf-8")


def test_report_reads_only_manifests_and_exposes_required_lanes() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-report-") as raw_tmp:
        root = Path(raw_tmp)
        write_manifest(root, "baseline", "baseline", "defect-repair", 100, 0)
        write_manifest(root, "routed", "routed-on", "defect-repair", 70, 2)
        write_manifest(root, "lookup", "routed-on", "simple-local-lookup", 10, 0)

        report = runner.build_study_report(root, minimum_complex_pairs=1, minimum_simple_runs=1)

    assert report["run_count"] == 3
    assert report["complex_gate"]["status"] == "passed"
    assert report["simple_lookup_gate"]["status"] == "passed"
    assert report["condition_summaries"]["routed-on"]["tool_latency_p95_ms"]["median"] == 20
    assert report["condition_summaries"]["routed-on"]["revisited_write_paths"]["available"] == 2


def test_missing_agent_usage_becomes_coverage_gap() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-report-") as raw_tmp:
        root = Path(raw_tmp)
        write_manifest(root, "baseline", "baseline", "defect-repair", 100, 0)
        write_manifest(root, "routed", "routed-on", "defect-repair", None, 2)

        report = runner.build_study_report(root, minimum_complex_pairs=1, minimum_simple_runs=1)

    assert report["complex_gate"]["status"] == "coverage-gap"


def main() -> int:
    tests = [
        test_report_reads_only_manifests_and_exposes_required_lanes,
        test_missing_agent_usage_becomes_coverage_gap,
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
