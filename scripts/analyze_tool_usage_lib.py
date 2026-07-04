from __future__ import annotations

import json
from collections import Counter, defaultdict
from pathlib import Path

from analyze_tool_usage_branches import agent_branch
from analyze_tool_usage_efficiency import (
    branch_transfers,
    external_transfer,
    summarize_transfers,
    top_transfer_rows,
)
from analyze_tool_usage_routes import missed_route_label

DEFAULT_TELEMETRY_PATH = Path(".codelens/telemetry/tool_usage.jsonl")
DEFAULT_MANIFEST_PATH = Path("docs/generated/surface-manifest.json")
DELEGATE_TOOL = "delegate_to_codex_builder"
FOLLOW_WINDOW = 5


def str_value(value) -> str | None:
    return value if isinstance(value, str) else None


def int_value(value, default: int = 0) -> int:
    return value if isinstance(value, int) else default


def bool_value(value) -> bool:
    return value if isinstance(value, bool) else False


def string_list(value) -> list[str]:
    if not isinstance(value, list):
        return []
    return [item for item in value if isinstance(item, str)]


def tool_name(event: dict) -> str:
    return str_value(event.get("tool")) or "<unknown>"


def session_id(event: dict) -> str:
    return str_value(event.get("session_id")) or "<none>"


def event_index(event: dict) -> int:
    return int_value(event.get("_index"))


def load_telemetry(path: Path) -> list[dict]:
    events: list[dict] = []
    with path.open(encoding="utf-8") as handle:
        for line in handle:
            stripped = line.strip()
            if not stripped:
                continue
            parsed = json.loads(stripped)
            if isinstance(parsed, dict):
                parsed["_index"] = len(events)
                events.append(parsed)
    return events


def load_manifest_executors(path: Path) -> dict[str, str]:
    if not path.exists():
        return {}
    with path.open(encoding="utf-8") as handle:
        parsed = json.load(handle)
    if not isinstance(parsed, dict):
        return {}
    registry = parsed.get("tool_registry")
    if not isinstance(registry, dict):
        return {}
    tools = registry.get("tools")
    if not isinstance(tools, list):
        return {}
    executors: dict[str, str] = {}
    for tool in tools:
        if not isinstance(tool, dict):
            continue
        name = str_value(tool.get("name"))
        executor = str_value(tool.get("preferred_executor"))
        if name and executor:
            executors[name] = executor
    return executors


def following_session_events(event: dict, by_session: dict[str, list[dict]]) -> list[dict]:
    index = event_index(event)
    return [
        candidate
        for candidate in by_session[session_id(event)]
        if event_index(candidate) > index
    ][:FOLLOW_WINDOW]


def first_followed_tool(event: dict, following: list[dict]) -> str | None:
    suggestions = [
        tool for tool in string_list(event.get("suggested_next_tools"))
        if tool != DELEGATE_TOOL
    ]
    if not suggestions:
        return None
    for candidate in following:
        if tool_name(candidate) in suggestions:
            return tool_name(candidate)
    return None


def analyze_telemetry(events: list[dict], manifest_path: Path) -> dict:
    executors = load_manifest_executors(manifest_path)
    by_session: dict[str, list[dict]] = defaultdict(list)
    handoff_consumers: dict[str, dict] = {}
    tool_counts = Counter()
    failed_tools = Counter()

    for event in events:
        tool_counts[tool_name(event)] += 1
        by_session[session_id(event)].append(event)
        if not bool_value(event.get("success")):
            failed_tools[tool_name(event)] += 1
        handoff_id = str_value(event.get("handoff_id"))
        if handoff_id and handoff_id not in handoff_consumers:
            handoff_consumers[handoff_id] = event

    suggestion_events = [
        event for event in events if string_list(event.get("suggested_next_tools"))
    ]
    delegate_events = [
        event
        for event in events
        if DELEGATE_TOOL in string_list(event.get("suggested_next_tools"))
        or str_value(event.get("delegate_handoff_id"))
    ]
    followed = 0
    missed: list[dict] = []
    missed_labels = Counter()
    missed_branches = Counter()
    correlations: list[dict] = []

    for event in suggestion_events:
        following = following_session_events(event, by_session)
        direct_tool = first_followed_tool(event, following)
        delegate_handoff_id = str_value(event.get("delegate_handoff_id"))
        consumer = (
            handoff_consumers.get(delegate_handoff_id)
            if delegate_handoff_id is not None
            else None
        )
        delegate_followed = (
            consumer is not None and event_index(consumer) > event_index(event)
        )
        if direct_tool or delegate_followed:
            followed += 1
        else:
            next_codelens_tools = [tool_name(candidate) for candidate in following[:3]]
            next_external_tools = string_list(event.get("next_external_tools"))[:3]
            route_label = missed_route_label(next_codelens_tools, next_external_tools)
            transfer = external_transfer(event)
            branch = agent_branch(event)
            missed_labels[route_label] += 1
            missed_branches[branch] += 1
            missed.append(
                {
                    "tool": tool_name(event),
                    "session_id": session_id(event),
                    "route_label": route_label,
                    "agent_branch": branch,
                    "suggested_next_tools": string_list(event.get("suggested_next_tools")),
                    "next_codelens_tools": next_codelens_tools,
                    "next_external_tools": next_external_tools,
                    "external_transfer": transfer,
                    "branch_transfers": branch_transfers(event),
                    "source_path": str_value(event.get("source_path")),
                    "source_line": event.get("source_line"),
                }
            )

    seen_handoffs: set[str] = set()
    for event in delegate_events:
        handoff_id = str_value(event.get("delegate_handoff_id"))
        if not handoff_id or handoff_id in seen_handoffs:
            continue
        consumer = handoff_consumers.get(handoff_id)
        if consumer is None or event_index(consumer) <= event_index(event):
            continue
        seen_handoffs.add(handoff_id)
        correlations.append(
            {
                "handoff_id": handoff_id,
                "delegate_target_tool": str_value(event.get("delegate_target_tool")),
                "emitting_session": session_id(event),
                "consuming_session": session_id(consumer),
                "consuming_tool": tool_name(consumer),
            }
        )

    builder_events = [
        event for event in events
        if executors.get(tool_name(event)) == "codex-builder"
    ]
    return {
        "behavior": {
            "total_events": len(events),
            "session_count": len(by_session),
            "suggestion_events": len(suggestion_events),
            "suggestions_followed": followed,
            "suggestions_missed": len(missed),
            "suggestion_follow_rate": (
                followed / len(suggestion_events) if suggestion_events else 0.0
            ),
            "delegate_emissions": len(delegate_events),
            "delegate_handoffs_consumed": len(correlations),
            "codex_builder_tool_events": len(builder_events),
            "tool_counts": tool_counts.most_common(20),
            "missed_label_counts": missed_labels.most_common(),
            "missed_branch_counts": missed_branches.most_common(),
            "missed_transfer_by_label": summarize_transfers(missed),
            "missed_transfer_by_branch": summarize_transfers(missed, "agent_branch"),
            "top_transfer_misses": top_transfer_rows(missed),
            "handoff_correlations": correlations,
            "missed_suggestions": missed[:10],
            "top_failed_tools": failed_tools.most_common(10),
        }
    }
