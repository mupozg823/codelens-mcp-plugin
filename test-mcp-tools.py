#!/usr/bin/env python3
"""
CodeLens MCP Plugin Tool Verification Script.
Uses a single SSE connection for both session and response handling.
"""

import json, sys, time, threading, queue, socket

HOST = "127.0.0.1"
PORT = 64342
BASE = f"http://{HOST}:{PORT}"
response_queue = queue.Queue()


def raw_sse_connect():
    """Open a raw socket SSE connection and return (socket, session_url)."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.settimeout(30)
    sock.connect((HOST, PORT))
    sock.sendall(
        b"GET /sse HTTP/1.1\r\nHost: 127.0.0.1:64342\r\nAccept: text/event-stream\r\n\r\n"
    )

    # Read HTTP response header
    buf = b""
    while b"\r\n\r\n" not in buf:
        buf += sock.recv(1)

    # Read SSE events to get session URL
    session_url = None
    line_buf = b""
    while not session_url:
        ch = sock.recv(1)
        line_buf += ch
        if line_buf.endswith(b"\n"):
            line = line_buf.decode().strip()
            if line.startswith("data: /message"):
                session_url = line[6:]
            line_buf = b""

    return sock, session_url


def sse_reader(sock):
    """Background thread that reads SSE events from the socket."""
    buf = b""
    while True:
        try:
            ch = sock.recv(1)
            if not ch:
                break
            buf += ch
            if buf.endswith(b"\n\n"):
                for line in buf.decode().strip().split("\n"):
                    if line.startswith("data: ") and not line.startswith(
                        "data: /message"
                    ):
                        try:
                            data = json.loads(line[6:])
                            response_queue.put(data)
                        except json.JSONDecodeError:
                            pass
                buf = b""
        except socket.timeout:
            continue
        except Exception:
            break


def send_post(session_url, payload):
    """Send a POST request to the message endpoint."""
    body = json.dumps(payload).encode()
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.settimeout(5)
    sock.connect((HOST, PORT))
    req = (
        f"POST {session_url} HTTP/1.1\r\n"
        f"Host: {HOST}:{PORT}\r\n"
        f"Content-Type: application/json\r\n"
        f"Content-Length: {len(body)}\r\n"
        f"\r\n"
    ).encode() + body
    sock.sendall(req)
    # Read response (202 Accepted)
    resp = b""
    try:
        while b"\r\n\r\n" not in resp:
            resp += sock.recv(1024)
    except:
        pass
    sock.close()


def rpc_call(session_url, method, params=None, req_id=1):
    msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
    if params:
        msg["params"] = params
    send_post(session_url, msg)


def rpc_notify(session_url, method, params=None):
    msg = {"jsonrpc": "2.0", "method": method}
    if params:
        msg["params"] = params
    send_post(session_url, msg)


def wait_response(req_id, timeout=10):
    start = time.time()
    stash = []
    while time.time() - start < timeout:
        try:
            data = response_queue.get(timeout=0.5)
            if data.get("id") == req_id:
                for s in stash:
                    response_queue.put(s)
                return data
            stash.append(data)
        except queue.Empty:
            continue
    for s in stash:
        response_queue.put(s)
    return None


def main():
    print("=== Connecting to JetBrains MCP Server ===")
    try:
        sse_sock, session_url = raw_sse_connect()
    except Exception as e:
        print(f"ERROR: Cannot connect - {e}")
        print("Is IntelliJ IDEA running with MCP Server enabled?")
        sys.exit(1)
    print(f"Session: {session_url}")

    # Start background SSE reader
    reader = threading.Thread(target=sse_reader, args=(sse_sock,), daemon=True)
    reader.start()
    time.sleep(0.3)

    # Initialize
    print("\n=== Step 1: Initialize ===")
    rpc_call(
        session_url,
        "initialize",
        {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "codelens-test", "version": "1.0"},
        },
        req_id=1,
    )
    resp = wait_response(1, timeout=10)
    if resp and "result" in resp:
        info = resp["result"].get("serverInfo", {})
        print(f"  Server: {info.get('name', '?')} v{info.get('version', '?')}")
    else:
        print(f"  ERROR: No response from initialize. resp={resp}")
        sys.exit(1)

    rpc_notify(session_url, "notifications/initialized")
    time.sleep(0.5)

    # List tools
    print("\n=== Step 2: List Tools ===")
    rpc_call(session_url, "tools/list", req_id=2)
    resp = wait_response(2, timeout=10)
    tools = []
    if resp and "result" in resp:
        tools = resp["result"].get("tools", [])
        print(f"  {len(tools)} tools registered:\n")
        for t in tools:
            print(f"  - {t['name']}")
    else:
        print(f"  ERROR: {resp}")
        sys.exit(1)

    # Check for CodeLens tools
    CL_TOOLS = [
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
    found = [t["name"] for t in tools if t["name"] in CL_TOOLS]
    missing = [n for n in CL_TOOLS if n not in found]

    print(f"\n=== CodeLens Tools: {len(found)}/{len(CL_TOOLS)} ===")
    for name in found:
        print(f"  ✅ {name}")
    for name in missing:
        print(f"  ❌ {name}")

    if not found:
        print("\nCodeLens MCP plugin is NOT installed or not loaded.")
        print(f"All available tools: {[t['name'] for t in tools]}")
        sys.exit(1)

    # Test tools
    req_id = 10
    test_results = {}

    # Test 1: get_symbols_overview
    if "get_symbols_overview" in found:
        print(f"\n--- Test: get_symbols_overview ---")
        rpc_call(
            session_url,
            "tools/call",
            {"name": "get_symbols_overview", "arguments": {"path": ".", "depth": 0}},
            req_id=req_id,
        )
        resp = wait_response(req_id, timeout=15)
        if resp and "result" in resp:
            text = resp["result"].get("content", [{}])[0].get("text", "")
            is_error = resp["result"].get("isError", False)
            status = "FAIL" if is_error else "PASS"
            print(f"  [{status}] ({len(text)} chars) {text[:150]}")
            test_results["get_symbols_overview"] = status
        else:
            print(f"  [FAIL] No response")
            test_results["get_symbols_overview"] = "FAIL"
        req_id += 1

    # Test 2: find_symbol
    if "find_symbol" in found:
        print(f"\n--- Test: find_symbol ---")
        rpc_call(
            session_url,
            "tools/call",
            {"name": "find_symbol", "arguments": {"name": "main"}},
            req_id=req_id,
        )
        resp = wait_response(req_id, timeout=15)
        if resp and "result" in resp:
            text = resp["result"].get("content", [{}])[0].get("text", "")
            is_error = resp["result"].get("isError", False)
            status = "FAIL" if is_error else "PASS"
            print(f"  [{status}] ({len(text)} chars) {text[:150]}")
            test_results["find_symbol"] = status
        else:
            print(f"  [FAIL] No response")
            test_results["find_symbol"] = "FAIL"
        req_id += 1

    # Test 3: search_for_pattern
    if "search_for_pattern" in found:
        print(f"\n--- Test: search_for_pattern ---")
        rpc_call(
            session_url,
            "tools/call",
            {
                "name": "search_for_pattern",
                "arguments": {"pattern": "class", "max_results": 3},
            },
            req_id=req_id,
        )
        resp = wait_response(req_id, timeout=15)
        if resp and "result" in resp:
            text = resp["result"].get("content", [{}])[0].get("text", "")
            is_error = resp["result"].get("isError", False)
            status = "FAIL" if is_error else "PASS"
            print(f"  [{status}] ({len(text)} chars) {text[:150]}")
            test_results["search_for_pattern"] = status
        else:
            print(f"  [FAIL] No response")
            test_results["search_for_pattern"] = "FAIL"
        req_id += 1

    # Summary
    print(f"\n{'='*50}")
    print(f"=== VERIFICATION SUMMARY ===")
    print(f"{'='*50}")
    for tool, status in test_results.items():
        icon = "✅" if status == "PASS" else "❌"
        print(f"  {icon} {tool}: {status}")

    passed = sum(1 for s in test_results.values() if s == "PASS")
    total = len(test_results)
    print(f"\n  Result: {passed}/{total} passed")

    sse_sock.close()


if __name__ == "__main__":
    main()
