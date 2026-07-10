"""Quality-first paired gates for productivity-study-v1 cohorts."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from statistics import median

from productivity_study_contract import Agent, Condition, IndexMode


class QualityVerdict(StrEnum):
    PASSED = "passed"
    FAILED = "failed"
    UNVERIFIED = "unverified"
    WITHHELD = "withheld"


class GateStatus(StrEnum):
    PASSED = "passed"
    FAILED = "failed"
    COVERAGE_GAP = "coverage-gap"


class Metric(StrEnum):
    AGENT_TOKENS = "agent_total_tokens"
    WALL_TIME = "wall_ms"
    MCP_CONTEXT = "mcp_context_tokens"
    DAEMON_CPU = "daemon_cpu_ms"
    PEAK_RSS = "peak_rss_bytes"


@dataclass(frozen=True, slots=True)
class StudyObservation:
    run_id: str
    pair_key: str
    condition: Condition
    quality: QualityVerdict
    agent_total_tokens: int | None
    wall_ms: int | None
    mcp_context_tokens: int | None
    daemon_cpu_ms: int | None
    peak_rss_bytes: int | None
    codelens_calls: int
    index_mode: IndexMode = IndexMode.WARM


@dataclass(frozen=True, slots=True)
class BlindReview:
    reviewer: Agent
    passed: bool


@dataclass(frozen=True, slots=True)
class GateResult:
    status: GateStatus
    reasons: tuple[str, ...]
    pair_count: int
    median_token_ratio: float | None
    median_wall_ratio: float | None


def evaluate_complex_gate(
    observations: list[StudyObservation], *, minimum_pairs: int
) -> GateResult:
    observations = warm_observations(observations)
    baseline_by_key = observations_by_condition(observations, Condition.BASELINE)
    routed_by_key = observations_by_condition(observations, Condition.ROUTED)
    pair_keys = tuple(sorted(set(baseline_by_key) | set(routed_by_key)))
    if len(pair_keys) < minimum_pairs:
        return GateResult(
            status=GateStatus.COVERAGE_GAP,
            reasons=(f"paired complex coverage {len(pair_keys)} < {minimum_pairs}",),
            pair_count=len(pair_keys),
            median_token_ratio=None,
            median_wall_ratio=None,
        )

    pairs: list[tuple[StudyObservation, StudyObservation]] = []
    for key in pair_keys:
        baseline = baseline_by_key.get(key)
        routed = routed_by_key.get(key)
        if baseline is None or routed is None:
            return GateResult(
                status=GateStatus.COVERAGE_GAP,
                reasons=(f"missing matched condition for {key}",),
                pair_count=len(pairs),
                median_token_ratio=None,
                median_wall_ratio=None,
            )
        if baseline.quality is not QualityVerdict.PASSED:
            return GateResult(
                status=GateStatus.COVERAGE_GAP,
                reasons=(f"baseline quality is not passed for {key}",),
                pair_count=len(pairs),
                median_token_ratio=None,
                median_wall_ratio=None,
            )
        if routed.quality is not QualityVerdict.PASSED:
            status = (
                GateStatus.FAILED
                if routed.quality is QualityVerdict.FAILED
                else GateStatus.COVERAGE_GAP
            )
            return GateResult(
                status=status,
                reasons=(f"routed quality is not passed for {key}",),
                pair_count=len(pairs),
                median_token_ratio=None,
                median_wall_ratio=None,
            )
        unavailable = unavailable_metric_name(baseline, routed)
        if unavailable is not None:
            return GateResult(
                status=GateStatus.COVERAGE_GAP,
                reasons=(f"{unavailable} unavailable for {key}",),
                pair_count=len(pairs),
                median_token_ratio=None,
                median_wall_ratio=None,
            )
        pairs.append((baseline, routed))

    token_ratio = median_ratio(pairs, Metric.AGENT_TOKENS)
    wall_ratio = median_ratio(pairs, Metric.WALL_TIME)
    context_ratio = median_ratio(pairs, Metric.MCP_CONTEXT)
    cpu_ratio = median_ratio(pairs, Metric.DAEMON_CPU)
    rss_ratio = median_ratio(pairs, Metric.PEAK_RSS)
    improved = token_ratio <= 0.8 or wall_ratio <= 0.8
    protected = all(ratio <= 1.1 for ratio in (token_ratio, wall_ratio, context_ratio, cpu_ratio, rss_ratio))
    if improved and protected:
        return GateResult(
            status=GateStatus.PASSED,
            reasons=(),
            pair_count=len(pairs),
            median_token_ratio=token_ratio,
            median_wall_ratio=wall_ratio,
        )
    reasons = pareto_failure_reasons(
        token_ratio,
        wall_ratio,
        context_ratio,
        cpu_ratio,
        rss_ratio,
    )
    return GateResult(
        status=GateStatus.FAILED,
        reasons=reasons,
        pair_count=len(pairs),
        median_token_ratio=token_ratio,
        median_wall_ratio=wall_ratio,
    )


def evaluate_simple_lookup_gate(
    observations: list[StudyObservation], *, minimum_runs: int
) -> GateResult:
    routed = [
        row
        for row in warm_observations(observations)
        if row.condition is Condition.ROUTED
    ]
    if len(routed) < minimum_runs:
        return GateResult(
            status=GateStatus.COVERAGE_GAP,
            reasons=(f"simple lookup coverage {len(routed)} < {minimum_runs}",),
            pair_count=len(routed),
            median_token_ratio=None,
            median_wall_ratio=None,
        )
    for row in routed:
        if row.quality is not QualityVerdict.PASSED:
            status = (
                GateStatus.FAILED
                if row.quality is QualityVerdict.FAILED
                else GateStatus.COVERAGE_GAP
            )
            return GateResult(
                status=status,
                reasons=(f"simple lookup quality is not passed for {row.run_id}",),
                pair_count=len(routed),
                median_token_ratio=None,
                median_wall_ratio=None,
            )
        if row.codelens_calls != 0:
            return GateResult(
                status=GateStatus.FAILED,
                reasons=(f"simple lookup used CodeLens for {row.run_id}",),
                pair_count=len(routed),
                median_token_ratio=None,
                median_wall_ratio=None,
            )
    return GateResult(
        status=GateStatus.PASSED,
        reasons=(),
        pair_count=len(routed),
        median_token_ratio=None,
        median_wall_ratio=None,
    )


def observations_by_condition(
    observations: list[StudyObservation], condition: Condition
) -> dict[str, StudyObservation]:
    return {
        row.pair_key: row for row in observations if row.condition is condition
    }


def warm_observations(observations: list[StudyObservation]) -> list[StudyObservation]:
    return [row for row in observations if row.index_mode is IndexMode.WARM]


def resolve_blind_reviews(reviews: tuple[BlindReview, ...]) -> QualityVerdict:
    decisions = {review.reviewer: review.passed for review in reviews}
    if set(decisions) != {Agent.CODEX, Agent.CLAUDE}:
        return QualityVerdict.WITHHELD
    if len(reviews) != len(decisions):
        return QualityVerdict.WITHHELD
    if len(set(decisions.values())) != 1:
        return QualityVerdict.WITHHELD
    return QualityVerdict.PASSED if decisions[Agent.CODEX] else QualityVerdict.FAILED


def unavailable_metric_name(
    baseline: StudyObservation, routed: StudyObservation
) -> str | None:
    for metric in Metric:
        if metric_value(baseline, metric) is None or metric_value(routed, metric) is None:
            return metric.value
    return None


def median_ratio(
    pairs: list[tuple[StudyObservation, StudyObservation]], metric: Metric
) -> float:
    ratios: list[float] = []
    for baseline, routed in pairs:
        baseline_value = metric_value(baseline, metric)
        routed_value = metric_value(routed, metric)
        assert isinstance(baseline_value, int)
        assert isinstance(routed_value, int)
        if baseline_value <= 0:
            return float("inf")
        ratios.append(routed_value / baseline_value)
    return float(median(ratios))


def metric_value(row: StudyObservation, metric: Metric) -> int | None:
    match metric:
        case Metric.AGENT_TOKENS:
            return row.agent_total_tokens
        case Metric.WALL_TIME:
            return row.wall_ms
        case Metric.MCP_CONTEXT:
            return row.mcp_context_tokens
        case Metric.DAEMON_CPU:
            return row.daemon_cpu_ms
        case Metric.PEAK_RSS:
            return row.peak_rss_bytes


def pareto_failure_reasons(
    token_ratio: float,
    wall_ratio: float,
    context_ratio: float,
    cpu_ratio: float,
    rss_ratio: float,
) -> tuple[str, ...]:
    reasons: list[str] = []
    if token_ratio > 0.8 and wall_ratio > 0.8:
        reasons.append("neither tokens nor wall time improved by 20%")
    for label, ratio in (
        ("agent_total_tokens", token_ratio),
        ("wall_ms", wall_ratio),
        ("mcp_context_tokens", context_ratio),
        ("daemon_cpu_ms", cpu_ratio),
        ("peak_rss_bytes", rss_ratio),
    ):
        if ratio > 1.1:
            reasons.append(f"{label} regressed by more than 10%")
    return tuple(reasons)
