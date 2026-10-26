"""Pure CLI JSONL adapter. No process, credential, configuration or file access.

Observed provider counters are distinct from complete current-run measurements.
Claude modelUsage is cumulative across turns and includes descendants; usage is
main-loop-only. Codex exec does not establish descendant coverage here, and
multiple turn counters are not assumed to be additive. See the operation ledger
for the official contracts and installed-protocol evidence used by this adapter.
"""

from __future__ import annotations

import copy
import json

from analysis_lite_metrics import normalize_provider_usage


def collect_cli_events(host, stdout, *, stderr="", returncode=None, fresh_session=None):
    """Parse captured output without I/O, commands, defaults or expected settings.

    fresh_session is a runner-supplied setup fact, never inferred from a new
    process. Only a successful fresh Claude stream with whole-tree modelUsage
    and no reset/parser gaps becomes complete. Raw inputs and all decoded events
    remain available even when usage is unknown. This grades no task quality.
    """
    if host not in {"codex", "claude"}:
        raise ValueError(f"unsupported CLI host: {host}")
    events, malformed, usage_records, errors, retries = [], [], [], [], []
    unresolved, tools, models, model_receipts, configured = [], {}, set(), [], []
    sessions, assistant_ids, retry_ids = set(), set(), set()
    terminal_seen, terminal_groups = set(), {}
    duplicate_usage = turn = 0
    final_text = None
    child_seen = terminal_success = reset_seen = False
    for line, raw in enumerate(stdout.splitlines(), 1):
        if not raw.strip():
            continue
        try:
            event = json.loads(raw)
            if not isinstance(event, dict) or not isinstance(event.get("type"), str):
                raise TypeError("expected typed event object")
            for name in ("parent_tool_use_id", "uuid", "session_id", "thread_id"):
                if event.get(name) is not None and not isinstance(event[name], str):
                    raise ValueError(f"invalid event identifier: {name}")
            item = event.get("item")
            if item is not None and (not isinstance(item, dict) or not isinstance(item.get("type"), str)
                                     or not isinstance(item.get("id"), str)):
                raise ValueError("invalid item type or identifier")
            message = event.get("message")
            if event["type"] in {"assistant", "user"} and message is not None and not isinstance(message, dict):
                raise ValueError("invalid message object")
            blocks = message.get("content") if isinstance(message, dict) else None
            for block in blocks if isinstance(blocks, list) else []:
                if not isinstance(block, dict) or not isinstance(block.get("type"), str):
                    raise TypeError("invalid content block")
                if block["type"] in {"tool_use", "tool_result"}:
                    name = "id" if block["type"] == "tool_use" else "tool_use_id"
                    if not isinstance(block.get(name), str) or not block[name]:
                        raise ValueError("invalid tool identifier")
        except (ValueError, TypeError) as exc:
            malformed.append({"line": line, "raw": raw, "error": str(exc)})
            continue
        events.append({"line": line, "event": event})
        kind, parent = event["type"], event.get("parent_tool_use_id")
        child_seen = child_seen or bool(parent)
        session = event.get("thread_id" if host == "codex" else "session_id")
        if isinstance(session, str) and session and not parent:
            sessions.add(session)
        if kind == "system" and event.get("subtype") == "conversation_reset":
            reset_seen = True
        if kind == "system" and event.get("subtype") == "init":
            configured.append({"line": line, "source": "system.init", "model": event.get("model")})
        if kind == "error" or kind == "turn.failed":
            errors.append({"line": line, "event": event})
        if kind == "system" and event.get("subtype") == "api_retry":
            identity = event.get("uuid") or json.dumps(event, sort_keys=True)
            if identity not in retry_ids:
                retries.append({"line": line, "event": event})
                retry_ids.add(identity)
        if host == "codex":
            if kind == "turn.started":
                turn += 1
            if kind == "turn.completed":
                fingerprint = json.dumps(event, sort_keys=True)
                key = (turn, fingerprint)
                if key in terminal_seen:
                    duplicate_usage += 1
                else:
                    terminal_seen.add(key)
                    if turn in terminal_groups and terminal_groups[turn] != fingerprint:
                        unresolved.append("conflicting-terminal-usage")
                    terminal_groups[turn] = fingerprint
                    usage_records.append({"line": line, "turn_sequence": turn, "raw": event})
                terminal_success = True
            item = event.get("item")
            if kind.startswith("item.") and isinstance(item, dict):
                item_kind, identity = item.get("type", ""), item.get("id")
                if (item_kind == "agent_message" and kind == "item.completed" and not parent
                        and isinstance(item.get("text"), str)):
                    final_text = item["text"]
                if "collab" in item_kind:
                    child_seen = True
                if item_kind in {"command_execution", "mcp_tool_call", "web_search"} and identity:
                    key = (parent, identity)
                    tool = tools.setdefault(key, {"id": identity, "parent_tool_use_id": parent,
                        "tool": item.get("tool") or item_kind, "started_line": None, "completed_line": None})
                    if kind == "item.started":
                        tool["started_line"] = tool["started_line"] or line
                    if kind == "item.completed":
                        tool.update(completed_line=line, status=item.get("status"))
        else:
            if kind == "result" and not parent:
                fingerprint = json.dumps(event, sort_keys=True)
                if fingerprint in terminal_seen:
                    duplicate_usage += 1
                else:
                    terminal_seen.add(fingerprint)
                    usage_records.append({"line": line, "raw": event})
                failed = event.get("is_error") is True or event.get("subtype") != "success"
                terminal_success = not failed
                if failed:
                    errors.append({"line": line, "event": event})
                if isinstance(event.get("result"), str):
                    final_text = event["result"]
            message = event.get("message")
            if kind in {"assistant", "user"} and isinstance(message, dict):
                if kind == "assistant":
                    if isinstance(message.get("id"), str):
                        assistant_ids.add((parent, message["id"]))
                    model = message.get("model")
                    if not parent and isinstance(model, str) and model:
                        models.add(model)
                        model_receipts.append({"line": line, "value": model,
                                               "source": "assistant.message.model"})
                blocks = message.get("content")
                for block in blocks if isinstance(blocks, list) else []:
                    if not isinstance(block, dict):
                        continue
                    identity = block.get("id") if block.get("type") == "tool_use" else block.get("tool_use_id")
                    if block.get("type") not in {"tool_use", "tool_result"} or not identity:
                        continue
                    key = (parent, identity)
                    tool = tools.setdefault(key, {"id": identity, "parent_tool_use_id": parent,
                        "tool": None, "started_line": None, "completed_line": None})
                    if block["type"] == "tool_use":
                        tool.update(tool=block.get("name"), started_line=tool["started_line"] or line)
                        child_seen = child_seen or block.get("name") in {"Agent", "Task"}
                    else:
                        tool.update(completed_line=line, is_error=block.get("is_error"))

    observed = normalize_provider_usage(None, source=f"{host}.jsonl")
    scope = "unknown"
    whole_tree = False
    if host == "codex":
        unresolved.append("codex-descendant-usage-unverified")
        if len(usage_records) == 1 and "conflicting-terminal-usage" not in unresolved:
            raw_usage = usage_records[0]["raw"].get("usage")
            counters = copy.deepcopy(raw_usage)
            if isinstance(counters, dict) and "cache_write_input_tokens" in counters:
                counters["cache_creation_input_tokens"] = counters["cache_write_input_tokens"]
            observed = normalize_provider_usage(counters,
                source="codex.turn.completed", input_includes_cache=True)
            observed.update(provider_raw=raw_usage, raw_kind="provider-field-mapping")
            scope = "codex-reported-turn-descendants-unverified"
        elif len(usage_records) > 1:
            unresolved.append("codex-multi-turn-aggregation-unverified")
    elif usage_records:
        # modelUsage is the running whole-tree total. Never sum result snapshots
        # or add assistant placeholder usage to a final result's counters.
        latest = usage_records[-1]["raw"]
        if "modelUsage" in latest:
            model_usage = latest["modelUsage"]
            normalized = []
            if isinstance(model_usage, dict):
                for name, values in model_usage.items():
                    values = values if isinstance(values, dict) else {}
                    counters = {"input_tokens": values.get("inputTokens"),
                        "output_tokens": values.get("outputTokens"),
                        "cache_read_input_tokens": values.get("cacheReadInputTokens"),
                        "cache_creation_input_tokens": values.get("cacheCreationInputTokens")}
                    normalized.append(normalize_provider_usage(counters,
                        source=f"claude.result.modelUsage.{name}", input_includes_cache=False))
            if normalized and all(row["status"] == "ok" for row in normalized):
                counters = {"input_tokens": sum(row["uncached_input_tokens"] for row in normalized),
                    "output_tokens": sum(row["output_tokens"] for row in normalized),
                    "cache_read_input_tokens": sum(row["cached_input_tokens"] for row in normalized),
                    "cache_creation_input_tokens": sum(row["cache_creation_input_tokens"] for row in normalized)}
                observed = normalize_provider_usage(counters, source="claude.result.modelUsage",
                                                    input_includes_cache=False)
                observed.update(provider_raw=model_usage, raw_kind="derived-sum-of-modelUsage")
                whole_tree = True
                scope = "claude-modelUsage-whole-tree"
                for earlier in usage_records[:-1]:
                    previous = earlier["raw"].get("modelUsage")
                    if not isinstance(previous, dict):
                        continue
                    for name, old in previous.items():
                        if not isinstance(old, dict):
                            continue
                        new = model_usage.get(name, {})
                        if not isinstance(new, dict):
                            new = {}
                        for field in ("inputTokens", "outputTokens", "cacheReadInputTokens", "cacheCreationInputTokens"):
                            before, after = old.get(field), new.get(field)
                            if type(before) is int and (type(after) is not int or after < before):
                                unresolved.append("nonmonotonic-cumulative-modelUsage")
            else:
                observed.update(status="error", error="invalid or incomplete modelUsage", provider_raw=model_usage)
                unresolved.append("invalid-modelUsage")
        else:
            observed = normalize_provider_usage(latest.get("usage"), source="claude.result.usage",
                                                input_includes_cache=False)
            scope = "claude-latest-result-main-loop-only"
            unresolved.append("claude-main-loop-only")
    if fresh_session is not True:
        unresolved.append("fresh-session-unconfirmed")
    if malformed:
        unresolved.append("malformed-jsonl")
    if reset_seen or len(sessions) > 1:
        unresolved.append("session-reset-or-multiple-sessions")
    if not usage_records:
        unresolved.append("missing-terminal-usage")
    if errors or (returncode is not None and returncode != 0):
        run_status = "failed"
    elif terminal_success and returncode == 0:
        run_status = "completed"
    else:
        run_status = "incomplete"
    if run_status != "completed":
        unresolved.append("run-not-successfully-terminated")
    complete = whole_tree and observed["status"] == "ok" and not unresolved
    provider = copy.deepcopy(observed)
    if not complete:
        provider["status"] = "partial" if observed["status"] == "ok" else observed["status"]
        for key in ("input_tokens", "output_tokens", "cached_input_tokens",
                    "cache_creation_input_tokens", "uncached_input_tokens", "total_tokens"):
            provider[key] = None
    return {
        "schema_version": "analysis-lite-cli-events-v1", "host": host,
        "raw_stdout": stdout, "raw_stderr": stderr, "returncode": returncode,
        "events": events, "malformed_lines": malformed, "usage_records": usage_records,
        "duplicate_usage_events": duplicate_usage, "observed_usage": observed,
        "provider_usage": provider, "usage_scope": scope,
        "usage_completeness": "complete" if complete else "unknown",
        "fresh_session_receipt": fresh_session, "unresolved": list(dict.fromkeys(unresolved)),
        "run_status": run_status, "errors": errors, "final_text": final_text,
        "child_activity_observed": child_seen, "tool_events": list(tools.values()),
        "tool_roundtrips": sum(bool(row["started_line"] and row["completed_line"]) for row in tools.values()),
        "tool_count_scope": "visible-paired-event-ids-only", "retry_events": retries,
        "visible_retry_events": len(retries), "actual_retry_attempts": None,
        "unique_assistant_message_ids": len(assistant_ids),
        "settings": {
            "model": {"value": next(iter(models)) if len(models) == 1 else None,
                "status": "observed" if len(models) == 1 else "multiple" if models else "unknown",
                "source": "assistant.message.model" if models else None,
                "receipts": model_receipts},
            "reasoning_effort": {"value": None, "status": "unknown", "receipts": []},
            "configured_model_receipts": configured,
        },
    }
