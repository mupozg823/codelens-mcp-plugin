#!/usr/bin/env bash
# Smoke-test the repo-local enterprise dual-daemon shape.
#
# This assumes `scripts/install-http-daemons-launchd.sh . --load` or an
# equivalent supervisor has already started:
#   - read-only reviewer/planner daemon on 127.0.0.1:7839
#   - mutation-enabled builder/refactor daemon on 127.0.0.1:7838
#
# The check intentionally uses the public HTTP MCP protocol instead of
# launchd internals so it also works for systemd/Docker deployments that
# expose the same dual-daemon contract.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
READONLY_URL="${CODELENS_READONLY_URL:-http://127.0.0.1:7839}"
MUTATION_URL="${CODELENS_MUTATION_URL:-http://127.0.0.1:7838}"
EXPECTED_GIT_SHA="${CODELENS_EXPECTED_GIT_SHA:-}"

if [[ -z "$EXPECTED_GIT_SHA" ]] && command -v git >/dev/null 2>&1; then
  EXPECTED_GIT_SHA="$(git -C "$ROOT" rev-parse --short=7 HEAD 2>/dev/null || true)"
fi

python3 - "$READONLY_URL" "$MUTATION_URL" "$EXPECTED_GIT_SHA" <<'PY'
import json
import sys
import urllib.error
import urllib.request

readonly_url, mutation_url, expected_git_sha = sys.argv[1:4]


def request(base, method, path, payload=None, headers=None):
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    req_headers = {
        "accept": "application/json",
        "content-type": "application/json",
    }
    if headers:
        req_headers.update(headers)
    req = urllib.request.Request(
        base.rstrip("/") + path,
        data=data,
        headers=req_headers,
        method=method,
    )
    with urllib.request.urlopen(req, timeout=5) as response:
        return response.status, response.headers, response.read().decode("utf-8")


def smoke_daemon(base, expected_surface, expected_mode):
    status, _, body = request(base, "GET", "/.well-known/mcp.json")
    assert status == 200, status
    card = json.loads(body)
    assert card["name"] == "codelens-mcp", card
    assert card["active_surface"] == expected_surface, card
    assert card["daemon_mode"] == expected_mode, card
    assert "streamable-http" in card["transport"], card
    assert "semantic-search" in card["features"], card

    status, headers, body = request(
        base,
        "POST",
        "/mcp",
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "EnterpriseDaemonSmoke",
                    "version": "1.0.0",
                },
                "protocolVersion": "2025-11-25",
            },
        },
    )
    assert status == 200, status
    session_id = headers.get("mcp-session-id")
    assert session_id, "initialize response did not include mcp-session-id"

    status, _, body = request(
        base,
        "POST",
        "/mcp",
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
        {"mcp-session-id": session_id, "mcp-protocol-version": "2025-11-25"},
    )
    assert status == 200, status
    tools_payload = json.loads(body)
    assert tools_payload["result"]["active_surface"] == expected_surface, tools_payload
    tool_names = {tool["name"] for tool in tools_payload["result"]["tools"]}
    assert "get_current_config" in tool_names, sorted(tool_names)

    status, _, body = request(
        base,
        "POST",
        "/mcp",
        {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "resources/read",
            "params": {"uri": "codelens://session/http"},
        },
        {"mcp-session-id": session_id, "mcp-protocol-version": "2025-11-25"},
    )
    assert status == 200, status
    resource_payload = json.loads(body)
    session = json.loads(resource_payload["result"]["contents"][0]["text"])
    assert session["active_surface"] == expected_surface, session
    assert session["daemon_mode"] == expected_mode, session
    assert session["semantic_search_status"] == "available", session
    assert session["health_summary"]["status"] == "ok", session
    drift = session["daemon_binary_drift"]
    assert drift["stale_daemon"] is False, drift
    assert drift["restart_recommended"] is False, drift
    if expected_git_sha:
        assert git_sha_matches(drift["binary_git_sha"], expected_git_sha), drift
        assert git_sha_matches(drift["head_git_sha"], expected_git_sha), drift

    try:
        request(
            base,
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

    return {
        "url": base,
        "surface": expected_surface,
        "mode": expected_mode,
        "git": drift["binary_git_sha"],
    }


def git_sha_matches(actual, expected):
    if not actual or not expected:
        return False
    return actual.startswith(expected) or expected.startswith(actual)


results = [
    smoke_daemon(readonly_url, "reviewer-graph", "read-only"),
    smoke_daemon(mutation_url, "refactor-full", "mutation-enabled"),
]

for result in results:
    print(
        "PASS enterprise-daemon {url}: surface={surface} mode={mode} git={git}".format(
            **result
        )
    )
PY
