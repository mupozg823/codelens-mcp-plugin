#!/usr/bin/env bash
# Dogfood CodeLens through its own read-only HTTP daemon.
#
# This is intentionally one layer above transport smoke: it calls real CodeLens
# workflow tools over MCP (`prepare_harness_session`, `review_architecture`) and
# fails when the running daemon is stale, unhealthy, or unable to analyze this
# repository through the public tool surface.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
READONLY_URL="${CODELENS_READONLY_URL:-http://127.0.0.1:7839}"
EXPECTED_GIT_SHA="${CODELENS_EXPECTED_GIT_SHA:-}"

if [[ -z "$EXPECTED_GIT_SHA" ]] && command -v git >/dev/null 2>&1; then
  EXPECTED_GIT_SHA="$(git -C "$ROOT" rev-parse --short=7 HEAD 2>/dev/null || true)"
fi

python3 - "$READONLY_URL" "$ROOT" "$EXPECTED_GIT_SHA" <<'PY'
import json
import sys
import urllib.error
import urllib.request

base_url, project_root, expected_git_sha = sys.argv[1:4]


def request(method, path, payload=None, headers=None):
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    req_headers = {
        "accept": "application/json",
        "content-type": "application/json",
    }
    if headers:
        req_headers.update(headers)
    req = urllib.request.Request(
        base_url.rstrip("/") + path,
        data=data,
        headers=req_headers,
        method=method,
    )
    with urllib.request.urlopen(req, timeout=20) as response:
        return response.status, response.headers, response.read().decode("utf-8")


def rpc(session_id, request_id, method, params=None):
    payload = {"jsonrpc": "2.0", "id": request_id, "method": method}
    if params is not None:
        payload["params"] = params
    status, _, body = request(
        "POST",
        "/mcp",
        payload,
        {"mcp-session-id": session_id, "mcp-protocol-version": "2025-11-25"},
    )
    assert status == 200, status
    envelope = json.loads(body)
    if "error" in envelope:
        raise AssertionError(envelope["error"])
    return envelope["result"]


def tool_call(session_id, request_id, name, arguments):
    return rpc(
        session_id,
        request_id,
        "tools/call",
        {"name": name, "arguments": arguments},
    )


def structured(result):
    if isinstance(result.get("structuredContent"), dict):
        return result["structuredContent"]
    content = result.get("content") or []
    if content and isinstance(content[0], dict) and isinstance(content[0].get("text"), str):
        parsed = json.loads(content[0]["text"])
        if isinstance(parsed, dict) and isinstance(parsed.get("data"), dict):
            return parsed["data"]
        return parsed
    raise AssertionError("tool result did not include structured content")


def git_sha_matches(actual, expected):
    if not actual or not expected:
        return False
    return actual.startswith(expected) or expected.startswith(actual)


status, headers, body = request(
    "POST",
    "/mcp",
    {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "clientInfo": {"name": "CodeLensDogfoodSmoke", "version": "1.0.0"},
            "protocolVersion": "2025-11-25",
        },
    },
)
assert status == 200, status
session_id = headers.get("mcp-session-id")
assert session_id, "initialize response did not include mcp-session-id"

try:
    prepare_result = tool_call(
        session_id,
        2,
        "prepare_harness_session",
        {
            "project": project_root,
            "profile": "planner-readonly",
            "task_overlay": "review",
            "host_context": "codex",
            "detail": "compact",
            "preferred_entrypoints": [
                "review_architecture",
                "review_changes",
                "get_current_config",
            ],
            "auto_refresh_stale": True,
            "auto_refresh_stale_threshold": 32,
        },
    )
    prepare = structured(prepare_result)
    assert prepare["activated"] is True, prepare
    assert prepare["project"]["project_name"] == "codelens-mcp-plugin", prepare
    assert prepare["project"]["indexed_files"] > 0, prepare
    assert prepare["health_summary"]["status"] == "ok", prepare
    assert prepare.get("warnings", []) == [], prepare
    visible = set(prepare["visible_tools"]["tool_names"])
    assert "review_architecture" in visible, prepare
    runtime = prepare["surface_generation"]["runtime"]
    if expected_git_sha:
        assert git_sha_matches(runtime["binary_git_sha"], expected_git_sha), prepare

    architecture_result = tool_call(
        session_id,
        3,
        "review_architecture",
        {
            "path": "scripts",
            "include_diagram": False,
            "max_nodes": 40,
        },
    )
    architecture = structured(architecture_result)
    assert architecture["analysis_id"].startswith("analysis-"), architecture
    assert architecture["blocker_count"] == 0, architecture
    assert architecture["readiness_score"] >= 0.99, architecture
    assert architecture["confidence"] >= 0.5, architecture
    assert architecture["summary_resource"]["uri"].startswith("codelens://analysis/"), architecture

    session_result = rpc(
        session_id,
        4,
        "resources/read",
        {"uri": "codelens://session/http"},
    )
    session = json.loads(session_result["contents"][0]["text"])
    assert session["daemon_mode"] == "read-only", session
    assert session["active_surface"] == "planner-readonly", session
    assert session["health_summary"]["status"] == "ok", session
    drift = session["daemon_binary_drift"]
    assert drift["stale_daemon"] is False, drift
    assert drift["restart_recommended"] is False, drift
    if expected_git_sha:
        assert git_sha_matches(drift["binary_git_sha"], expected_git_sha), drift
        assert git_sha_matches(drift["head_git_sha"], expected_git_sha), drift
finally:
    try:
        request(
            "DELETE",
            "/mcp",
            headers={
                "mcp-session-id": session_id,
                "mcp-protocol-version": "2025-11-25",
            },
        )
    except urllib.error.HTTPError as exc:
        if exc.code >= 500:
            raise

print(
    "PASS codelens-dogfood: prepare_harness_session + review_architecture "
    f"via {base_url} git={expected_git_sha or 'unchecked'}"
)
PY
