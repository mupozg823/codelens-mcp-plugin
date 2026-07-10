#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# 1. Install uv (if not installed):
#      curl -LsSf https://astral.sh/uv/install.sh | sh
# 2. Run directly:
#      uv run scripts/summarize-productivity-proof-runs.py
# 3. CI can also run it with system Python:
#      python3 scripts/summarize-productivity-proof-runs.py
# ------------------

from __future__ import annotations

import argparse
import json
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Final, Mapping, Sequence, TypeAlias

DEFAULT_INPUT_DIR: Final = Path(".codelens/reports/productivity/runs")
DEFAULT_AUDIT_HISTORY_DIR: Final = Path(".codelens/reports/productivity/history")
JsonValue: TypeAlias = str | int | float | bool | None | Mapping[str, "JsonValue"] | Sequence["JsonValue"]
JsonMap: TypeAlias = Mapping[str, JsonValue]


@dataclass(frozen=True, slots=True)
class ProductivityMetrics:
    run_id: str
    total_events: int
    session_count: int
    suggestion_events: int
    suggestions_followed: int
    suggestions_missed: int
    suggestions_diverted: int
    suggestions_unresolved: int
    suggestion_follow_rate: float
    delegate_emissions: int
    handoffs_consumed: int
    builder_tool_events: int
    provenance_status: str
    evidence_status: str
    runtime_event_count: int
    host_runtime_event_count: int
    unattributed_runtime_event_count: int
    legacy_unverified_event_count: int


@dataclass(frozen=True, slots=True)
class AuditCoverage:
    builder_session_count: int
    planner_session_count: int
    top_failed_check_code: str | None
    top_failed_check_count: int


def int_field(data: JsonMap, key: str) -> int:
    value = data.get(key)
    return value if isinstance(value, int) else 0


def float_field(data: JsonMap, key: str) -> float:
    value = data.get(key)
    return float(value) if isinstance(value, int | float) else 0.0


def str_field(data: JsonMap, key: str, default: str) -> str:
    value = data.get(key)
    return value if isinstance(value, str) else default


def load_metrics(path: Path) -> ProductivityMetrics | None:
    parsed = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(parsed, Mapping):
        return None
    behavior = parsed.get("behavior")
    if not isinstance(behavior, Mapping):
        return None
    provenance = behavior.get("provenance")
    provenance_map = provenance if isinstance(provenance, Mapping) else {}
    return ProductivityMetrics(
        run_id=path.parent.name,
        total_events=int_field(behavior, "total_events"),
        session_count=int_field(behavior, "session_count"),
        suggestion_events=int_field(behavior, "suggestion_events"),
        suggestions_followed=int_field(behavior, "suggestions_followed"),
        suggestions_missed=int_field(behavior, "suggestions_missed"),
        suggestions_diverted=int_field(behavior, "suggestions_diverted"),
        suggestions_unresolved=int_field(behavior, "suggestions_unresolved"),
        suggestion_follow_rate=float_field(behavior, "suggestion_follow_rate"),
        delegate_emissions=int_field(behavior, "delegate_emissions"),
        handoffs_consumed=int_field(behavior, "delegate_handoffs_consumed"),
        builder_tool_events=int_field(behavior, "codex_builder_tool_events"),
        provenance_status=str_field(provenance_map, "status", "unverified"),
        evidence_status=str_field(provenance_map, "evidence_status", "unknown"),
        runtime_event_count=int_field(provenance_map, "runtime_events"),
        host_runtime_event_count=int_field(provenance_map, "host_runtime_events"),
        unattributed_runtime_event_count=int_field(
            provenance_map, "unattributed_runtime_events"
        ),
        legacy_unverified_event_count=int_field(
            provenance_map, "legacy_unverified_events"
        ),
    )


def load_runs(input_dir: Path, limit: int) -> list[ProductivityMetrics]:
    runs = []
    for path in sorted(input_dir.glob("*/tool-usage.json")):
        metrics = load_metrics(path)
        if metrics is not None:
            runs.append(metrics)
    return runs[-limit:]


def load_latest_audit_coverage(audit_history_dir: Path) -> AuditCoverage | None:
    paths = sorted(audit_history_dir.glob("eval-session-audit-*.json"))
    if not paths:
        return None
    parsed = json.loads(paths[-1].read_text(encoding="utf-8"))
    if not isinstance(parsed, Mapping):
        return None
    audit = parsed.get("audit_pass_rate")
    if not isinstance(audit, Mapping):
        return None
    top_failed = audit.get("top_failed_checks")
    top_code = None
    top_count = 0
    if isinstance(top_failed, Sequence) and not isinstance(top_failed, str) and top_failed:
        first = top_failed[0]
        if isinstance(first, Mapping):
            code = first.get("code")
            top_code = code if isinstance(code, str) else None
            top_count = int_field(first, "count")
    return AuditCoverage(
        builder_session_count=int_field(audit, "builder_session_count"),
        planner_session_count=int_field(audit, "planner_session_count"),
        top_failed_check_code=top_code,
        top_failed_check_count=top_count,
    )


def delta_int(latest: int, previous: int | None) -> str:
    if previous is None:
        return "n/a"
    delta = latest - previous
    sign = "+" if delta >= 0 else ""
    return f"{sign}{delta}"


def delta_rate(latest: float, previous: float | None) -> str:
    if previous is None:
        return "n/a"
    delta = (latest - previous) * 100
    sign = "+" if delta >= 0 else ""
    return f"{sign}{delta:.1f}pp"


def pct(value: float) -> str:
    return f"{value * 100:.1f}%"


def render_audit_bridge(latest: ProductivityMetrics, coverage: AuditCoverage | None) -> list[str]:
    if coverage is None:
        return ["- Audit coverage: `n/a`"]
    lines = [
        f"- Runtime builder audit sessions: `{coverage.builder_session_count}`",
        f"- Runtime planner audit sessions: `{coverage.planner_session_count}`",
        f"- Telemetry builder tool events: `{latest.builder_tool_events}`",
    ]
    if latest.builder_tool_events > 0 and coverage.builder_session_count == 0:
        lines.append(
            "- Builder signal mismatch: telemetry saw builder-like tool events, but runtime audit saw no applicable builder session."
        )
    if coverage.top_failed_check_code is not None:
        lines.append(
            f"- Top audit check: `{coverage.top_failed_check_code}` in `{coverage.top_failed_check_count}` session(s)"
        )
    return lines


def render_markdown(
    runs: list[ProductivityMetrics],
    input_dir: Path,
    audit_coverage: AuditCoverage | None,
) -> str:
    if not runs:
        raise SystemExit(f"no productivity tool-usage snapshots found under {input_dir}")
    latest = runs[-1]
    previous = runs[-2] if len(runs) >= 2 else None
    previous_id = previous.run_id if previous is not None else "n/a"
    previous_events = previous.total_events if previous is not None else None
    previous_sessions = previous.session_count if previous is not None else None
    previous_follow_rate = previous.suggestion_follow_rate if previous is not None else None
    previous_missed = previous.suggestions_missed if previous is not None else None
    previous_builder = previous.builder_tool_events if previous is not None else None
    lines = [
        "# CodeLens productivity trend summary",
        "",
        f"- Generated at: `{datetime.now(UTC).isoformat()}`",
        f"- Input dir: `{input_dir}`",
        f"- Runs analyzed: `{len(runs)}`",
        f"- Latest run: `{latest.run_id}`",
        f"- Previous run: `{previous_id}`",
        "",
        "## Latest Delta",
        "",
        f"- Tool events: `{latest.total_events}` (`{delta_int(latest.total_events, previous_events)}`)",
        f"- Sessions: `{latest.session_count}` (`{delta_int(latest.session_count, previous_sessions)}`)",
        f"- Suggestion events: `{latest.suggestion_events}`",
        f"- Suggestions followed/diverted/unresolved/missed: `{latest.suggestions_followed}` / `{latest.suggestions_diverted}` / `{latest.suggestions_unresolved}` / `{latest.suggestions_missed}` (`{delta_int(latest.suggestions_missed, previous_missed)}` missed)",
        f"- Direct suggestion follow rate: `{pct(latest.suggestion_follow_rate)}` (`{delta_rate(latest.suggestion_follow_rate, previous_follow_rate)}`)",
        f"- Delegate emissions / handoffs consumed: `{latest.delegate_emissions}` / `{latest.handoffs_consumed}`",
        f"- Builder tool events: `{latest.builder_tool_events}` (`{delta_int(latest.builder_tool_events, previous_builder)}`)",
        "",
        "## Telemetry Provenance",
        "",
        f"- Attribution status: `{latest.provenance_status}`",
        f"- Productivity evidence: `{latest.evidence_status}`",
        f"- Runtime host-attributed / unattributed / legacy-unverified events: `{latest.host_runtime_event_count}` / `{latest.unattributed_runtime_event_count}` / `{latest.legacy_unverified_event_count}`",
        "",
        "## Audit Coverage Bridge",
        "",
        *render_audit_bridge(latest, audit_coverage),
        "",
        "## Interpretation",
        "",
    ]
    if latest.evidence_status in {"unverified", "unknown"}:
        lines.append(
            "- Latest telemetry is unverified and cannot support a productivity claim; collect runtime-marked events before comparing trends."
        )
    elif latest.evidence_status == "smoke_only":
        lines.append(
            "- Latest telemetry contains unattributed runtime activity, not host-attributed agent activity, and cannot support a productivity claim."
        )
    elif latest.evidence_status == "bootstrap_only":
        lines.append(
            "- Latest telemetry verifies host attribution only for bootstrap traffic and cannot support a productivity claim; collect a task tool call."
        )
    elif previous is None:
        lines.append("- Only one task-observed run is available; collect more runs before claiming trend improvement.")
    elif latest.suggestions_missed > previous.suggestions_missed:
        lines.append("- Missed suggestions increased; inspect `tool-usage.txt` before claiming improvement.")
    else:
        lines.append("- Latest run had no external-fallback regression; direct follow rate alone is not a productivity result.")
    if latest.builder_tool_events == 0:
        lines.append("- Builder coverage is still absent in tool telemetry.")
    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input-dir", type=Path, default=DEFAULT_INPUT_DIR)
    parser.add_argument("--audit-history-dir", type=Path, default=DEFAULT_AUDIT_HISTORY_DIR)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--limit", type=int, default=14)
    args = parser.parse_args()
    if args.limit <= 0:
        raise SystemExit("--limit must be positive")
    rendered = render_markdown(
        load_runs(args.input_dir, args.limit),
        args.input_dir,
        load_latest_audit_coverage(args.audit_history_dir),
    )
    if args.output is None:
        print(rendered, end="")
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
        print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
