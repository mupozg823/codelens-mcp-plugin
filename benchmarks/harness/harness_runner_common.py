#!/usr/bin/env python3
"""Shared helpers for Codex/Claude harness task runners."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import subprocess
import urllib.request
from urllib.error import URLError
from datetime import datetime
from pathlib import Path

DEFAULT_HTTP_REQUEST_TIMEOUT_SECONDS = 5
DEFAULT_HTTP_BOOTSTRAP_TIMEOUT_SECONDS = 10
DEFAULT_HTTP_TOOL_TIMEOUT_SECONDS = 90
RUN_MANIFEST_FILENAME = "run-manifest.json"
RUN_EVENT_LOG_FILENAME = "run-events.jsonl"


def load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


def load_task_text(args) -> str:
    if args.task and args.task_file:
        raise SystemExit("Use either --task or --task-file, not both.")
    if args.task_file:
        return Path(args.task_file).expanduser().read_text().strip()
    if args.task:
        return args.task.strip()
    raise SystemExit("Either --task or --task-file is required.")


def infer_mode_from_policy(policy_name: str) -> str:
    if policy_name in {"prefer_routed_codelens", "prefer_codelens_after_bootstrap"}:
        return "routed-on"
    if policy_name == "prefer_naive_codelens":
        return "naive-on"
    return "baseline"


def slugify(value: str) -> str:
    chars = []
    for char in value.lower():
        if char.isalnum():
            chars.append(char)
        else:
            chars.append("-")
    slug = "".join(chars)
    while "--" in slug:
        slug = slug.replace("--", "-")
    return slug.strip("-") or "task"


def now_iso() -> str:
    return datetime.now().isoformat(timespec="seconds")


def read_json_file(path: Path):
    return json.loads(path.read_text())


def write_json_file(path: Path, payload: dict):
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")


def run_identity(
    *,
    runner: str,
    agent: str,
    repo_path: Path,
    execution_repo_path: Path,
    task_kind: str,
    mode: str,
    scenario_id: str | None,
):
    return {
        "runner": runner,
        "agent": agent,
        "repo": str(repo_path),
        "execution_repo": str(execution_repo_path),
        "task_kind": task_kind,
        "mode": mode or "",
        "scenario_id": scenario_id or None,
    }


def append_run_event(event_log_path: Path, event_type: str, payload: dict):
    event_log_path.parent.mkdir(parents=True, exist_ok=True)
    record = {
        "timestamp": now_iso(),
        "event_type": event_type,
        "payload": payload,
    }
    with event_log_path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(record, ensure_ascii=False) + "\n")


def ensure_run_manifest(
    *,
    run_dir: Path,
    runner: str,
    agent: str,
    repo_path: Path,
    execution_repo_path: Path,
    task_kind: str,
    mode: str,
    scenario_id: str | None,
    recommended_policy: str,
    route_mode: str,
):
    manifest_path = run_dir / RUN_MANIFEST_FILENAME
    event_log_path = run_dir / RUN_EVENT_LOG_FILENAME
    identity = run_identity(
        runner=runner,
        agent=agent,
        repo_path=repo_path,
        execution_repo_path=execution_repo_path,
        task_kind=task_kind,
        mode=mode,
        scenario_id=scenario_id,
    )
    if manifest_path.exists():
        manifest = read_json_file(manifest_path)
        existing_identity = manifest.get("identity") or {}
        mismatches = []
        for key, value in identity.items():
            if existing_identity.get(key) != value:
                mismatches.append(
                    {
                        "field": key,
                        "expected": value,
                        "found": existing_identity.get(key),
                    }
                )
        if mismatches:
            raise SystemExit(
                "run-dir already belongs to a different harness execution identity: "
                f"{json.dumps(mismatches, ensure_ascii=False)}"
            )
        manifest["updated_at"] = now_iso()
        manifest["routing"] = {
            "recommended_policy": recommended_policy,
            "route_mode": route_mode,
        }
        write_json_file(manifest_path, manifest)
        append_run_event(
            event_log_path,
            "run_reopened",
            {
                "identity": identity,
                "routing": manifest["routing"],
            },
        )
        return manifest_path, event_log_path, manifest

    manifest = {
        "schema_version": "codelens-harness-run-v1",
        "run_id": run_dir.name,
        "run_dir": str(run_dir),
        "created_at": now_iso(),
        "updated_at": now_iso(),
        "identity": identity,
        "routing": {
            "recommended_policy": recommended_policy,
            "route_mode": route_mode,
        },
        "checkpoints": {},
        "artifacts": {},
    }
    write_json_file(manifest_path, manifest)
    append_run_event(
        event_log_path,
        "run_initialized",
        {
            "identity": identity,
            "routing": manifest["routing"],
        },
    )
    return manifest_path, event_log_path, manifest


def load_run_manifest(manifest_path: Path) -> dict:
    return read_json_file(manifest_path)


def checkpoint_status(manifest: dict, stage: str) -> str | None:
    checkpoint = (manifest.get("checkpoints") or {}).get(stage) or {}
    status = checkpoint.get("status")
    return status if isinstance(status, str) else None


def artifact_is_reusable(manifest_path: Path, stage: str, artifact_path: Path) -> bool:
    if not manifest_path.exists() or not artifact_path.exists():
        return False
    manifest = load_run_manifest(manifest_path)
    return checkpoint_status(manifest, stage) == "completed"


def load_reusable_artifact_json(manifest_path: Path, stage: str, artifact_path: Path):
    if not artifact_is_reusable(manifest_path, stage, artifact_path):
        return None
    return read_json_file(artifact_path)


def checkpoint_run_stage(
    manifest_path: Path,
    event_log_path: Path,
    stage: str,
    *,
    status: str,
    artifacts: dict[str, str | Path] | None = None,
    details: dict | None = None,
):
    manifest = load_run_manifest(manifest_path)
    checkpoints = manifest.setdefault("checkpoints", {})
    stage_record = checkpoints.get(stage) or {}
    stage_record.update(
        {
            "status": status,
            "updated_at": now_iso(),
        }
    )
    if details:
        stage_record["details"] = details
    if artifacts:
        materialized_artifacts = {
            key: str(Path(value)) if isinstance(value, Path) else str(value)
            for key, value in artifacts.items()
        }
        stage_record["artifacts"] = materialized_artifacts
        manifest.setdefault("artifacts", {}).update(materialized_artifacts)
    checkpoints[stage] = stage_record
    manifest["updated_at"] = now_iso()
    write_json_file(manifest_path, manifest)
    append_run_event(
        event_log_path,
        "stage_checkpointed",
        {
            "stage": stage,
            "status": status,
            "details": details or {},
            "artifacts": checkpoints[stage].get("artifacts", {}),
        },
    )
    return manifest


def record_stage_reuse(
    manifest_path: Path,
    event_log_path: Path,
    stage: str,
    *,
    artifacts: dict[str, str | Path] | None = None,
    details: dict | None = None,
):
    manifest = load_run_manifest(manifest_path)
    checkpoints = manifest.setdefault("checkpoints", {})
    stage_record = checkpoints.get(stage) or {}
    stage_record["status"] = stage_record.get("status") or "completed"
    stage_record["last_reused_at"] = now_iso()
    stage_record["reuse_count"] = int(stage_record.get("reuse_count") or 0) + 1
    if details:
        stage_record["reuse_details"] = details
    if artifacts:
        materialized_artifacts = {
            key: str(Path(value)) if isinstance(value, Path) else str(value)
            for key, value in artifacts.items()
        }
        stage_record["artifacts"] = {
            **(stage_record.get("artifacts") or {}),
            **materialized_artifacts,
        }
        manifest.setdefault("artifacts", {}).update(materialized_artifacts)
    checkpoints[stage] = stage_record
    manifest["updated_at"] = now_iso()
    write_json_file(manifest_path, manifest)
    append_run_event(
        event_log_path,
        "stage_reused",
        {
            "stage": stage,
            "details": details or {},
            "artifacts": stage_record.get("artifacts", {}),
            "reuse_count": stage_record.get("reuse_count", 0),
        },
    )
    return manifest


def resolve_execution_repo_path(repo_path: Path, repo_id: str = "", alias_dir: Path | None = None):
    repo_path = repo_path.expanduser().resolve()
    if str(repo_path).isascii():
        return repo_path, None

    if alias_dir is None:
        alias_dir = Path.home() / ".codex" / "harness" / "workspaces"
    alias_dir.mkdir(parents=True, exist_ok=True)
    digest = hashlib.sha1(str(repo_path).encode("utf-8")).hexdigest()[:10]
    alias_path = alias_dir / f"{slugify(repo_id or repo_path.name)}-{digest}"

    if alias_path.is_symlink():
        if alias_path.resolve() != repo_path:
            alias_path.unlink()
            alias_path.symlink_to(repo_path, target_is_directory=True)
    elif not alias_path.exists():
        alias_path.symlink_to(repo_path, target_is_directory=True)

    if alias_path.exists() or alias_path.is_symlink():
        return alias_path, {
            "active": True,
            "alias_path": str(alias_path),
            "target_path": str(repo_path),
        }

    return repo_path, None


def render_prompt(brief: dict, global_instruction_label: str, mcp_preflight: dict | None = None) -> str:
    lines = [
        f"Task kind: {brief['task_kind']}",
        f"Routing policy: {brief['recommended_policy']} ({brief['policy_source']}, confidence={brief['confidence']})",
        f"Route mode: {brief['route_mode']}",
        f"Policy reason: {brief['explanation']}",
        "",
    ]

    if brief.get("evaluation_mode") == "read-only-eval":
        lines.extend(
            [
                "This is a harness evaluation run.",
                "Treat the task as read-only: do not apply patches, do not modify files, and do not stage or commit changes.",
                "Focus on evidence, bounded review/planning output, and the smallest verification needed to support the verdict.",
                "Keep the native bootstrap to one small boundary check such as changed-file listing; avoid full diff stats or broad repo scans before the first workflow report.",
                "If the first native boundary check uses `rg`, exclude docs/build/generated noise by default (`--glob '!node_modules' --glob '!.next' --glob '!coverage' --glob '!dist' --glob '!docs/**' --glob '!*.tsbuildinfo'`) unless the task explicitly targets those paths.",
                "",
            ]
        )
    elif brief.get("evaluation_mode") == "bounded-local-eval":
        lines.extend(
            [
                "This is a harness evaluation run.",
                "Prefer a bounded local read-only pass first. Do not edit files unless the task explicitly requires it.",
                "",
            ]
        )

    lines.extend(
        [
            f"Follow the repository AGENTS.md and the global {global_instruction_label} instructions.",
            "Use the following routing guidance for this task:",
        ]
    )

    for action in brief.get("first_actions", []):
        lines.append(f"- {action}")

    if brief.get("workflow_budget") or brief.get("result_budget") or brief.get("stop_rule"):
        lines.extend(["", "Bounded evaluation contract:"])
        for key, value in brief.get("workflow_budget", {}).items():
            lines.append(f"- {key}: {value}")
        for key, value in brief.get("result_budget", {}).items():
            lines.append(f"- result {key}: {value}")
        if brief.get("stop_rule"):
            lines.append(f"- stop rule: {brief['stop_rule']}")

    if brief.get("preferred_entrypoints") and brief.get("use_codelens") != "avoid":
        lines.extend(
            [
                "",
                "Preferred CodeLens entrypoints for this task kind:",
                *[f"- {tool}" for tool in brief["preferred_entrypoints"]],
            ]
        )

    if mcp_preflight:
        lines.extend(["", "MCP preflight:"])
        if mcp_preflight.get("available"):
            lines.append(
                f"- CodeLens MCP reachable; active surface={mcp_preflight.get('auto_surface') or 'unknown'}, budget={mcp_preflight.get('auto_budget') or 'unknown'}."
            )
            if mcp_preflight.get("tools_list_contract_mode") == "lean":
                lines.append(
                    "- Deferred tools/list is lean by default. If you need output schemas for mutation, verifier, or tool-shape ambiguity, retry tools/list with includeOutputSchema=true and narrow by namespace/tier first."
                )
            elif mcp_preflight.get("tools_list_contract_mode") == "full":
                lines.append("- Deferred tools/list already includes output schemas.")
            if mcp_preflight.get("richer_contract_prefetched"):
                lines.append(
                    f"- Richer tool contract prefetched for {mcp_preflight.get('richer_contract_scope') or 'targeted'} scope; visible tools={mcp_preflight.get('richer_contract_tool_count') or 'unknown'}."
                )
            if mcp_preflight.get("embedding_indexed") is not None:
                lines.append(
                    f"- Semantic index ready={mcp_preflight.get('embedding_indexed')}, indexed_symbols={mcp_preflight.get('embedding_indexed_symbols', 0)}."
                )
            if mcp_preflight.get("preferred_entrypoints"):
                lines.append(
                    f"- Suggested bootstrap entrypoints: {', '.join(mcp_preflight['preferred_entrypoints'])}."
                )
            if mcp_preflight.get("recommended_entrypoint"):
                lines.append(
                    f"- Recommended next CodeLens entrypoint: {mcp_preflight['recommended_entrypoint']} ({mcp_preflight.get('recommendation_source', 'preflight')})."
                )
            if mcp_preflight.get("recommended_contract_action"):
                lines.append(
                    f"- Contract action: {mcp_preflight['recommended_contract_action']}."
                )
            if mcp_preflight.get("recommended_followup_tools"):
                lines.append(
                    f"- Follow-up tools after the first entrypoint: {', '.join(mcp_preflight['recommended_followup_tools'])}."
                )
            if mcp_preflight.get("fallback_to_native"):
                lines.append("- Even though MCP is reachable, stay native first and escalate only after the initial local boundary check.")
        else:
            lines.append("- CodeLens MCP preflight failed; treat CodeLens as unavailable for this run.")
            lines.append("- Stay on the native path and do not assume workflow tools are available.")
            if mcp_preflight.get("error"):
                lines.append(f"- Preflight error: {mcp_preflight['error']}")

    lines.extend(["", "Task:", brief.get("task", "").strip(), ""])
    if brief.get("evaluation_mode") == "read-only-eval":
        lines.append("Verification guidance:")
        lines.append("- Run only the smallest directly relevant verification needed to support the reviewer/preflight verdict.")
        if brief.get("verify_commands"):
            lines.append("- Repo-wide verification is optional for this evaluation run; only escalate if the evidence requires it.")
            lines.extend(f"- Optional repo check: {command}" for command in brief["verify_commands"])
    else:
        lines.append("Verification before finishing:")
        if brief.get("verify_commands"):
            lines.extend(f"- {command}" for command in brief["verify_commands"])
        else:
            lines.append("- Run the smallest relevant verification available in the repo.")

    lines.extend(
        [
            "",
            "Delivery:",
            "- Keep CodeLens usage aligned with the routing policy above.",
            "- Report the verdict, evidence used, verification actually run, and remaining risks.",
            "- If this is a read-only evaluation run, leave the worktree unchanged.",
            "",
        ]
    )
    return "\n".join(lines)


def mcp_http_call(
    base_url: str,
    method: str,
    params: dict | None = None,
    request_id: int = 1,
    headers: dict | None = None,
    include_headers: bool = False,
    timeout_seconds: int = DEFAULT_HTTP_REQUEST_TIMEOUT_SECONDS,
):
    payload = {
        "jsonrpc": "2.0",
        "id": request_id,
        "method": method,
    }
    if params is not None:
        payload["params"] = params
    request_headers = {"Content-Type": "application/json"}
    if headers:
        request_headers.update(headers)
    req = urllib.request.Request(
        base_url,
        data=json.dumps(payload).encode("utf-8"),
        headers=request_headers,
    )
    with urllib.request.urlopen(req, timeout=timeout_seconds) as resp:
        parsed = json.loads(resp.read().decode("utf-8"))
        if include_headers:
            return parsed, {key.lower(): value for key, value in resp.headers.items()}
        return parsed


def mcp_http_tool_call(
    base_url: str,
    name: str,
    arguments: dict,
    request_id: int = 1,
    session_id: str | None = None,
    timeout_seconds: int = DEFAULT_HTTP_TOOL_TIMEOUT_SECONDS,
):
    payload = {
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": arguments,
        },
    }
    request_headers = {"Content-Type": "application/json"}
    if session_id:
        request_headers["mcp-session-id"] = session_id
    return mcp_http_call(
        base_url,
        "tools/call",
        {"name": name, "arguments": arguments},
        request_id=request_id,
        headers=request_headers,
        timeout_seconds=timeout_seconds,
    )


def extract_tool_payload(response):
    if not isinstance(response, dict):
        return {}
    result = response.get("result")
    if isinstance(result, dict):
        content = result.get("content")
        if isinstance(content, list) and content:
            text = content[0].get("text", "{}")
            try:
                parsed = json.loads(text)
                if isinstance(parsed, dict):
                    return parsed
            except Exception:
                pass
        if "data" in result or "success" in result or "error" in result:
            return result
    error = response.get("error")
    if isinstance(error, dict):
        return {"success": False, "error": error.get("message", "unknown error")}
    return {}


def safe_capture_metrics_snapshot(base_url: str, request_id: int, session_id: str | None = None):
    try:
        return capture_metrics_snapshot(
            base_url,
            request_id=request_id,
            session_id=session_id,
        ), None
    except Exception as exc:
        return None, str(exc)


def is_hidden_by_deferred_loading(payload: dict) -> bool:
    error = str((payload or {}).get("error") or "")
    return "hidden by deferred loading" in error


def should_prefetch_richer_tools_contract(brief: dict) -> bool:
    task_kind = str(brief.get("task_kind") or "").strip().lower()
    if task_kind == "refactor preflight":
        return True

    task_text = str(brief.get("task") or "").lower()
    mutation_markers = ("refactor", "rename", "mutation", "verifier", "preflight")
    return any(marker in task_text for marker in mutation_markers)


def fetch_richer_tools_contract(base_url: str, session_id: str | None, request_id: int):
    headers = {"mcp-session-id": session_id} if session_id else None
    workflow_result = mcp_http_call(
        base_url,
        "tools/list",
        {"tier": "workflow", "includeOutputSchema": True},
        request_id=request_id,
        headers=headers,
    )
    workflow_payload = workflow_result.get("result", {}) if isinstance(workflow_result, dict) else {}
    tools = workflow_payload.get("tools", [])
    if tools:
        return {
            "prefetched": True,
            "scope": "workflow",
            "tool_count": workflow_payload.get("tool_count", len(tools)),
            "include_output_schema": workflow_payload.get("include_output_schema"),
            "full_tool_exposure": workflow_payload.get("full_tool_exposure"),
            "tools": tools,
        }

    full_result = mcp_http_call(
        base_url,
        "tools/list",
        {"includeOutputSchema": True},
        request_id=request_id + 1,
        headers=headers,
    )
    full_payload = full_result.get("result", {}) if isinstance(full_result, dict) else {}
    return {
        "prefetched": True,
        "scope": "full",
        "tool_count": full_payload.get("tool_count", len(full_payload.get("tools", []))),
        "include_output_schema": full_payload.get("include_output_schema"),
        "full_tool_exposure": full_payload.get("full_tool_exposure"),
        "tools": full_payload.get("tools", []),
    }


def extract_tool_names(payload: dict) -> list[str]:
    tools = payload.get("tools", []) if isinstance(payload, dict) else []
    names = []
    for tool in tools:
        if isinstance(tool, dict):
            name = tool.get("name")
            if isinstance(name, str):
                names.append(name)
    return names


def build_codex_routing_recommendation(
    brief: dict,
    tool_list_result: dict,
    primitive_result: dict,
    richer_contract: dict | None,
    tools_list_contract_mode: str,
):
    preferred_entrypoints = list(brief.get("preferred_entrypoints") or [])
    visible_tool_names = set(extract_tool_names(tool_list_result))
    primitive_tool_names = set(extract_tool_names(primitive_result))
    richer_tool_names = set(extract_tool_names(richer_contract or {}))

    recommended_entrypoint = None
    recommendation_source = "preferred_entrypoints"
    for candidate in preferred_entrypoints:
        if candidate in visible_tool_names:
            recommended_entrypoint = candidate
            recommendation_source = "default_contract"
            break
    if recommended_entrypoint is None:
        for candidate in preferred_entrypoints:
            if candidate in richer_tool_names:
                recommended_entrypoint = candidate
                recommendation_source = "prefetched_contract"
                break

    contract_action = "stay_on_default_contract"
    if tools_list_contract_mode == "lean" and should_prefetch_richer_tools_contract(brief):
        contract_action = (
            "use_prefetched_workflow_contract" if richer_contract else "request_workflow_contract"
        )
    elif tools_list_contract_mode == "lean":
        contract_action = "stay_lean_until_shape_needed"
    elif recommended_entrypoint is None and primitive_tool_names:
        contract_action = "open_primitive_tier"

    followup_tools = []
    for candidate in preferred_entrypoints:
        if candidate == recommended_entrypoint:
            continue
        if candidate in visible_tool_names or candidate in richer_tool_names:
            followup_tools.append(candidate)

    return {
        "recommended_entrypoint": recommended_entrypoint,
        "recommendation_source": recommendation_source,
        "recommended_contract_action": contract_action,
        "recommended_followup_tools": followup_tools[:3],
        "preferred_entrypoints_visible": [
            tool for tool in preferred_entrypoints if tool in visible_tool_names
        ],
        "preferred_entrypoints_in_prefetched_contract": [
            tool for tool in preferred_entrypoints if tool in richer_tool_names
        ],
    }


def summarize_called_tools(delta_payload: dict) -> list[dict]:
    rows = []
    for row in delta_payload.get("tools", []) if isinstance(delta_payload, dict) else []:
        if not isinstance(row, dict):
            continue
        tool = row.get("tool")
        calls = int(row.get("calls") or 0)
        if not isinstance(tool, str) or calls <= 0:
            continue
        rows.append(
            {
                "tool": tool,
                "calls": calls,
                "total_ms": int(row.get("total_ms") or 0),
                "total_tokens": int(row.get("total_tokens") or 0),
            }
        )
    rows.sort(key=lambda item: (-item["calls"], -item["total_ms"], item["tool"]))
    return rows


def parse_codex_json_events(text: str) -> list[dict]:
    rows = []
    for line in text.splitlines():
        candidate = line.strip()
        if not candidate.startswith("{"):
            continue
        try:
            payload = json.loads(candidate)
        except json.JSONDecodeError:
            continue
        if isinstance(payload, dict):
            rows.append(payload)
    return rows


def summarize_codex_mcp_tool_events(
    event_rows: list[dict],
    *,
    server_name: str | None = "codelens",
) -> list[dict]:
    by_tool: dict[str, dict] = {}
    for row in event_rows:
        if not isinstance(row, dict):
            continue
        event_type = row.get("type")
        item = row.get("item")
        if event_type not in {"item.started", "item.completed"} or not isinstance(item, dict):
            continue
        if item.get("type") != "mcp_tool_call":
            continue
        tool = item.get("tool")
        server = item.get("server")
        if not isinstance(tool, str):
            continue
        if server_name and server != server_name:
            continue
        state = by_tool.setdefault(
            tool,
            {
                "tool": tool,
                "server": server,
                "calls": 0,
                "successes": 0,
                "failures": 0,
                "cancelled": 0,
                "statuses": [],
                "error_messages": [],
            },
        )
        if event_type == "item.started":
            state["calls"] += 1
            continue

        if state["calls"] == 0:
            state["calls"] = 1
        status = item.get("status")
        if isinstance(status, str):
            state["statuses"].append(status)
            normalized = status.lower()
            if normalized in {"completed", "success", "succeeded"}:
                state["successes"] += 1
            elif normalized == "failed":
                state["failures"] += 1
            elif normalized in {"cancelled", "canceled"}:
                state["cancelled"] += 1
        error = item.get("error")
        if isinstance(error, dict):
            message = error.get("message")
            if isinstance(message, str) and message not in state["error_messages"]:
                state["error_messages"].append(message)

    rows = list(by_tool.values())
    rows.sort(
        key=lambda item: (
            -item["calls"],
            -item["successes"],
            -item["failures"],
            -item["cancelled"],
            item["tool"],
        )
    )
    return rows


def build_codex_recommendation_outcome(
    mcp_preflight: dict | None,
    delta_payload: dict,
    *,
    codex_event_rows: list[dict] | None = None,
) -> dict | None:
    if not isinstance(mcp_preflight, dict) or not mcp_preflight.get("available"):
        return None

    recommended_entrypoint = mcp_preflight.get("recommended_entrypoint")
    recommended_followup_tools = list(mcp_preflight.get("recommended_followup_tools") or [])
    recommended_contract_action = mcp_preflight.get("recommended_contract_action")
    event_trace_tools = summarize_codex_mcp_tool_events(codex_event_rows or [])
    event_trace_counts = {row["tool"]: row["calls"] for row in event_trace_tools}
    event_trace_successes = {row["tool"]: row["successes"] for row in event_trace_tools}
    event_trace_failures = {row["tool"]: row["failures"] for row in event_trace_tools}
    event_trace_cancelled = {row["tool"]: row["cancelled"] for row in event_trace_tools}
    used_event_trace = bool(event_trace_tools)

    called_tools = event_trace_tools if used_event_trace else summarize_called_tools(delta_payload)
    called_counts = {row["tool"]: row["calls"] for row in called_tools}
    session = delta_payload.get("session", {}) if isinstance(delta_payload, dict) else {}
    deferred_expansions = int(session.get("deferred_namespace_expansion_count") or 0)
    deferred_hidden_denials = int(session.get("deferred_hidden_tool_call_denied_count") or 0)

    recommended_entrypoint_called = (
        recommended_entrypoint in called_counts if isinstance(recommended_entrypoint, str) else None
    )
    recommended_entrypoint_success_count = (
        event_trace_successes.get(recommended_entrypoint, 0)
        if used_event_trace and isinstance(recommended_entrypoint, str)
        else None
    )
    recommended_entrypoint_failure_count = (
        event_trace_failures.get(recommended_entrypoint, 0)
        if used_event_trace and isinstance(recommended_entrypoint, str)
        else None
    )
    recommended_entrypoint_cancelled_count = (
        event_trace_cancelled.get(recommended_entrypoint, 0)
        if used_event_trace and isinstance(recommended_entrypoint, str)
        else None
    )
    recommended_followup_tools_called = [
        tool for tool in recommended_followup_tools if tool in called_counts
    ]
    recommended_followup_tools_succeeded = [
        tool for tool in recommended_followup_tools if event_trace_successes.get(tool, 0) > 0
    ] if used_event_trace else list(recommended_followup_tools_called)
    recommended_followup_tools_missed = [
        tool for tool in recommended_followup_tools if tool not in called_counts
    ]
    recommended_followup_tools_attempted_without_success = [
        tool
        for tool in recommended_followup_tools_called
        if tool not in recommended_followup_tools_succeeded
    ]

    alignment = "no-recommendation"
    entrypoint_matched = (
        recommended_entrypoint_success_count and recommended_entrypoint_success_count > 0
        if used_event_trace
        else recommended_entrypoint_called
    )
    followup_matched = (
        bool(recommended_followup_tools_succeeded)
        if used_event_trace
        else bool(recommended_followup_tools_called)
    )
    if entrypoint_matched:
        alignment = "matched-entrypoint"
    elif followup_matched:
        alignment = "matched-followup"
    elif recommended_entrypoint_called:
        alignment = "attempted-entrypoint"
    elif recommended_followup_tools_called:
        alignment = "attempted-followup"
    elif recommended_entrypoint or recommended_followup_tools:
        alignment = "no-match"

    contract_action_aligned = None
    if recommended_contract_action == "stay_lean_until_shape_needed":
        contract_action_aligned = deferred_expansions == 0 and deferred_hidden_denials == 0
    elif recommended_contract_action == "use_prefetched_workflow_contract":
        contract_action_aligned = bool(mcp_preflight.get("richer_contract_prefetched"))
    elif recommended_contract_action in {"stay_on_default_contract", "open_primitive_tier"}:
        contract_action_aligned = True

    return {
        "recommended_entrypoint": recommended_entrypoint,
        "evidence_source": "codex_event_trace" if used_event_trace else "tool_metrics",
        "recommended_entrypoint_called": recommended_entrypoint_called,
        "recommended_entrypoint_call_count": (
            called_counts.get(recommended_entrypoint, 0) if isinstance(recommended_entrypoint, str) else 0
        ),
        "recommended_entrypoint_success_count": recommended_entrypoint_success_count,
        "recommended_entrypoint_failure_count": recommended_entrypoint_failure_count,
        "recommended_entrypoint_cancelled_count": recommended_entrypoint_cancelled_count,
        "recommended_followup_tools": recommended_followup_tools,
        "recommended_followup_tools_called": recommended_followup_tools_called,
        "recommended_followup_tools_succeeded": recommended_followup_tools_succeeded,
        "recommended_followup_tools_attempted_without_success": recommended_followup_tools_attempted_without_success,
        "recommended_followup_tools_missed": recommended_followup_tools_missed,
        "recommended_contract_action": recommended_contract_action,
        "contract_action_aligned": contract_action_aligned,
        "deferred_namespace_expansion_count": deferred_expansions,
        "deferred_hidden_tool_call_denied_count": deferred_hidden_denials,
        "alignment": alignment,
        "top_called_tools": called_tools[:5],
    }


def summarize_codex_recommendation_outcome(outcome: dict | None) -> str:
    if not isinstance(outcome, dict):
        return ""
    recommended_entrypoint = outcome.get("recommended_entrypoint")
    alignment = outcome.get("alignment")
    if alignment == "matched-entrypoint" and recommended_entrypoint:
        return f"recommended entrypoint {recommended_entrypoint} was exercised"
    if alignment == "matched-followup":
        called = outcome.get("recommended_followup_tools_succeeded") or []
        if called:
            return f"recommended follow-up tools exercised: {', '.join(called)}"
    if alignment == "attempted-entrypoint" and recommended_entrypoint:
        return f"recommended entrypoint {recommended_entrypoint} was attempted but did not complete successfully"
    if alignment == "attempted-followup":
        called = outcome.get("recommended_followup_tools_attempted_without_success") or []
        if called:
            return f"recommended follow-up tools were attempted but did not complete successfully: {', '.join(called)}"
    if alignment == "no-match" and recommended_entrypoint:
        source = outcome.get("evidence_source")
        if source == "codex_event_trace":
            return f"recommended entrypoint {recommended_entrypoint} was not observed in the Codex event trace"
        return f"recommended entrypoint {recommended_entrypoint} was not observed in tool metrics"
    return ""


def probe_codex_mcp(base_url: str, repo_path: Path, brief: dict, request_id_base: int = 9000):
    preferred_entrypoints = list(brief.get("preferred_entrypoints") or [])
    fallback_to_native = brief.get("recommended_policy") in {
        "prefer_codelens_after_bootstrap",
        "native_or_naive_both_ok_but_default_native",
        "avoid_codelens_for_simple_local_lookup",
        "prefer_native_for_simple_local_lookup",
    }
    try:
        init_response, response_headers = mcp_http_call(
            base_url,
            "initialize",
            {
                "clientInfo": {"name": "CodexHarness", "version": "1.0.0"},
                "deferredToolLoading": True,
            },
            request_id=request_id_base,
            headers={},
            include_headers=True,
        )
        session_id = response_headers.get("mcp-session-id")
        bootstrap = mcp_http_tool_call(
            base_url,
            "prepare_harness_session",
            {
                "project": str(repo_path),
                "detail": "compact",
                "preferred_entrypoints": preferred_entrypoints,
            },
            request_id=request_id_base + 3,
            session_id=session_id,
        )
        bootstrap_payload = extract_tool_payload(bootstrap)
        bootstrap_data = (
            bootstrap_payload.get("data", {}) if isinstance(bootstrap_payload, dict) else {}
        )
        if bootstrap_payload.get("success") is not False and isinstance(bootstrap_data, dict) and bootstrap_data:
            session_payload = bootstrap_data.get("http_session", {})
            visible_tools = bootstrap_data.get("visible_tools", {})
            routing = bootstrap_data.get("routing", {})
            project_data = bootstrap_data.get("project", {})
            config_data = bootstrap_data.get("config", {})
            caps_data = bootstrap_data.get("capabilities", {})
            include_output_schema = (
                session_payload.get("default_tools_list_contract_mode") != "lean"
            )
            tools_list_contract_mode = "full" if include_output_schema else "lean"
            schema_recovery_hint = None
            if not include_output_schema:
                schema_recovery_hint = (
                    "Retry tools/list with includeOutputSchema=true only when output shapes matter; "
                    "prefer narrowing by namespace or tier before requesting a full contract."
                )
            richer_contract = None
            if tools_list_contract_mode == "lean" and should_prefetch_richer_tools_contract(brief):
                richer_contract = fetch_richer_tools_contract(base_url, session_id, request_id_base + 4)
            recommended_entrypoint = routing.get("recommended_entrypoint")
            preferred_visible = routing.get("preferred_entrypoints_visible") or []
            followup_tools = [tool for tool in preferred_visible if tool != recommended_entrypoint]
            return {
                "available": True,
                "session_id": session_id,
                "tool_count": visible_tools.get("tool_count", 0),
                "tool_count_total": visible_tools.get("tool_count_total", visible_tools.get("tool_count", 0)),
                "effective_namespaces": visible_tools.get("effective_namespaces", []),
                "preferred_namespaces": visible_tools.get("preferred_namespaces", []),
                "loaded_tiers": visible_tools.get("loaded_tiers", []),
                "deferred_loading_active": visible_tools.get("deferred_loading_active"),
                "include_output_schema": include_output_schema,
                "tools_list_contract_mode": tools_list_contract_mode,
                "schema_recovery_hint": schema_recovery_hint,
                "richer_contract_prefetched": bool(richer_contract and richer_contract.get("prefetched")),
                "richer_contract_scope": None if not richer_contract else richer_contract.get("scope"),
                "richer_contract_tool_count": None if not richer_contract else richer_contract.get("tool_count"),
                "recommended_entrypoint": recommended_entrypoint,
                "recommendation_source": "official_bootstrap_tool",
                "recommended_contract_action": (
                    "stay_lean_until_shape_needed"
                    if tools_list_contract_mode == "lean"
                    else "stay_on_default_contract"
                ),
                "recommended_followup_tools": followup_tools[:3],
                "preferred_entrypoints_visible": preferred_visible,
                "preferred_entrypoints_in_prefetched_contract": [] if not richer_contract else [
                    tool
                    for tool in preferred_entrypoints
                    if tool in extract_tool_names(richer_contract)
                ],
                "auto_surface": project_data.get("auto_surface"),
                "auto_budget": project_data.get("auto_budget"),
                "indexed_files": project_data.get(
                    "indexed_files",
                    (config_data.get("symbol_index") or {}).get("indexed_files"),
                ),
                "frameworks": project_data.get("frameworks", config_data.get("frameworks", [])),
                "embedding_model": caps_data.get("embedding_model"),
                "embedding_indexed": caps_data.get("embedding_indexed"),
                "embedding_indexed_symbols": caps_data.get("embedding_indexed_symbols"),
                "activate_project_error": None,
                "preferred_entrypoints": preferred_entrypoints,
                "fallback_to_native": fallback_to_native,
                "init_response": init_response,
                "probe_strategy": "official_bootstrap_tool",
            }

        tool_list = mcp_http_call(
            f"{base_url}",
            "tools/list",
            request_id=request_id_base + 10,
            headers={"mcp-session-id": session_id} if session_id else None,
        )
        tool_list_primitive = mcp_http_call(
            f"{base_url}",
            "tools/list",
            {"tier": "primitive"},
            request_id=request_id_base + 11,
            headers={"mcp-session-id": session_id} if session_id else None,
        )
        activate = mcp_http_tool_call(
            base_url,
            "activate_project",
            {"project": str(repo_path)},
            request_id=request_id_base + 12,
            session_id=session_id,
        )
        activate_payload = extract_tool_payload(activate)
        if is_hidden_by_deferred_loading(activate_payload):
            tool_list_primitive = mcp_http_call(
                f"{base_url}",
                "tools/list",
                {"tier": "primitive"},
                request_id=request_id_base + 13,
                headers={"mcp-session-id": session_id} if session_id else None,
            )
            activate = mcp_http_tool_call(
                base_url,
                "activate_project",
                {"project": str(repo_path)},
                request_id=request_id_base + 14,
                session_id=session_id,
            )
            activate_payload = extract_tool_payload(activate)
        capabilities = mcp_http_tool_call(
            base_url,
            "get_capabilities",
            {},
            request_id=request_id_base + 15,
            session_id=session_id,
        )
        caps_payload = extract_tool_payload(capabilities)
        list_result = tool_list.get("result", {}) if isinstance(tool_list, dict) else {}
        primitive_result = (
            tool_list_primitive.get("result", {}) if isinstance(tool_list_primitive, dict) else {}
        )
        caps_data = caps_payload.get("data", {}) if isinstance(caps_payload, dict) else {}
        activate_data = activate_payload.get("data", {}) if isinstance(activate_payload, dict) else {}
        include_output_schema = bool(list_result.get("include_output_schema", True))
        tools_list_contract_mode = "full" if include_output_schema else "lean"
        schema_recovery_hint = None
        if not include_output_schema:
            schema_recovery_hint = (
                "Retry tools/list with includeOutputSchema=true only when output shapes matter; "
                "prefer narrowing by namespace or tier before requesting a full contract."
            )
        richer_contract = None
        if tools_list_contract_mode == "lean" and should_prefetch_richer_tools_contract(brief):
            richer_contract = fetch_richer_tools_contract(base_url, session_id, request_id_base + 7)
        routing_recommendation = build_codex_routing_recommendation(
            brief,
            list_result,
            primitive_result,
            richer_contract,
            tools_list_contract_mode,
        )
        return {
            "available": True,
            "session_id": session_id,
            "tool_count": list_result.get("tool_count", len(list_result.get("tools", []))),
            "tool_count_total": list_result.get(
                "tool_count_total",
                list_result.get("tool_count", len(list_result.get("tools", []))),
            ),
            "effective_namespaces": list_result.get("effective_namespaces", []),
            "preferred_namespaces": list_result.get("preferred_namespaces", []),
            "loaded_tiers": primitive_result.get("loaded_tiers", list_result.get("loaded_tiers", [])),
            "deferred_loading_active": list_result.get("deferred_loading_active"),
            "include_output_schema": include_output_schema,
            "tools_list_contract_mode": tools_list_contract_mode,
            "schema_recovery_hint": schema_recovery_hint,
            "richer_contract_prefetched": bool(richer_contract and richer_contract.get("prefetched")),
            "richer_contract_scope": None if not richer_contract else richer_contract.get("scope"),
            "richer_contract_tool_count": None if not richer_contract else richer_contract.get("tool_count"),
            "recommended_entrypoint": routing_recommendation.get("recommended_entrypoint"),
            "recommendation_source": routing_recommendation.get("recommendation_source"),
            "recommended_contract_action": routing_recommendation.get("recommended_contract_action"),
            "recommended_followup_tools": routing_recommendation.get("recommended_followup_tools"),
            "preferred_entrypoints_visible": routing_recommendation.get("preferred_entrypoints_visible"),
            "preferred_entrypoints_in_prefetched_contract": routing_recommendation.get(
                "preferred_entrypoints_in_prefetched_contract"
            ),
            "auto_surface": activate_data.get("auto_surface"),
            "auto_budget": activate_data.get("auto_budget"),
            "indexed_files": activate_data.get("indexed_files"),
            "frameworks": activate_data.get("frameworks", []),
            "embedding_model": caps_data.get("embedding_model"),
            "embedding_indexed": caps_data.get("embedding_indexed"),
            "embedding_indexed_symbols": caps_data.get("embedding_indexed_symbols"),
            "activate_project_error": activate_payload.get("error"),
            "preferred_entrypoints": preferred_entrypoints,
            "fallback_to_native": fallback_to_native,
            "init_response": init_response,
            "probe_strategy": "legacy_round_trip_fallback",
        }
    except URLError as exc:
        return {
            "available": False,
            "error": str(exc),
            "preferred_entrypoints": preferred_entrypoints,
            "fallback_to_native": True,
        }
    except Exception as exc:
        return {
            "available": False,
            "error": str(exc),
            "preferred_entrypoints": preferred_entrypoints,
            "fallback_to_native": True,
        }


def capture_metrics_snapshot(base_url: str, request_id: int, session_id: str | None = None):
    return mcp_http_tool_call(
        base_url,
        "get_tool_metrics",
        {},
        request_id=request_id,
        session_id=session_id,
    )


def delta_mapping(before: dict, after: dict):
    result = {}
    for key, after_value in after.items():
        before_value = before.get(key, 0)
        if isinstance(after_value, dict) and isinstance(before_value, dict):
            result[key] = delta_mapping(before_value, after_value)
        elif isinstance(after_value, (int, float)):
            result[key] = after_value - (before_value if isinstance(before_value, (int, float)) else 0)
        else:
            result[key] = after_value
    return result


def delta_keyed_list(before_list, after_list, key_name):
    before_map = {item[key_name]: item for item in before_list if key_name in item}
    rows = []
    for item in after_list:
        key = item.get(key_name)
        if key is None:
            continue
        before_item = before_map.get(key, {})
        delta = {f"{key_name}": key}
        for field, value in item.items():
            if field == key_name:
                continue
            before_value = before_item.get(field, 0)
            if isinstance(value, (int, float)):
                delta[field] = value - (before_value if isinstance(before_value, (int, float)) else 0)
            else:
                delta[field] = value
        numeric_values = [value for field, value in delta.items() if field != key_name and isinstance(value, (int, float))]
        if any(value != 0 for value in numeric_values):
            rows.append(delta)
    return rows


def safe_ratio(num, den):
    if not den:
        return 0.0
    return float(num) / float(den)


def recompute_derived(session_delta: dict):
    composite_calls = session_delta.get("composite_calls", 0)
    total_calls = session_delta.get("total_calls", 0)
    quality_contracts = session_delta.get("quality_contract_emitted_count", 0)
    verifier_contracts = session_delta.get("verifier_contract_emitted_count", 0)
    recommended_checks = session_delta.get("recommended_checks_emitted_count", 0)
    truncated = session_delta.get("truncated_response_count", 0)
    guidance_emitted = session_delta.get("composite_guidance_emitted_count", 0)
    expansions = session_delta.get("deferred_namespace_expansion_count", 0)
    mutation_checks = session_delta.get("mutation_preflight_checked_count", 0)
    return {
        "composite_ratio": safe_ratio(composite_calls, total_calls),
        "quality_contract_present_rate": safe_ratio(quality_contracts, composite_calls),
        "verifier_contract_present_rate": safe_ratio(verifier_contracts, composite_calls),
        "blocker_emit_rate": safe_ratio(session_delta.get("blocker_emit_count", 0), verifier_contracts),
        "verifier_followthrough_rate": safe_ratio(
            session_delta.get("verifier_followthrough_count", 0),
            verifier_contracts,
        ),
        "recommended_check_followthrough_rate": safe_ratio(
            session_delta.get("recommended_check_followthrough_count", 0),
            recommended_checks,
        ),
        "handle_reuse_rate": safe_ratio(
            session_delta.get("handle_reuse_count", 0),
            max(
                session_delta.get("analysis_summary_reads", 0)
                + session_delta.get("analysis_section_reads", 0),
                0,
            ),
        ),
        "quality_focus_reuse_rate": safe_ratio(
            session_delta.get("quality_focus_reuse_count", 0),
            quality_contracts,
        ),
        "performance_watchpoint_emit_rate": safe_ratio(
            session_delta.get("performance_watchpoint_emit_count", 0),
            quality_contracts,
        ),
        "composite_guidance_followthrough_rate": safe_ratio(
            session_delta.get("composite_guidance_followed_count", 0),
            guidance_emitted,
        ),
        "mutation_preflight_gate_deny_rate": safe_ratio(
            session_delta.get("mutation_preflight_gate_denied_count", 0),
            mutation_checks,
        ),
        "deferred_hidden_tool_call_deny_rate": safe_ratio(
            session_delta.get("deferred_hidden_tool_call_denied_count", 0),
            expansions,
        ),
        "truncation_followup_rate": safe_ratio(
            session_delta.get("truncation_followup_count", 0),
            truncated,
        ),
    }


def subtract_metrics_capture_overhead(delta_payload: dict):
    tools = []
    capture_tool = None
    for row in delta_payload.get("tools", []):
        if row.get("tool") == "get_tool_metrics":
            capture_tool = row
            continue
        tools.append(row)
    if not capture_tool:
        delta_payload["capture_overhead_subtracted"] = False
        return delta_payload

    calls = int(capture_tool.get("calls") or 0)
    success_count = int(capture_tool.get("success_count") or 0)
    errors = int(capture_tool.get("errors") or 0)
    total_ms = int(capture_tool.get("total_ms") or 0)
    total_tokens = int(capture_tool.get("total_tokens") or 0)

    session = delta_payload.get("session", {})
    adjustments = {
        "total_calls": calls,
        "success_count": success_count,
        "error_count": errors,
        "total_ms": total_ms,
        "total_tokens": total_tokens,
        "composite_calls": calls,
        "timeline_length": calls,
        "composite_guidance_emitted_count": calls,
        "composite_guidance_followed_count": calls,
    }
    for key, value in adjustments.items():
        if key in session and isinstance(session[key], (int, float)):
            session[key] = max(session[key] - value, 0)

    if "count" in delta_payload and isinstance(delta_payload["count"], int):
        delta_payload["count"] = max(delta_payload["count"] - 1, 0)

    delta_payload["tools"] = tools
    delta_payload["derived_kpis"] = recompute_derived(session)
    delta_payload["capture_overhead_subtracted"] = True
    delta_payload["capture_overhead"] = {
        "tool": "get_tool_metrics",
        "calls": calls,
        "success_count": success_count,
        "errors": errors,
        "total_ms": total_ms,
        "total_tokens": total_tokens,
    }
    return delta_payload


def build_metrics_delta(session_eval, before_raw: dict, after_raw: dict):
    before_payload = session_eval.unwrap_metrics_payload(before_raw)
    after_payload = session_eval.unwrap_metrics_payload(after_raw)
    before_session = before_payload.get("session") or {}
    after_session = after_payload.get("session") or {}
    delta_session = delta_mapping(before_session, after_session)
    delta_tools = delta_keyed_list(
        before_payload.get("tools") or before_payload.get("per_tool") or [],
        after_payload.get("tools") or after_payload.get("per_tool") or [],
        "tool",
    )
    delta_surfaces = delta_keyed_list(
        before_payload.get("surfaces") or before_payload.get("per_surface") or [],
        after_payload.get("surfaces") or after_payload.get("per_surface") or [],
        "surface",
    )
    payload = {
        "count": max(int(after_payload.get("count") or 0) - int(before_payload.get("count") or 0), 0),
        "session": delta_session,
        "derived_kpis": recompute_derived(delta_session),
        "tools": delta_tools,
        "surfaces": delta_surfaces,
    }
    return subtract_metrics_capture_overhead(payload)


def write_session_entry_artifacts(
    *,
    session_eval,
    session_entry: dict,
    run_dir: Path,
    session_entry_json_path: str,
    session_entry_md_path: str,
    archive_suffix: str,
    repo_id: str,
    task_kind: str,
    mode: str,
):
    entry_json = Path(session_entry_json_path).expanduser() if session_entry_json_path else run_dir / "session-entry.json"
    entry_md = Path(session_entry_md_path).expanduser() if session_entry_md_path else run_dir / "session-entry.md"
    entry_json.write_text(json.dumps(session_entry, ensure_ascii=False, indent=2) + "\n")
    entry_md.write_text(session_eval.render_markdown(session_entry) + "\n")

    archive_dir = session_eval.DEFAULT_REPORT_DIR
    archive_dir.mkdir(parents=True, exist_ok=True)
    archive_base = (
        f"{datetime.now().strftime('%Y%m%d-%H%M%S')}-"
        f"{slugify(repo_id)}-"
        f"{slugify(task_kind)}-"
        f"{slugify(mode)}"
        f"{archive_suffix}"
    )
    archive_entry_json = archive_dir / f"{archive_base}.json"
    archive_entry_md = archive_dir / f"{archive_base}.md"
    archive_entry_json.write_text(json.dumps(session_entry, ensure_ascii=False, indent=2) + "\n")
    archive_entry_md.write_text(session_eval.render_markdown(session_entry) + "\n")

    return {
        "session_entry_json": str(entry_json),
        "session_entry_markdown": str(entry_md),
        "archived_session_entry_json": str(archive_entry_json),
        "archived_session_entry_markdown": str(archive_entry_md),
        "archive_entry_json_path": archive_entry_json,
    }


def run_harness_eval(harness_eval_script: Path, *, repo_path: Path, archive_entry_json: Path, output_json: Path, output_md: Path, label: str, base_report_path: str = ""):
    harness_cmd = [
        "python3",
        str(harness_eval_script),
        "--repo",
        str(repo_path),
        "--skip-synthetic",
        "--no-default-session-glob",
        "--session-entry-glob",
        str(archive_entry_json),
        "--output-json",
        str(output_json),
        "--output-md",
        str(output_md),
        "--label",
        label,
    ]
    if base_report_path:
        harness_cmd.extend(["--base-report", str(base_report_path)])
    harness_result = subprocess.run(harness_cmd, check=True, capture_output=True, text=True)
    return json.loads(harness_result.stdout)


def run_refresh(refresh_policy_script: Path, *, label: str, output_json: Path):
    refresh_cmd = [
        "python3",
        str(refresh_policy_script),
        "--label",
        label,
    ]
    refresh_result = subprocess.run(refresh_cmd, check=True, capture_output=True, text=True)
    refresh_payload = json.loads(refresh_result.stdout)
    output_json.write_text(json.dumps(refresh_payload, ensure_ascii=False, indent=2) + "\n")
    return refresh_payload


def build_entry_args(
    *,
    repo_path: Path,
    repo: dict,
    scenario: dict | None,
    task_kind: str,
    mode: str,
    agent: str,
    session_eval,
    acceptance_passed: str,
    verify_passed: str,
    quality_score: str,
    recommended_policy: str,
    notes: str,
    recommendation_outcome: dict | None = None,
):
    return argparse.Namespace(
        repo=str(repo_path),
        repo_id=repo.get("id", ""),
        repo_label=repo.get("label", ""),
        scenario_id=scenario.get("scenario_id") if scenario else None,
        captured_at=datetime.now().isoformat(timespec="seconds"),
        task_kind=task_kind,
        mode=mode,
        agent=agent,
        acceptance_passed=session_eval.parse_bool_flag(acceptance_passed) if acceptance_passed else None,
        verify_passed=session_eval.parse_bool_flag(verify_passed) if verify_passed else None,
        success=True,
        quality_score=float(quality_score) if quality_score else None,
        recommended_policy=recommended_policy,
        notes=notes,
        recommendation_outcome=recommendation_outcome,
    )
