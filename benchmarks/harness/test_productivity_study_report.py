#!/usr/bin/env python3
"""Tests for productivity-study quality and Pareto gates."""

from __future__ import annotations

import sys
from dataclasses import replace
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_report as report


def measured_run(
    *,
    run_id: str,
    condition: report.Condition,
    accepted: bool = True,
    tokens: int = 100,
    wall_ms: int = 100,
    context_tokens: int = 100,
    cpu_ms: int = 100,
    peak_rss_bytes: int = 100,
    pair_key: str = "codex::impact::task-1::warm",
    index_mode: report.IndexMode = report.IndexMode.WARM,
) -> report.StudyObservation:
    return report.StudyObservation(
        run_id=run_id,
        pair_key=pair_key,
        condition=condition,
        quality=report.QualityVerdict.PASSED if accepted else report.QualityVerdict.FAILED,
        agent_total_tokens=tokens,
        wall_ms=wall_ms,
        mcp_context_tokens=context_tokens,
        daemon_cpu_ms=cpu_ms,
        peak_rss_bytes=peak_rss_bytes,
        codelens_calls=1,
        index_mode=index_mode,
    )


def test_pareto_gate_passes_when_tokens_improve_without_other_regression() -> None:
    baseline = measured_run(run_id="baseline", condition=report.Condition.BASELINE)
    routed = measured_run(
        run_id="routed",
        condition=report.Condition.ROUTED,
        tokens=75,
        wall_ms=105,
        context_tokens=108,
        cpu_ms=109,
        peak_rss_bytes=110,
    )

    gate = report.evaluate_complex_gate([baseline, routed], minimum_pairs=1)

    assert gate.status is report.GateStatus.PASSED
    assert gate.median_token_ratio == 0.75


def test_pareto_gate_rejects_quality_drop_even_when_tokens_improve() -> None:
    baseline = measured_run(run_id="baseline", condition=report.Condition.BASELINE)
    routed = measured_run(
        run_id="routed",
        condition=report.Condition.ROUTED,
        accepted=False,
        tokens=50,
    )

    gate = report.evaluate_complex_gate([baseline, routed], minimum_pairs=1)

    assert gate.status is report.GateStatus.FAILED
    assert "quality" in gate.reasons[0]


def test_pareto_gate_returns_coverage_gap_when_metric_is_unavailable() -> None:
    baseline = measured_run(run_id="baseline", condition=report.Condition.BASELINE)
    routed = measured_run(run_id="routed", condition=report.Condition.ROUTED)
    routed = replace(routed, daemon_cpu_ms=None)

    gate = report.evaluate_complex_gate([baseline, routed], minimum_pairs=1)

    assert gate.status is report.GateStatus.COVERAGE_GAP
    assert "daemon_cpu_ms" in gate.reasons[0]


def test_withheld_blind_quality_is_a_coverage_gap_not_a_regression() -> None:
    baseline = measured_run(run_id="baseline", condition=report.Condition.BASELINE)
    routed = measured_run(run_id="routed", condition=report.Condition.ROUTED)
    routed = replace(routed, quality=report.QualityVerdict.WITHHELD)

    gate = report.evaluate_complex_gate([baseline, routed], minimum_pairs=1)

    assert gate.status is report.GateStatus.COVERAGE_GAP
    assert "quality" in gate.reasons[0]


def test_simple_lookup_requires_routed_mode_to_stay_native() -> None:
    routed = measured_run(run_id="routed", condition=report.Condition.ROUTED)
    routed = replace(routed, codelens_calls=0)

    gate = report.evaluate_simple_lookup_gate([routed], minimum_runs=1)

    assert gate.status is report.GateStatus.PASSED


def test_complex_gate_excludes_cold_start_from_primary_comparison() -> None:
    cold_baseline = measured_run(
        run_id="cold-baseline",
        condition=report.Condition.BASELINE,
        pair_key="codex::impact::task-1::cold",
        index_mode=report.IndexMode.COLD,
    )
    cold_routed = measured_run(
        run_id="cold-routed",
        condition=report.Condition.ROUTED,
        tokens=50,
        pair_key="codex::impact::task-1::cold",
        index_mode=report.IndexMode.COLD,
    )
    warm_baseline = measured_run(
        run_id="warm-baseline",
        condition=report.Condition.BASELINE,
        pair_key="codex::impact::task-1::warm",
    )
    warm_routed = measured_run(
        run_id="warm-routed",
        condition=report.Condition.ROUTED,
        tokens=100,
        pair_key="codex::impact::task-1::warm",
    )

    gate = report.evaluate_complex_gate(
        [cold_baseline, cold_routed, warm_baseline, warm_routed], minimum_pairs=1
    )

    assert gate.status is report.GateStatus.FAILED
    assert gate.pair_count == 1


def test_blind_review_disagreement_withholds_quality_verdict() -> None:
    reviews = (
        report.BlindReview(report.Agent.CODEX, True),
        report.BlindReview(report.Agent.CLAUDE, False),
    )

    verdict = report.resolve_blind_reviews(reviews)

    assert verdict is report.QualityVerdict.WITHHELD


def test_two_independent_blind_reviews_must_both_pass() -> None:
    reviews = (
        report.BlindReview(report.Agent.CODEX, True),
        report.BlindReview(report.Agent.CLAUDE, True),
    )

    verdict = report.resolve_blind_reviews(reviews)

    assert verdict is report.QualityVerdict.PASSED


def main() -> int:
    tests = [
        test_pareto_gate_passes_when_tokens_improve_without_other_regression,
        test_pareto_gate_rejects_quality_drop_even_when_tokens_improve,
        test_pareto_gate_returns_coverage_gap_when_metric_is_unavailable,
        test_withheld_blind_quality_is_a_coverage_gap_not_a_regression,
        test_simple_lookup_requires_routed_mode_to_stay_native,
        test_complex_gate_excludes_cold_start_from_primary_comparison,
        test_blind_review_disagreement_withholds_quality_verdict,
        test_two_independent_blind_reviews_must_both_pass,
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
