#!/usr/bin/env python3
"""
CodeLens MCP Plugin smoke test against the JetBrains IDE SSE endpoint.

This script avoids third-party dependencies and talks directly to the
JetBrains MCP Server exposed by the IDE on the local SSE transport.
"""

import http.client
import json
import queue
import sys
import threading
import time
import urllib.parse

HOST = "127.0.0.1"
PORT = 64342
TIMEOUT = 15
EVENT_QUEUE = queue.Queue()


def open_sse_session():
    conn = http.client.HTTPConnection(HOST, PORT, timeout=10)
    conn.request("GET", "/sse", headers={"Accept": "text/event-stream"})
    response = conn.getresponse()
    if response.status != 200:
        raise RuntimeError(f"SSE connect failed: {response.status} {response.reason}")
    return conn, response


def sse_reader(response):
    current_event = None
    data_lines = []
    try:
        while True:
            raw = response.readline()
            if not raw:
                break

            line = raw.decode("utf-8", errors="ignore").rstrip("\r\n")
            if line == "":
                payload = "\n".join(data_lines)
                if current_event == "endpoint":
                    EVENT_QUEUE.put(("endpoint", payload))
                elif current_event == "message":
                    try:
                        EVENT_QUEUE.put(("message", json.loads(payload)))
                    except json.JSONDecodeError:
                        EVENT_QUEUE.put(("parse_error", payload))
                current_event = None
                data_lines = []
                continue

            if line.startswith("event:"):
                current_event = line.split(":", 1)[1].strip()
            elif line.startswith("data:"):
                data_lines.append(line.split(":", 1)[1].lstrip())
    except Exception as exc:
        EVENT_QUEUE.put(("reader_error", repr(exc)))


def wait_for_endpoint(timeout=5):
    start = time.time()
    while time.time() - start < timeout:
        try:
            event_type, payload = EVENT_QUEUE.get(timeout=0.5)
        except queue.Empty:
            continue
        if event_type == "endpoint":
            return payload
        print(f"  [WARN] Ignoring SSE event before endpoint: {event_type}")
    return None


def post_json(path, payload):
    body = json.dumps(payload).encode("utf-8")
    conn = http.client.HTTPConnection(HOST, PORT, timeout=10)
    conn.request(
        "POST",
        path,
        body=body,
        headers={"Content-Type": "application/json"},
    )
    response = conn.getresponse()
    response.read()
    conn.close()
    return response.status, response.reason


def rpc_call(message_path, req_id, method, params=None):
    payload = {"jsonrpc": "2.0", "id": req_id, "method": method}
    if params is not None:
        payload["params"] = params
    return post_json(message_path, payload)


def rpc_notify(message_path, method, params=None):
    payload = {"jsonrpc": "2.0", "method": method}
    if params is not None:
        payload["params"] = params
    return post_json(message_path, payload)


def wait_for_response(req_id, timeout=TIMEOUT):
    start = time.time()
    stash = []
    while time.time() - start < timeout:
        try:
            event_type, payload = EVENT_QUEUE.get(timeout=0.5)
        except queue.Empty:
            continue

        if event_type == "message" and isinstance(payload, dict) and payload.get("id") == req_id:
            for item in stash:
                EVENT_QUEUE.put(item)
            return payload

        stash.append((event_type, payload))

    for item in stash:
        EVENT_QUEUE.put(item)
    return None


def decode_tool_payload(response):
    if not response or "result" not in response:
        return None, "missing result"

    result = response["result"]
    if result.get("isError"):
        return None, f"mcp isError=true: {result}"

    content = result.get("content", [])
    if not content:
        return None, "empty content"

    text = content[0].get("text", "")
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError:
        return None, f"tool returned non-JSON text: {text[:200]}"

    if not parsed.get("success", False):
        return None, parsed.get("error", text[:200])

    return parsed.get("data", {}), None


def main():
    print("=== Connecting to JetBrains MCP Server ===")
    try:
        sse_conn, sse_response = open_sse_session()
    except Exception as exc:
        print(f"ERROR: Cannot connect - {exc}")
        print("Is IntelliJ IDEA running with MCP Server enabled?")
        sys.exit(1)

    reader = threading.Thread(target=sse_reader, args=(sse_response,), daemon=True)
    reader.start()

    session_url = wait_for_endpoint()
    if not session_url:
        print("ERROR: Did not receive /message session URL from /sse")
        sys.exit(1)

    parsed = urllib.parse.urlparse(session_url)
    message_path = parsed.path
    if parsed.query:
        message_path += f"?{parsed.query}"
    print(f"Session: {message_path}")

    print("\n=== Step 1: Initialize ===")
    status, reason = rpc_call(
        message_path,
        1,
        "initialize",
        {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "codelens-test", "version": "1.0"},
        },
    )
    print(f"  POST initialize -> {status} {reason}")
    response = wait_for_response(1)
    if not response or "result" not in response:
        print(f"  ERROR: No initialize response. resp={response}")
        sys.exit(1)

    server_info = response["result"].get("serverInfo", {})
    print(f"  Server: {server_info.get('name', '?')} v{server_info.get('version', '?')}")

    status, reason = rpc_notify(message_path, "notifications/initialized")
    print(f"  POST notifications/initialized -> {status} {reason}")

    print("\n=== Step 2: List Tools ===")
    rpc_call(message_path, 2, "tools/list", {})
    response = wait_for_response(2)
    if not response or "result" not in response:
        print(f"  ERROR: No tools/list response. resp={response}")
        sys.exit(1)

    tools = response["result"].get("tools", [])
    print(f"  {len(tools)} total tools registered")

    codelens_tools = [
        "get_symbols_overview",
        "find_symbol",
        "find_referencing_symbols",
        "search_for_pattern",
        "get_type_hierarchy",
        "find_referencing_code_snippets",
        "replace_symbol_body",
        "insert_after_symbol",
        "insert_before_symbol",
        "rename_symbol",
        "read_file",
        "list_dir",
        "find_file",
        "create_text_file",
        "delete_lines",
        "insert_at_line",
        "replace_lines",
        "replace_content",
    ]
    available = {tool["name"] for tool in tools}
    found = [name for name in codelens_tools if name in available]
    missing = [name for name in codelens_tools if name not in available]

    print(f"  CodeLens tools present: {len(found)}/{len(codelens_tools)}")
    if missing:
        print(f"  Missing: {', '.join(missing)}")

    if not found:
        print("ERROR: CodeLens tools are not visible through the MCP server")
        sys.exit(1)

    checks = [
        (
            "get_symbols_overview",
            {
                "path": "src/main/kotlin/com/codelens/tools/BaseMcpTool.kt",
                "depth": 1,
            },
            lambda data: data.get("count", 0) >= 1,
        ),
        (
            "find_symbol",
            {
                "name": "BaseMcpTool",
                "file_path": "src/main/kotlin/com/codelens/tools/BaseMcpTool.kt",
                "include_body": False,
            },
            lambda data: data.get("count", 0) >= 1,
        ),
        (
            "search_for_pattern",
            {
                "pattern": "override val toolName",
                "file_glob": "*.kt",
                "max_results": 3,
            },
            lambda data: data.get("count", 0) >= 1,
        ),
        (
            "list_dir",
            {
                "relative_path": "src/main/kotlin/com/codelens/tools",
                "recursive": False,
            },
            lambda data: data.get("count", 0) >= 1,
        ),
    ]

    print("\n=== Step 3: Smoke Tests ===")
    results = []
    req_id = 10
    for tool_name, arguments, validator in checks:
        print(f"\n--- {tool_name} ---")
        rpc_call(
            message_path,
            req_id,
            "tools/call",
            {"name": tool_name, "arguments": arguments},
        )
        response = wait_for_response(req_id)
        parsed_payload, error = decode_tool_payload(response)
        if error:
            print(f"  FAIL: {error}")
            results.append((tool_name, "FAIL"))
        elif validator(parsed_payload):
            print(f"  PASS: {json.dumps(parsed_payload, ensure_ascii=False)[:180]}")
            results.append((tool_name, "PASS"))
        else:
            print(f"  FAIL: unexpected payload {json.dumps(parsed_payload, ensure_ascii=False)[:180]}")
            results.append((tool_name, "FAIL"))
        req_id += 1

    print("\n=== Summary ===")
    passed = 0
    for tool_name, status_name in results:
        print(f"  {status_name}: {tool_name}")
        if status_name == "PASS":
            passed += 1
    print(f"\nResult: {passed}/{len(results)} passed")

    sse_conn.close()
    if passed != len(results):
        sys.exit(1)


if __name__ == "__main__":
    main()
