from __future__ import annotations

import json
from pathlib import Path

from analyze_tool_usage_lib import str_value
from codex_rollout_events import (
    GENERIC_CALL_RE,
    RESULT_RE,
    attach_external_call,
    attach_external_result,
    attach_result,
    codelens_tool_name,
    external_agent_branch,
    is_codelens_tool,
    message_text,
    output_text,
    parse_payload_json,
    parse_result_payload,
    pending_event,
    response_payload,
    session_id_from_entry,
)

SIGNALS = (
    "mcp__codelens",
    "prepare_harness_session",
    "semantic_index_missing",
    "delegate_to_codex_builder",
    "handoff_id",
)
def rollout_files(path: Path) -> list[Path]:
    if path.is_file():
        return [path]
    files: list[Path] = []
    for candidate in path.rglob("*.jsonl"):
        try:
            text = candidate.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            continue
        if any(signal in text for signal in SIGNALS):
            files.append(candidate)
    return sorted(files)


def load_codex_rollout_events(paths: list[Path]) -> list[dict]:
    events: list[dict] = []
    for path in paths:
        session_id = path.stem
        pending: dict | None = None
        completed: dict | None = None
        external_parent: tuple[dict, str] | None = None
        pending_codex_call_id: str | None = None
        codex_output_parents: dict[str, dict] = {}
        for line_no, line in enumerate(path.open(encoding="utf-8", errors="ignore"), 1):
            entry = json.loads(line)
            session_id = session_id_from_entry(entry, session_id)
            response = response_payload(entry)
            if response:
                response_type = str_value(response.get("type"))
                if response_type == "function_call":
                    raw_tool = str_value(response.get("name")) or ""
                    call_id = str_value(response.get("call_id"))
                    if is_codelens_tool(raw_tool):
                        pending = pending_event(
                            codelens_tool_name(raw_tool),
                            session_id,
                            path,
                            line_no,
                            len(events),
                        )
                        events.append(pending)
                        pending_codex_call_id = call_id
                        completed = None
                        external_parent = None
                        codex_output_parents.clear()
                    elif completed is not None:
                        attach_external_call(
                            completed,
                            raw_tool,
                            raw_tool + (str_value(response.get("arguments")) or ""),
                            "codex",
                        )
                        if call_id:
                            codex_output_parents[call_id] = completed
                    continue
                if response_type == "function_call_output":
                    call_id = str_value(response.get("call_id"))
                    text = output_text(response)
                    if (
                        pending is not None
                        and call_id
                        and call_id == pending_codex_call_id
                    ):
                        result = parse_payload_json(text)
                        if result:
                            attach_result(pending, result)
                            completed = pending
                            pending = None
                            pending_codex_call_id = None
                            external_parent = None
                        continue
                    if call_id and call_id in codex_output_parents:
                        parent = codex_output_parents.pop(call_id)
                        attach_external_result(parent, text, "codex")
                    continue
            message = message_text(entry)
            if not message:
                continue
            call = GENERIC_CALL_RE.search(message)
            if call is not None:
                raw_tool = call.group("tool")
                if not is_codelens_tool(raw_tool):
                    if completed is not None:
                        branch = external_agent_branch(raw_tool)
                        attach_external_call(completed, raw_tool, message, branch)
                        external_parent = (completed, branch)
                    continue
                pending = pending_event(
                    codelens_tool_name(raw_tool),
                    session_id,
                    path,
                    line_no,
                    len(events),
                )
                events.append(pending)
                completed = None
                external_parent = None
                pending_codex_call_id = None
                codex_output_parents.clear()
                continue
            result = parse_result_payload(message)
            if pending is not None and result:
                attach_result(pending, result)
                completed = pending
                pending = None
                external_parent = None
                pending_codex_call_id = None
                continue
            if external_parent is not None and RESULT_RE.search(message) is not None:
                parent, branch = external_parent
                attach_external_result(parent, message, branch)
                external_parent = None
    return events
