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
            if mcp_preflight.get("embedding_indexed") is not None:
                lines.append(
                    f"- Semantic index ready={mcp_preflight.get('embedding_indexed')}, indexed_symbols={mcp_preflight.get('embedding_indexed_symbols', 0)}."
                )
            if mcp_preflight.get("preferred_entrypoints"):
                lines.append(
                    f"- Suggested bootstrap entrypoints: {', '.join(mcp_preflight['preferred_entrypoints'])}."
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
    with urllib.request.urlopen(req, timeout=5) as resp:
        parsed = json.loads(resp.read().decode("utf-8"))
        if include_headers:
            return parsed, {key.lower(): value for key, value in resp.headers.items()}
        return parsed


def mcp_http_tool_call(base_url: str, name: str, arguments: dict, request_id: int = 1, session_id: str | None = None):
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


def safe_capture_metrics_snapshot(base_url: str, request_id: int):
    try:
        return capture_metrics_snapshot(base_url, request_id=request_id), None
    except Exception as exc:
        return None, str(exc)


def is_hidden_by_deferred_loading(payload: dict) -> bool:
    error = str((payload or {}).get("error") or "")
    return "hidden by deferred loading" in error


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
        tool_list = mcp_http_call(
            f"{base_url}",
            "tools/list",
            request_id=request_id_base + 1,
            headers={"mcp-session-id": session_id} if session_id else None,
        )
        tool_list_primitive = mcp_http_call(
            f"{base_url}",
            "tools/list",
            {"tier": "primitive"},
            request_id=request_id_base + 2,
            headers={"mcp-session-id": session_id} if session_id else None,
        )
        activate = mcp_http_tool_call(
            base_url,
            "activate_project",
            {"project": str(repo_path)},
            request_id=request_id_base + 3,
            session_id=session_id,
        )
        activate_payload = extract_tool_payload(activate)
        if is_hidden_by_deferred_loading(activate_payload):
            tool_list_primitive = mcp_http_call(
                f"{base_url}",
                "tools/list",
                {"tier": "primitive"},
                request_id=request_id_base + 4,
                headers={"mcp-session-id": session_id} if session_id else None,
            )
            activate = mcp_http_tool_call(
                base_url,
                "activate_project",
                {"project": str(repo_path)},
                request_id=request_id_base + 5,
                session_id=session_id,
            )
            activate_payload = extract_tool_payload(activate)
        capabilities = mcp_http_tool_call(
            base_url,
            "get_capabilities",
            {},
            request_id=request_id_base + 6,
            session_id=session_id,
        )
        caps_payload = extract_tool_payload(capabilities)
        list_result = tool_list.get("result", {}) if isinstance(tool_list, dict) else {}
        primitive_result = (
            tool_list_primitive.get("result", {}) if isinstance(tool_list_primitive, dict) else {}
        )
        caps_data = caps_payload.get("data", {}) if isinstance(caps_payload, dict) else {}
        activate_data = activate_payload.get("data", {}) if isinstance(activate_payload, dict) else {}
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


def capture_metrics_snapshot(base_url: str, request_id: int):
    return mcp_http_tool_call(base_url, "get_tool_metrics", {}, request_id=request_id)


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
    )
