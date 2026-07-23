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
# Historical telemetry rows may contain the retired synthetic action. Keep it
# only for backwards-compatible parsing; current runtime responses never emit it.
LEGACY_DELEGATE_TOOL = "delegate_to_codex_builder"
FOLLOW_WINDOW = 5
BOOTSTRAP_TOOLS = {"tools/list", "prepare_harness_session"}


def str_value(value) -> str | None:
    return value if isinstance(value, str) else None


def int_value(value, default: int = 0) -> int:
    return value if isinstance(value, int) else default


def string_list(value) -> list[str]:
    return [item for item in value if isinstance(item, str)] if isinstance(value, list) else []


def tool_name(event: dict) -> str:
    return str_value(event.get("tool")) or "<unknown>"


def session_id(event: dict) -> str:
    return str_value(event.get("session_id")) or "<none>"


def recording_origin(event: dict) -> str:
    origin = str_value(event.get("recording_origin"))
    return origin if origin in {"runtime", "test"} else "legacy_unverified"


def attributed_host_client(event: dict) -> str | None:
    normalized = (str_value(event.get("client_name")) or "").casefold()
    return "codex" if "codex" in normalized else "claude" if "claude" in normalized else None


def event_index(event: dict) -> int:
    return int_value(event.get("_index"))


def is_test_pollution(event: dict) -> bool:
    # Test rows must not influence live-host productivity evidence.
    return recording_origin(event) == "test" or session_id(event).startswith("test-session-")


def is_attributed_host_runtime_event(event: dict) -> bool:
    """Return whether a live row is bound to an initialized MCP host session."""
    return (
        recording_origin(event) == "runtime"
        and session_id(event) not in {"<none>", "local"}
        and attributed_host_client(event) is not None
    )


def load_telemetry(path: Path) -> list[dict]:
    events: list[dict] = []
    with path.open(encoding="utf-8") as handle:
        for line in handle:
            stripped = line.strip()
            if not stripped:
                continue
            parsed = json.loads(stripped)
            if isinstance(parsed, dict) and not is_test_pollution(parsed):
                parsed["_index"] = len(events)
                events.append(parsed)
    return events


def load_manifest_execution_classes(path: Path) -> dict[str, str]:
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
    execution_classes: dict[str, str] = {}
    for tool in tools:
        if not isinstance(tool, dict):
            continue
        name = str_value(tool.get("name"))
        policy = tool.get("execution_policy")
        execution_class = (
            str_value(policy.get("execution_class"))
            if isinstance(policy, dict)
            else None
        )
        if execution_class is None:
            # Read old manifests without extending the retired model-specific
            # field into newly generated reports.
            legacy_executor = str_value(tool.get("preferred_executor"))
            execution_class = "mutate" if legacy_executor == "codex-builder" else None
        if name and execution_class:
            execution_classes[name] = execution_class
    return execution_classes


def following_session_events(event: dict, by_session: dict[str, list[dict]]) -> list[dict]:
    index = event_index(event)
    return [
        candidate
        for candidate in by_session[session_id(event)]
        if event_index(candidate) > index
    ][:FOLLOW_WINDOW]


def first_followed_event(event: dict, following: list[dict]) -> dict | None:
    suggestions = [
        tool for tool in string_list(event.get("suggested_next_tools"))
        if tool != LEGACY_DELEGATE_TOOL
    ]
    if not suggestions:
        return None
    for candidate in following:
        if tool_name(candidate) in suggestions:
            return candidate
    return None


def analyze_telemetry(events: list[dict], manifest_path: Path) -> dict:
    execution_classes = load_manifest_execution_classes(manifest_path)
    productivity_events = [event for event in events if recording_origin(event) != "runtime" or is_attributed_host_runtime_event(event)]
    by_session: dict[str, list[dict]] = defaultdict(list)
    handoff_consumers: dict[str, dict] = {}
    tool_counts = Counter()
    failed_tools = Counter()
    origin_counts = Counter(recording_origin(event) for event in events)
    host_runtime_event_counts = Counter(attributed_host_client(event) for event in productivity_events if recording_origin(event) == "runtime").most_common()
    host_runtime_event_count = sum(count for _, count in host_runtime_event_counts)
    task_observed = any(is_attributed_host_runtime_event(event) and tool_name(event) not in BOOTSTRAP_TOOLS for event in events)

    for event in productivity_events:
        tool_counts[tool_name(event)] += 1
        by_session[session_id(event)].append(event)
        if event.get("success") is not True:
            failed_tools[tool_name(event)] += 1
        handoff_id = str_value(event.get("handoff_id"))
        if handoff_id and handoff_id not in handoff_consumers:
            handoff_consumers[handoff_id] = event

    suggestion_events = [
        event
        for event in productivity_events
        if string_list(event.get("suggested_next_tools"))
    ]
    delegate_events = [
        event
        for event in productivity_events
        if LEGACY_DELEGATE_TOOL in string_list(event.get("suggested_next_tools"))
        or str_value(event.get("delegate_handoff_id"))
    ]
    followed = 0
    outcome_success = 0
    outcome_error = 0
    outcome_unknown = 0
    diverted = 0
    unresolved = 0
    missed: list[dict] = []
    missed_labels = Counter()
    missed_branches = Counter()
    correlations: list[dict] = []

    for event in suggestion_events:
        following = following_session_events(event, by_session)
        direct_event = first_followed_event(event, following)
        delegate_handoff_id = str_value(event.get("delegate_handoff_id"))
        consumer = (
            handoff_consumers.get(delegate_handoff_id)
            if delegate_handoff_id is not None
            else None
        )
        delegate_followed = consumer is not None and event_index(consumer) > event_index(event)
        outcome_event = direct_event or (consumer if delegate_followed else None)
        if outcome_event is not None:
            followed += 1
            if outcome_event.get("success") is True:
                outcome_success += 1
            elif outcome_event.get("success") is False:
                outcome_error += 1
            else:
                outcome_unknown += 1
        else:
            next_codelens_tools = [tool_name(candidate) for candidate in following[:3]]
            next_external_tools = string_list(event.get("next_external_tools"))[:3]
            if not next_external_tools:
                if next_codelens_tools:
                    diverted += 1
                else:
                    unresolved += 1
                continue
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

    mutation_events = [
        event for event in productivity_events
        if execution_classes.get(tool_name(event)) == "mutate"
    ]
    suggestion_resolved = len(suggestion_events) - unresolved
    return {
        "behavior": {
            "total_events": len(productivity_events),
            "session_count": len(by_session),
            "suggestion_events": len(suggestion_events),
            "suggestions_followed": followed,
            "suggestions_missed": len(missed),
            "suggestions_diverted": diverted,
            "suggestions_unresolved": unresolved,
            "suggestion_follow_rate": followed / len(suggestion_events) if suggestion_events else 0.0,
            "suggestion_acceptance_rate": followed / suggestion_resolved if suggestion_resolved else 0.0,
            "suggestion_resolution_rate": suggestion_resolved / len(suggestion_events) if suggestion_events else 0.0,
            "suggestion_outcome_success": outcome_success,
            "suggestion_outcome_error": outcome_error,
            "suggestion_outcome_unknown": outcome_unknown,
            "suggestion_successful_outcome_rate": outcome_success / followed if followed else 0.0,
            "suggestion_value_rate": outcome_success / suggestion_resolved if suggestion_resolved else 0.0,
            "delegate_emissions": len(delegate_events),
            "delegate_handoffs_consumed": len(correlations),
            "mutation_tool_events": len(mutation_events),
            "tool_counts": tool_counts.most_common(20),
            "missed_label_counts": missed_labels.most_common(),
            "missed_branch_counts": missed_branches.most_common(),
            "missed_transfer_by_label": summarize_transfers(missed),
            "missed_transfer_by_branch": summarize_transfers(missed, "agent_branch"),
            "top_transfer_misses": top_transfer_rows(missed),
            "handoff_correlations": correlations,
            "missed_suggestions": missed[:10],
            "top_failed_tools": failed_tools.most_common(10),
            "provenance": {
                "status": (
                    "unverified"
                    if origin_counts["legacy_unverified"] > 0
                    else "verified"
                    if host_runtime_event_count > 0
                    else "smoke_only"
                    if origin_counts["runtime"] > 0
                    else "unverified"
                ),
                "evidence_status": (
                    "unverified"
                    if origin_counts["legacy_unverified"] > 0
                    else "task_observed"
                    if task_observed
                    else "bootstrap_only"
                    if host_runtime_event_count > 0
                    else "smoke_only"
                    if origin_counts["runtime"] > 0
                    else "unverified"
                ),
                "runtime_events": origin_counts["runtime"],
                "host_runtime_events": host_runtime_event_count,
                "unattributed_runtime_events": origin_counts["runtime"] - host_runtime_event_count,
                "host_runtime_event_counts": host_runtime_event_counts,
                "legacy_unverified_events": origin_counts["legacy_unverified"],
            },
        }
    }
