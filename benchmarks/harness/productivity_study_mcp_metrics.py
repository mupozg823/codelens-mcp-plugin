"""Aggregate isolated-daemon telemetry without retaining event payloads."""

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path


def agent_session_ids(
    telemetry_path: Path, excluded_session_ids: tuple[str, ...]
) -> tuple[str, ...]:
    ids: set[str] = set()
    for event in read_events(telemetry_path):
        session_id = event.get("session_id")
        if isinstance(session_id, str) and session_id not in excluded_session_ids:
            ids.add(session_id)
    return tuple(sorted(ids))


def aggregate_agent_metrics(
    telemetry_path: Path,
    excluded_session_ids: tuple[str, ...],
    session_snapshot: Callable[[str], dict],
    resources: dict[str, int | None],
    daemon_startup_ms: int,
) -> dict[str, object]:
    sessions = agent_session_ids(telemetry_path, excluded_session_ids)
    events = [
        event
        for event in read_events(telemetry_path)
        if event.get("session_id") in sessions
    ]
    if not sessions or not events:
        return unavailable_metrics(resources, daemon_startup_ms)
    snapshots = tuple(session_snapshot(session_id) for session_id in sessions)
    latencies = integer_values(events, "elapsed_ms")
    return {
        "status": "available",
        "context_tokens": sum(integer_values(events, "tokens")),
        "handle_reuse_count": required_session_sum(
            snapshots, "handle_reuse_count"
        ),
        "duplicate_retrieval_count": None,
        "external_fallback_count": None,
        "tool_latency_p50_ms": percentile(latencies, 50),
        "tool_latency_p95_ms": percentile(latencies, 95),
        "daemon_cpu_ms": resources["daemon_cpu_ms"],
        "peak_rss_bytes": resources["peak_rss_bytes"],
        "daemon_startup_ms": daemon_startup_ms,
        "agent_mcp_session_count": len(sessions),
        "agent_mcp_event_count": len(events),
    }


def read_events(telemetry_path: Path) -> tuple[dict[str, object], ...]:
    if not telemetry_path.is_file():
        return ()
    events: list[dict[str, object]] = []
    for line in telemetry_path.read_text(encoding="utf-8").splitlines():
        try:
            decoded: object = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(decoded, dict):
            events.append(decoded)
    return tuple(events)


def integer_values(events: list[dict[str, object]], field: str) -> list[int]:
    values: list[int] = []
    for event in events:
        value = event.get(field)
        if isinstance(value, int):
            values.append(value)
    return values


def percentile(values: list[int], percentage: int) -> int | None:
    if not values:
        return None
    ordered = sorted(values)
    return ordered[((len(ordered) - 1) * min(percentage, 100)) // 100]


def required_session_sum(snapshots: tuple[dict, ...], field: str) -> int | None:
    if not snapshots:
        return None
    values: list[int] = []
    for snapshot in snapshots:
        session = snapshot.get("session")
        value = session.get(field) if isinstance(session, dict) else None
        if not isinstance(value, int):
            return None
        values.append(value)
    return sum(values)


def unavailable_metrics(
    resources: dict[str, int | None], daemon_startup_ms: int
) -> dict[str, object]:
    return {
        "status": "unavailable",
        "context_tokens": None,
        "handle_reuse_count": None,
        "duplicate_retrieval_count": None,
        "external_fallback_count": None,
        "tool_latency_p50_ms": None,
        "tool_latency_p95_ms": None,
        "daemon_cpu_ms": resources["daemon_cpu_ms"],
        "peak_rss_bytes": resources["peak_rss_bytes"],
        "daemon_startup_ms": daemon_startup_ms,
        "agent_mcp_session_count": 0,
        "agent_mcp_event_count": 0,
    }
