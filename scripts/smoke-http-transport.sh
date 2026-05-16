#!/usr/bin/env bash
# Smoke-test the real HTTP daemon path with a built codelens-mcp binary.
#
# The in-process Rust HTTP tests cover protocol details. This script covers
# the release/operator path: build a binary with `--features http`, launch it
# on a loopback port, initialize an MCP HTTP session, call tools/list, and
# terminate the session.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${CODELENS_HTTP_BIN:-"$ROOT/target/debug/codelens-mcp"}"

if [[ -z "${CODELENS_HTTP_BIN:-}" ]]; then
  cargo build -p codelens-mcp --features http
fi

if [[ ! -x "$BIN" ]]; then
  echo "missing executable HTTP binary: $BIN" >&2
  exit 2
fi

PORT="$(
  python3 - <<'PY'
import socket

with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)"

TMP_DIR="$(mktemp -d)"
LOG="$TMP_DIR/codelens-http.log"
PID=""

cleanup() {
  local status=$?
  if [[ -n "$PID" ]] && kill -0 "$PID" 2>/dev/null; then
    kill "$PID" 2>/dev/null || true
    wait "$PID" 2>/dev/null || true
  fi
  if [[ "$status" -ne 0 && -s "$LOG" ]]; then
    echo "---- codelens http log tail ----" >&2
    tail -80 "$LOG" >&2 || true
  fi
  rm -rf "$TMP_DIR"
  exit "$status"
}
trap cleanup EXIT

"$BIN" "$ROOT" --transport http --listen 127.0.0.1 --port "$PORT" --auth off \
  >"$TMP_DIR/codelens-http.stdout" \
  2>"$LOG" &
PID=$!

python3 - "$PORT" <<'PY'
import json
import sys
import time
import urllib.error
import urllib.request

port = int(sys.argv[1])
base = "http://127.0.0.1:{}".format(port)


def request(method, path, payload=None, headers=None):
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    req_headers = {
        "accept": "application/json",
        "content-type": "application/json",
    }
    if headers:
        req_headers.update(headers)
    req = urllib.request.Request(
        base + path,
        data=data,
        headers=req_headers,
        method=method,
    )
    with urllib.request.urlopen(req, timeout=5) as response:
        body = response.read().decode("utf-8")
        return response.status, response.headers, body


deadline = time.time() + 10
last_error = None
while time.time() < deadline:
    try:
        status, _, body = request("GET", "/.well-known/mcp.json")
        if status == 200 and '"latestProtocolVersion": "2025-11-25"' in body:
            break
    except (urllib.error.URLError, TimeoutError, ConnectionError) as exc:
        last_error = exc
        time.sleep(0.1)
else:
    raise SystemExit("HTTP daemon did not become ready: {!r}".format(last_error))

status, headers, body = request(
    "POST",
    "/mcp",
    {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "clientInfo": {"name": "SmokeHarness", "version": "1.0.0"},
            "protocolVersion": "2025-11-25",
        },
    },
)
assert status == 200, status
session_id = headers.get("mcp-session-id")
assert session_id, "initialize response did not include mcp-session-id"
initialize_payload = json.loads(body)
assert initialize_payload["result"]["serverInfo"]["name"] == "codelens-mcp"

status, _, body = request(
    "POST",
    "/mcp",
    {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
    {"mcp-session-id": session_id, "mcp-protocol-version": "2025-11-25"},
)
assert status == 200, status
tools_payload = json.loads(body)
tools = tools_payload["result"]["tools"]
names = {tool["name"] for tool in tools}
for required in ("get_current_config", "review_architecture", "start_analysis_job"):
    assert required in names, "missing {} in tools/list".format(required)

try:
    request(
        "DELETE",
        "/mcp",
        headers={"mcp-session-id": session_id, "mcp-protocol-version": "2025-11-25"},
    )
except urllib.error.HTTPError as exc:
    if exc.code >= 500:
        raise
PY

printf 'PASS smoke-http-transport: %s on 127.0.0.1:%s\n' "$("$BIN" --version)" "$PORT"
