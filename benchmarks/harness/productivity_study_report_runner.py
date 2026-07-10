"""Manifest-only cohort report for productivity-study-v1."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from statistics import median

from productivity_study_contract import Condition, IndexMode
from productivity_study_report import (
    GateResult,
    GateStatus,
    QualityVerdict,
    StudyObservation,
    evaluate_complex_gate,
    evaluate_simple_lookup_gate,
)

SUMMARY_METRICS = (
    ("agent_total_tokens", ("result", "agent_usage", "total_tokens")),
    ("wall_ms", ("result", "wall_ms")),
    ("mcp_context_tokens", ("result", "mcp_metrics", "context_tokens")),
    ("daemon_cpu_ms", ("result", "mcp_metrics", "daemon_cpu_ms")),
    ("peak_rss_bytes", ("result", "mcp_metrics", "peak_rss_bytes")),
    ("tool_latency_p50_ms", ("result", "mcp_metrics", "tool_latency_p50_ms")),
    ("tool_latency_p95_ms", ("result", "mcp_metrics", "tool_latency_p95_ms")),
    ("file_write_events", ("result", "agent_activity", "file_write_events")),
    ("revisited_write_paths", ("result", "agent_activity", "revisited_write_paths")),
)


def load_manifests(root: Path) -> tuple[dict[str, object], ...]:
    manifests: list[dict[str, object]] = []
    for path in sorted(root.rglob("run-manifest.json")):
        decoded: object = json.loads(path.read_text(encoding="utf-8"))
        if isinstance(decoded, dict):
            manifests.append(decoded)
    return tuple(manifests)


def build_study_report(
    root: Path, *, minimum_complex_pairs: int, minimum_simple_runs: int
) -> dict[str, object]:
    manifests = load_manifests(root)
    observations = [observation_from_manifest(manifest) for manifest in manifests]
    complex_rows = [
        row for row, manifest in zip(observations, manifests, strict=True)
        if task_kind(manifest) != "simple-local-lookup"
    ]
    simple_rows = [
        row for row, manifest in zip(observations, manifests, strict=True)
        if task_kind(manifest) == "simple-local-lookup"
    ]
    complex_gates = gates_by_agent(complex_rows, True, minimum_complex_pairs)
    simple_gates = gates_by_agent(simple_rows, False, minimum_simple_runs)
    return {
        "schema_version": "productivity-study-report-v1",
        "study_id": study_id_for(manifests),
        "run_count": len(manifests),
        "condition_summaries": condition_summaries(manifests),
        "complex_gate": gate_payload(combine_gates(complex_gates)),
        "complex_gates_by_agent": {agent: gate_payload(gate) for agent, gate in complex_gates.items()},
        "simple_lookup_gate": gate_payload(combine_gates(simple_gates)),
        "simple_lookup_gates_by_agent": {agent: gate_payload(gate) for agent, gate in simple_gates.items()},
    }


def observation_from_manifest(manifest: dict[str, object]) -> StudyObservation:
    identity = object_at(manifest, ("identity",))
    result = object_at(manifest, ("result",))
    condition = enum_value(Condition, string_at(identity, "condition"))
    index_mode = enum_value(IndexMode, string_at(identity, "index_mode"))
    usage = object_at(result, ("agent_usage",))
    activity = object_at(result, ("agent_activity",))
    metrics = object_at(result, ("mcp_metrics",))
    agent_tokens = integer_at(usage, "total_tokens") if usage.get("status") == "available" else None
    return StudyObservation(
        run_id=string_at(identity, "scenario_id"),
        pair_key=f"{string_at(identity, 'scenario_id')}::{string_at(identity, 'agent')}",
        condition=condition,
        quality=quality_from_manifest(manifest),
        agent_total_tokens=agent_tokens,
        wall_ms=integer_at(result, "wall_ms"),
        mcp_context_tokens=integer_at(metrics, "context_tokens"),
        daemon_cpu_ms=integer_at(metrics, "daemon_cpu_ms"),
        peak_rss_bytes=integer_at(metrics, "peak_rss_bytes"),
        codelens_calls=integer_at(activity, "codelens_calls") or 0,
        index_mode=index_mode,
    )


def condition_summaries(manifests: tuple[dict[str, object], ...]) -> dict[str, object]:
    summaries: dict[str, object] = {}
    for condition in Condition:
        rows = [
            manifest for manifest in manifests
            if string_at(object_at(manifest, ("identity",)), "condition") == condition.value
        ]
        summary: dict[str, object] = {"run_count": len(rows)}
        for name, path in SUMMARY_METRICS:
            values = [value for row in rows if (value := integer_path(row, path)) is not None]
            summary[name] = {"available": len(values), "median": int(median(values)) if values else None}
        summaries[condition.value] = summary
    return summaries


def gates_by_agent(
    observations: list[StudyObservation],
    complex_task: bool,
    minimum: int,
) -> dict[str, GateResult]:
    agents = sorted({row.pair_key.rpartition("::")[2] for row in observations})
    gates: dict[str, GateResult] = {}
    for agent in agents:
        rows = [row for row in observations if row.pair_key.endswith(f"::{agent}")]
        if complex_task:
            gates[agent] = evaluate_complex_gate(rows, minimum_pairs=minimum)
        else:
            gates[agent] = evaluate_simple_lookup_gate(rows, minimum_runs=minimum)
    return gates


def combine_gates(gates: dict[str, GateResult]) -> GateResult:
    if not gates:
        return GateResult(GateStatus.COVERAGE_GAP, ("no agent observations",), 0, None, None)
    results = tuple(gates.values())
    status = GateStatus.PASSED
    if any(result.status is GateStatus.FAILED for result in results):
        status = GateStatus.FAILED
    elif any(result.status is GateStatus.COVERAGE_GAP for result in results):
        status = GateStatus.COVERAGE_GAP
    reasons = tuple(f"{agent}: {reason}" for agent, result in gates.items() for reason in result.reasons)
    return GateResult(status, reasons, sum(result.pair_count for result in results), None, None)


def gate_payload(gate: GateResult) -> dict[str, object]:
    return {
        "status": gate.status.value,
        "reasons": list(gate.reasons),
        "pair_count": gate.pair_count,
        "median_token_ratio": gate.median_token_ratio,
        "median_wall_ratio": gate.median_wall_ratio,
    }


def quality_from_manifest(manifest: dict[str, object]) -> QualityVerdict:
    value = manifest.get("quality_status")
    mapping = {
        "passed": QualityVerdict.PASSED,
        "failed": QualityVerdict.FAILED,
        "held": QualityVerdict.WITHHELD,
    }
    return mapping.get(value, QualityVerdict.UNVERIFIED)


def task_kind(manifest: dict[str, object]) -> str:
    return string_at(object_at(manifest, ("identity",)), "task_kind")


def study_id_for(manifests: tuple[dict[str, object], ...]) -> str | None:
    if not manifests:
        return None
    return string_at(object_at(manifests[0], ("identity",)), "study_id")


def integer_path(payload: dict[str, object], path: tuple[str, ...]) -> int | None:
    current: object = payload
    for key in path[:-1]:
        if not isinstance(current, dict):
            return None
        current = current.get(key)
    return integer_at(current, path[-1]) if isinstance(current, dict) else None


def object_at(payload: dict[str, object], path: tuple[str, ...]) -> dict[str, object]:
    current: object = payload
    for key in path:
        current = current.get(key) if isinstance(current, dict) else None
    return current if isinstance(current, dict) else {}


def string_at(payload: dict[str, object], key: str) -> str:
    value = payload.get(key)
    if not isinstance(value, str):
        raise ValueError(f"missing manifest string: {key}")
    return value


def integer_at(payload: dict[str, object], key: str) -> int | None:
    value = payload.get(key)
    return value if isinstance(value, int) and not isinstance(value, bool) else None


def enum_value(enum_type: type[Condition] | type[IndexMode], value: str) -> Condition | IndexMode:
    return enum_type(value)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("artifact_root", type=Path)
    parser.add_argument("--minimum-complex-pairs", type=int, required=True)
    parser.add_argument("--minimum-simple-runs", type=int, required=True)
    args = parser.parse_args()
    report = build_study_report(
        args.artifact_root,
        minimum_complex_pairs=args.minimum_complex_pairs,
        minimum_simple_runs=args.minimum_simple_runs,
    )
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
