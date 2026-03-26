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


def request_json(method, host, port, path, payload=None, timeout=10):
    body = None if payload is None else json.dumps(payload).encode("utf-8")
    conn = http.client.HTTPConnection(host, port, timeout=timeout)
    headers = {"Accept": "application/json"}
    if body is not None:
        headers["Content-Type"] = "application/json"
    conn.request(method, path, body=body, headers=headers)
    response = conn.getresponse()
    raw = response.read()
    conn.close()
    if response.status >= 400:
        raise RuntimeError(
            f"{method} {path} failed: {response.status} {response.reason} {raw.decode('utf-8', 'ignore')}"
        )
    return json.loads(raw.decode("utf-8"))


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

        if (
            event_type == "message"
            and isinstance(payload, dict)
            and payload.get("id") == req_id
        ):
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
    print(
        f"  Server: {server_info.get('name', '?')} v{server_info.get('version', '?')}"
    )

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
        "activate_project",
        "get_current_config",
        "get_project_modules",
        "get_open_files",
        "get_file_problems",
        "check_onboarding_performed",
        "initial_instructions",
        "list_memories",
        "read_memory",
        "write_memory",
        "delete_memory",
        "edit_memory",
        "rename_memory",
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
        "get_run_configurations",
        "execute_run_configuration",
        "reformat_file",
        "execute_terminal_command",
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
            "activate_project",
            {"project": "codelens-mcp-plugin"},
            lambda data: data.get("activated") is True,
        ),
        (
            "get_current_config",
            {},
            lambda data: data.get("tool_count", 0) >= len(codelens_tools),
        ),
        (
            "get_project_modules",
            {},
            lambda data: data.get("count", 0) >= 1,
        ),
        (
            "check_onboarding_performed",
            {},
            lambda data: isinstance(data.get("required_memories"), list),
        ),
        (
            "initial_instructions",
            {},
            lambda data: isinstance(data.get("instructions"), list)
            and len(data["instructions"]) >= 1,
        ),
        (
            "list_memories",
            {},
            lambda data: isinstance(data.get("memories"), list),
        ),
        (
            "read_memory",
            {"memory_name": "project_overview"},
            lambda data: "CodeLens MCP" in data.get("content", ""),
        ),
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
            print(
                f"  FAIL: unexpected payload {json.dumps(parsed_payload, ensure_ascii=False)[:180]}"
            )
            results.append((tool_name, "FAIL"))
        req_id += 1

    print("\n=== Summary ===")
    passed = 0
    for tool_name, status_name in results:
        print(f"  {status_name}: {tool_name}")
        if status_name == "PASS":
            passed += 1
    print(f"\nResult: {passed}/{len(results)} passed")

    print("\n=== Step 4: Serena Compatibility ===")
    compat_results = []
    try:
        status_payload = request_json("GET", "127.0.0.1", 24226, "/status")
        status_ok = status_payload.get("projectRoot", "").endswith(
            "/codelens-mcp-plugin"
        ) and isinstance(status_payload.get("pluginVersion"), str)
        compat_results.append(("status", status_ok, status_payload))
    except Exception as exc:
        compat_results.append(("status", False, str(exc)))

    try:
        find_symbol_payload = request_json(
            "POST",
            "127.0.0.1",
            24226,
            "/findSymbol",
            {
                "namePath": "CodeLensStartupActivity",
                "relativePath": "src/main/kotlin/com/codelens/plugin/CodeLensStartupActivity.kt",
                "includeLocation": True,
            },
        )
        symbols = find_symbol_payload.get("symbols", [])
        symbol = symbols[0] if symbols else {}
        symbol_ok = (
            isinstance(symbols, list)
            and len(symbols) >= 1
            and symbol.get("namePath") == "CodeLensStartupActivity"
            and symbol.get("relativePath")
            == "src/main/kotlin/com/codelens/plugin/CodeLensStartupActivity.kt"
            and "textRange" in symbol
        )
        compat_results.append(("findSymbol", symbol_ok, find_symbol_payload))
    except Exception as exc:
        compat_results.append(("findSymbol", False, str(exc)))

    try:
        references_payload = request_json(
            "POST",
            "127.0.0.1",
            24226,
            "/findReferences",
            {
                "namePath": "JsonBuilder",
                "relativePath": "src/main/kotlin/com/codelens/util/JsonBuilder.kt",
                "includeQuickInfo": True,
            },
        )
        references = references_payload.get("symbols", [])
        references_ok = (
            isinstance(references, list)
            and len(references) >= 1
            and any(
                symbol.get("relativePath")
                != "src/main/kotlin/com/codelens/util/JsonBuilder.kt"
                for symbol in references
            )
        )
        compat_results.append(("findReferences", references_ok, references_payload))
    except Exception as exc:
        compat_results.append(("findReferences", False, str(exc)))

    try:
        overview_payload = request_json(
            "POST",
            "127.0.0.1",
            24226,
            "/getSymbolsOverview",
            {
                "relativePath": "src/main/kotlin/com/codelens/plugin/CodeLensStartupActivity.kt",
                "depth": 1,
                "includeFileDocumentation": False,
            },
        )
        overview_symbols = overview_payload.get("symbols", [])
        overview_symbol = overview_symbols[0] if overview_symbols else {}
        overview_ok = (
            isinstance(overview_symbols, list)
            and len(overview_symbols) >= 1
            and overview_symbol.get("namePath") == "CodeLensStartupActivity"
            and isinstance(overview_symbol.get("children"), list)
        )
        compat_results.append(("getSymbolsOverview", overview_ok, overview_payload))
    except Exception as exc:
        compat_results.append(("getSymbolsOverview", False, str(exc)))

    try:
        supertypes_payload = request_json(
            "POST",
            "127.0.0.1",
            24226,
            "/getSupertypes",
            {
                "namePath": "CodeLensStartupActivity",
                "relativePath": "src/main/kotlin/com/codelens/plugin/CodeLensStartupActivity.kt",
                "depth": 1,
                "limitChildren": 2,
            },
        )
        hierarchy = supertypes_payload.get("hierarchy", [])
        hierarchy_ok = (
            isinstance(hierarchy, list)
            and len(hierarchy) >= 1
            and hierarchy[0].get("symbol", {}).get("namePath")
        )
        compat_results.append(("getSupertypes", hierarchy_ok, supertypes_payload))
    except Exception as exc:
        compat_results.append(("getSupertypes", False, str(exc)))

    try:
        subtypes_payload = request_json(
            "POST",
            "127.0.0.1",
            24226,
            "/getSubtypes",
            {
                "namePath": "BaseMcpTool",
                "relativePath": "src/main/kotlin/com/codelens/tools/BaseMcpTool.kt",
                "depth": 1,
                "limitChildren": 3,
            },
        )
        sub_hierarchy = subtypes_payload.get("hierarchy", [])
        subtypes_ok = isinstance(sub_hierarchy, list) and len(sub_hierarchy) >= 1
        compat_results.append(("getSubtypes", subtypes_ok, subtypes_payload))
    except Exception as exc:
        compat_results.append(("getSubtypes", False, str(exc)))

    try:
        read_payload = request_json(
            "POST",
            "127.0.0.1",
            24226,
            "/readFile",
            {
                "relativePath": "build.gradle.kts",
                "startLine": 1,
                "endLine": 5,
            },
        )
        read_ok = (
            "content" in read_payload
            and read_payload.get("totalLines", 0) > 0
            and read_payload.get("startLine") == 1
        )
        compat_results.append(("readFile", read_ok, read_payload))
    except Exception as exc:
        compat_results.append(("readFile", False, str(exc)))

    try:
        listdir_payload = request_json(
            "POST",
            "127.0.0.1",
            24226,
            "/listDir",
            {"relativePath": "src/main/kotlin/com/codelens/serena"},
        )
        entries = listdir_payload.get("entries", [])
        listdir_ok = isinstance(entries, list) and len(entries) >= 2
        compat_results.append(("listDir", listdir_ok, listdir_payload))
    except Exception as exc:
        compat_results.append(("listDir", False, str(exc)))

    try:
        findfile_payload = request_json(
            "POST",
            "127.0.0.1",
            24226,
            "/findFile",
            {"fileMask": "*.kt", "relativePath": "src/main/kotlin/com/codelens/serena"},
        )
        files = findfile_payload.get("files", [])
        findfile_ok = isinstance(files, list) and len(files) >= 2
        compat_results.append(("findFile", findfile_ok, findfile_payload))
    except Exception as exc:
        compat_results.append(("findFile", False, str(exc)))

    try:
        search_payload = request_json(
            "POST",
            "127.0.0.1",
            24226,
            "/searchForPattern",
            {
                "pattern": "class SerenaCompat",
                "relativePath": "src/main/kotlin/com/codelens/serena",
                "contextLinesBefore": 1,
                "contextLinesAfter": 1,
            },
        )
        matches = search_payload.get("matches", {})
        search_ok = isinstance(matches, dict) and len(matches) >= 1
        compat_results.append(("searchForPattern", search_ok, search_payload))
    except Exception as exc:
        compat_results.append(("searchForPattern", False, str(exc)))

    try:
        snippets_payload = request_json(
            "POST",
            "127.0.0.1",
            24226,
            "/findReferencingCodeSnippets",
            {
                "namePath": "JsonBuilder",
                "relativePath": "src/main/kotlin/com/codelens/util/JsonBuilder.kt",
                "contextLinesBefore": 2,
                "contextLinesAfter": 2,
            },
        )
        snippets = snippets_payload.get("snippets", [])
        snippets_ok = (
            isinstance(snippets, list)
            and len(snippets) >= 1
            and "snippet" in snippets[0]
            and "line" in snippets[0]
        )
        compat_results.append(
            ("findReferencingCodeSnippets", snippets_ok, snippets_payload)
        )
    except Exception as exc:
        compat_results.append(("findReferencingCodeSnippets", False, str(exc)))

    try:
        runconfigs_payload = request_json(
            "POST",
            "127.0.0.1",
            24226,
            "/getRunConfigurations",
            {},
        )
        configs = runconfigs_payload.get("configurations", [])
        runconfigs_ok = isinstance(configs, list)
        compat_results.append(
            ("getRunConfigurations", runconfigs_ok, runconfigs_payload)
        )
    except Exception as exc:
        compat_results.append(("getRunConfigurations", False, str(exc)))

    try:
        reformat_payload = request_json(
            "POST",
            "127.0.0.1",
            24226,
            "/reformatFile",
            {"relativePath": "src/main/kotlin/com/codelens/tools/BaseMcpTool.kt"},
        )
        reformat_ok = reformat_payload.get("status") == "ok"
        compat_results.append(("reformatFile", reformat_ok, reformat_payload))
    except Exception as exc:
        compat_results.append(("reformatFile", False, str(exc)))

    try:
        terminal_payload = request_json(
            "POST",
            "127.0.0.1",
            24226,
            "/executeTerminalCommand",
            {"command": "echo hello_codelens", "timeout": 5000},
        )
        terminal_ok = terminal_payload.get(
            "exitCode"
        ) == 0 and "hello_codelens" in terminal_payload.get("output", "")
        compat_results.append(("executeTerminalCommand", terminal_ok, terminal_payload))
    except Exception as exc:
        compat_results.append(("executeTerminalCommand", False, str(exc)))

    compat_passed = 0
    for name, ok, payload in compat_results:
        if ok:
            compat_passed += 1
            print(f"  PASS: {name} -> {json.dumps(payload, ensure_ascii=False)[:180]}")
        else:
            print(f"  FAIL: {name} -> {payload}")

    print(f"\nCompatibility: {compat_passed}/{len(compat_results)} passed")

    sse_conn.close()
    if passed != len(results) or compat_passed != len(compat_results):
        sys.exit(1)


if __name__ == "__main__":
    main()
