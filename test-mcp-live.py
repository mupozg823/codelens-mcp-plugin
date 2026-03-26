#!/usr/bin/env python3
"""Live test of CodeLens MCP tools via SSE"""
import json, requests, sseclient, threading, time, sys

BASE = "http://127.0.0.1:64342"
responses = {}

def listen_sse(url, stop_event):
    resp = requests.get(url, stream=True, headers={"Accept": "text/event-stream"})
    client = sseclient.SSEClient(resp)
    for event in client.events():
        if stop_event.is_set():
            break
        if event.event == "endpoint":
            responses["endpoint"] = event.data
        elif event.event == "message":
            try:
                msg = json.loads(event.data)
                msg_id = msg.get("id")
                if msg_id:
                    responses[msg_id] = msg
            except:
                pass

def post(session_id, payload):
    url = f"{BASE}/message?sessionId={session_id}"
    requests.post(url, json=payload, headers={"Content-Type": "application/json"})

def wait_for(msg_id, timeout=10):
    start = time.time()
    while time.time() - start < timeout:
        if msg_id in responses:
            return responses[msg_id]
        time.sleep(0.1)
    return None

stop = threading.Event()
t = threading.Thread(target=listen_sse, args=(f"{BASE}/sse", stop), daemon=True)
t.start()
time.sleep(1)

# Get session
endpoint = responses.get("endpoint", "")
session_id = endpoint.split("sessionId=")[-1] if "sessionId=" in endpoint else ""
if not session_id:
    print("FAIL: No session ID"); sys.exit(1)
print(f"Session: {session_id}")

# Initialize
post(session_id, {"jsonrpc":"2.0","id":1,"method":"initialize","params":{
    "protocolVersion":"2024-11-05","capabilities":{},
    "clientInfo":{"name":"codelens-test","version":"1.0"}}})
r = wait_for(1)
print(f"\n=== INITIALIZE ===\n{json.dumps(r, indent=2, ensure_ascii=False)[:500]}")

post(session_id, {"jsonrpc":"2.0","method":"notifications/initialized"})
time.sleep(0.5)

# List tools
post(session_id, {"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}})
r = wait_for(2)
if r and "result" in r:
    tools = r["result"].get("tools", [])
    print(f"\n=== TOOLS ({len(tools)}) ===")
    for tool in tools:
        print(f"  - {tool['name']}")
    
    # Find CodeLens tools
    codelens = [t for t in tools if t["name"] in [
        "get_symbols_overview","find_symbol","find_referencing_symbols",
        "search_for_pattern","replace_symbol_body","insert_after_symbol",
        "insert_before_symbol","rename_symbol"]]
    print(f"\n=== CODELENS TOOLS ({len(codelens)}) ===")
    for t in codelens:
        print(f"  - {t['name']}: {t.get('description','')[:80]}")

# Test 1: get_symbols_overview on build.gradle.kts
print("\n=== TEST: get_symbols_overview ===")
post(session_id, {"jsonrpc":"2.0","id":10,"method":"tools/call","params":{
    "name":"get_symbols_overview",
    "arguments":{"path":"src/main/kotlin/com/codelens/tools/BaseMcpTool.kt"}}})
r = wait_for(10, timeout=15)
if r:
    content = r.get("result",{}).get("content",[])
    for c in content:
        print(c.get("text","")[:1000])

# Test 2: find_symbol
print("\n=== TEST: find_symbol ===")
post(session_id, {"jsonrpc":"2.0","id":11,"method":"tools/call","params":{
    "name":"find_symbol",
    "arguments":{"name":"BaseMcpTool","include_body":False}}})
r = wait_for(11, timeout=15)
if r:
    content = r.get("result",{}).get("content",[])
    for c in content:
        print(c.get("text","")[:1000])

# Test 3: search_for_pattern
print("\n=== TEST: search_for_pattern ===")
post(session_id, {"jsonrpc":"2.0","id":12,"method":"tools/call","params":{
    "name":"search_for_pattern",
    "arguments":{"pattern":"McpToolsProvider","path":"src/"}}})
r = wait_for(12, timeout=15)
if r:
    content = r.get("result",{}).get("content",[])
    for c in content:
        print(c.get("text","")[:1000])

stop.set()
print("\n=== DONE ===")
