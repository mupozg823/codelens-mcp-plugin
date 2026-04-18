#!/bin/bash
set -euo pipefail

MCP_URL="${CODELENS_AUDIT_MCP_URL:-http://127.0.0.1:7837/mcp}"
TIMEOUT_SECS="${CODELENS_AUDIT_TIMEOUT_SECS:-10}"
POLL_INTERVAL_SECS="${CODELENS_AUDIT_POLL_INTERVAL_SECS:-0.5}"
MAX_POLLS="${CODELENS_AUDIT_MAX_POLLS:-20}"
DEFAULT_OUTPUT_DIR="${CODELENS_AUDIT_OUTPUT_DIR:-.codelens/reports}"
OUTPUT_PATH="${1:-}"

if [[ -z "$OUTPUT_PATH" ]]; then
	mkdir -p "$DEFAULT_OUTPUT_DIR"
	OUTPUT_PATH="$DEFAULT_OUTPUT_DIR/eval-session-audit-$(date +%Y%m%d-%H%M%S).json"
else
	mkdir -p "$(dirname "$OUTPUT_PATH")"
fi

python3 - "$OUTPUT_PATH" "$MCP_URL" "$TIMEOUT_SECS" "$POLL_INTERVAL_SECS" "$MAX_POLLS" <<'PY'
import json
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

output_path, mcp_url, timeout_secs, poll_interval_secs, max_polls = sys.argv[1:]
timeout_secs = float(timeout_secs)
poll_interval_secs = float(poll_interval_secs)
max_polls = int(max_polls)
request_id = 0


def nested_get(node, *path):
    current = node
    for key in path:
        if not isinstance(current, dict):
            return None
        current = current.get(key)
    return current


def first_present(node, *paths):
    for path in paths:
        value = nested_get(node, *path)
        if value is not None:
            return value
    return None


def rpc(method, params):
    global request_id
    request_id += 1
    body = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        }
    ).encode("utf-8")
    request = urllib.request.Request(
        mcp_url,
        data=body,
        headers={"content-type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout_secs) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except (urllib.error.URLError, TimeoutError) as exc:
        raise SystemExit(f"RPC {method} failed: {exc}") from exc
    if "error" in payload:
        raise SystemExit(f"RPC {method} returned error: {payload['error']}")
    return payload


def tool_call(name, arguments):
    return rpc("tools/call", {"name": name, "arguments": arguments})


def section_content(payload):
    content = first_present(
        payload,
        ("result", "structuredContent", "data", "content"),
        ("result", "structuredContent", "content"),
    )
    if content is None:
        raise SystemExit(f"Missing section content in payload: {json.dumps(payload)}")
    return content


start_payload = tool_call(
    "start_analysis_job",
    {"kind": "eval_session_audit", "profile_hint": "ci-audit"},
)
job_id = first_present(
    start_payload,
    ("result", "structuredContent", "data", "job_id"),
    ("result", "structuredContent", "job_id"),
)
if not isinstance(job_id, str) or not job_id:
    raise SystemExit(f"Missing job_id from start_analysis_job: {json.dumps(start_payload)}")

analysis_id = None
status = None
for _ in range(max_polls):
    job_payload = tool_call("get_analysis_job", {"job_id": job_id})
    status = first_present(
        job_payload,
        ("result", "structuredContent", "data", "status"),
        ("result", "structuredContent", "status"),
    )
    if status == "completed":
        analysis_id = first_present(
            job_payload,
            ("result", "structuredContent", "data", "analysis_id"),
            ("result", "structuredContent", "analysis_id"),
        )
        break
    if status in {"failed", "cancelled"}:
        raise SystemExit(f"eval_session_audit job {job_id} ended with status={status}")
    time.sleep(poll_interval_secs)

if not isinstance(analysis_id, str) or not analysis_id:
    raise SystemExit(
        f"Timed out waiting for eval_session_audit completion (job_id={job_id}, status={status or 'unknown'})"
    )

audit_pass_rate = section_content(
    tool_call(
        "get_analysis_section",
        {"analysis_id": analysis_id, "section": "audit_pass_rate"},
    )
)
session_rows = section_content(
    tool_call(
        "get_analysis_section",
        {"analysis_id": analysis_id, "section": "session_rows"},
    )
)

payload = {
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "mcp_url": mcp_url,
    "job_id": job_id,
    "analysis_id": analysis_id,
    "audit_pass_rate": audit_pass_rate,
    "session_rows": session_rows,
}
Path(output_path).write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
print(output_path)
PY
