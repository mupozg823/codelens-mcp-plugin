from __future__ import annotations

import json
import re
from pathlib import Path

from analyze_tool_usage_branches import BRANCHES, tool_branch
from analyze_tool_usage_lib import str_value

CODELENS_PREFIX = "mcp__codelens__"
CODELENS_DOTTED_PREFIX = "mcp__codelens."
NEXT_EXTERNAL_LIMIT = 5
GENERIC_CALL_RE = re.compile(r"\[external_agent_tool_call: (?P<tool>[^\]]+)\]")
RESULT_RE = re.compile(
    r"\[external_agent_tool_result\]\n(?P<body>.*?)\n\[/external_agent_tool_result\]",
    re.S,
)
OVERFLOW_MARKERS = (
    "exceeds maximum allowed tokens",
    "Output has been saved",
)


def message_text(entry: dict) -> str:
    if entry.get("type") != "event_msg":
        return ""
    payload = entry.get("payload")
    if not isinstance(payload, dict) or payload.get("type") != "agent_message":
        return ""
    return str_value(payload.get("message")) or ""


def response_payload(entry: dict) -> dict:
    if entry.get("type") != "response_item":
        return {}
    payload = entry.get("payload")
    return payload if isinstance(payload, dict) else {}


def session_id_from_entry(entry: dict, fallback: str) -> str:
    if entry.get("type") != "session_meta":
        return fallback
    payload = entry.get("payload")
    if not isinstance(payload, dict):
        return fallback
    return str_value(payload.get("id")) or fallback


def output_text(payload: dict) -> str:
    output = payload.get("output")
    if isinstance(output, str):
        return output
    if output is None:
        return ""
    return json.dumps(output, ensure_ascii=False)


def is_codelens_tool(tool: str) -> bool:
    return tool.startswith(CODELENS_PREFIX) or tool.startswith(CODELENS_DOTTED_PREFIX)


def codelens_tool_name(tool: str) -> str:
    if tool.startswith(CODELENS_PREFIX):
        return tool[len(CODELENS_PREFIX):]
    if tool.startswith(CODELENS_DOTTED_PREFIX):
        return tool[len(CODELENS_DOTTED_PREFIX):]
    return tool


def parse_result_payload(message: str) -> dict:
    match = RESULT_RE.search(message)
    if match is None:
        return {}
    body = match.group("body").strip()
    if not body.startswith("{"):
        return {}
    try:
        parsed = json.loads(body)
    except json.JSONDecodeError:
        return {}
    return parsed if isinstance(parsed, dict) else {}


def suggested_tools_from_payload(payload: dict) -> list[str]:
    raw = payload.get("suggested_next_tools")
    if isinstance(raw, list):
        return [tool for tool in raw if isinstance(tool, str)]
    routing = payload.get("routing")
    if not isinstance(routing, dict):
        return []
    recommended = str_value(routing.get("recommended_entrypoint"))
    return [recommended] if recommended else []


def pending_event(
    tool: str,
    session_id: str,
    source_path: Path,
    line_no: int,
    index: int,
) -> dict:
    return {
        "_index": index,
        "tool": tool,
        "surface": "codex-rollout",
        "elapsed_ms": 0,
        "tokens": 0,
        "success": True,
        "truncated": False,
        "session_id": session_id,
        "source_path": str(source_path),
        "source_line": line_no,
    }


def attach_result(event: dict, payload: dict) -> None:
    event["success"] = payload.get("success", True) is not False
    suggestions = suggested_tools_from_payload(payload)
    if suggestions:
        event["suggested_next_tools"] = suggestions
    handoff_id = str_value(payload.get("handoff_id"))
    if handoff_id:
        event["handoff_id"] = handoff_id
    delegate_handoff_id = str_value(payload.get("delegate_handoff_id"))
    if delegate_handoff_id:
        event["delegate_handoff_id"] = delegate_handoff_id
    delegate_target_tool = str_value(payload.get("delegate_target_tool"))
    if delegate_target_tool:
        event["delegate_target_tool"] = delegate_target_tool


def attach_external_tool(event: dict, tool: str) -> None:
    add_int_metric(event, "next_external_tool_count", 1)
    current = event.setdefault("next_external_tools", [])
    if isinstance(current, list) and len(current) < NEXT_EXTERNAL_LIMIT:
        current.append(tool)


def add_int_metric(event: dict, key: str, delta: int) -> None:
    current = event.get(key)
    event[key] = (current if isinstance(current, int) else 0) + delta


def add_branch_count(event: dict, branch: str) -> None:
    if branch not in BRANCHES:
        return
    add_int_metric(event, f"next_{branch}_tool_count", 1)
    counts = event.setdefault("next_branch_counts", {})
    if not isinstance(counts, dict):
        counts = {}
        event["next_branch_counts"] = counts
    current = counts.get(branch)
    counts[branch] = (current if isinstance(current, int) else 0) + 1


def add_branch_metric(event: dict, branch: str, suffix: str, delta: int) -> None:
    if branch in BRANCHES:
        add_int_metric(event, f"next_{branch}_{suffix}", delta)


def attach_external_call(event: dict, tool: str, message: str, branch: str) -> None:
    attach_external_tool(event, tool)
    add_branch_count(event, branch)
    add_int_metric(event, "next_external_call_chars", len(message))
    add_branch_metric(event, branch, "call_chars", len(message))


def attach_external_result(event: dict, message: str, branch: str) -> None:
    add_int_metric(event, "next_external_result_chars", len(message))
    add_branch_metric(event, branch, "result_chars", len(message))
    if any(marker in message for marker in OVERFLOW_MARKERS):
        add_int_metric(event, "next_external_overflow_count", 1)
        add_branch_metric(event, branch, "overflow_count", 1)


def external_agent_branch(tool: str) -> str:
    branch = tool_branch(tool)
    return "claude" if branch == "unknown" else branch


def parse_payload_json(text: str) -> dict:
    stripped = text.strip()
    if not stripped.startswith("{"):
        return {}
    try:
        parsed = json.loads(stripped)
    except json.JSONDecodeError:
        return {}
    return parsed if isinstance(parsed, dict) else {}
