use super::*;

// ── LSP tool tests ───────────────────────────────────────────────────

#[test]
fn returns_lsp_references_via_tool_call() {
    let project = project_root();
    fs::write(
        project.as_path().join("ref_target.py"),
        "class MyClass:\n    pass\n\nobj = MyClass()\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({ "file_path": "ref_target.py", "symbol_name": "MyClass" }),
    );
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_lsp_diagnostics_via_tool_call() {
    let project = project_root();
    let mock_lsp = concat!(
        "#!/usr/bin/env python3\n",
        "import sys, json\n",
        "def read_msg():\n",
        "    h = ''\n",
        "    while True:\n",
        "        c = sys.stdin.buffer.read(1)\n",
        "        if not c: return None\n",
        "        h += c.decode('ascii')\n",
        "        if h.endswith('\\r\\n\\r\\n'): break\n",
        "    length = int([l for l in h.split('\\r\\n') if l.startswith('Content-Length:')][0].split(': ')[1])\n",
        "    return json.loads(sys.stdin.buffer.read(length).decode('utf-8'))\n",
        "def send(r):\n",
        "    out = json.dumps(r)\n",
        "    b = out.encode('utf-8')\n",
        "    sys.stdout.buffer.write(f'Content-Length: {len(b)}\\r\\n\\r\\n'.encode('ascii'))\n",
        "    sys.stdout.buffer.write(b)\n",
        "    sys.stdout.buffer.flush()\n",
        "while True:\n",
        "    msg = read_msg()\n",
        "    if msg is None: break\n",
        "    rid = msg.get('id')\n",
        "    m = msg.get('method', '')\n",
        "    if m == 'initialized': continue\n",
        "    if rid is None: continue\n",
        "    if m == 'initialize':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'textDocumentSync':1,'diagnosticProvider':{}}}})\n",
        "    elif m == 'textDocument/diagnostic':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'kind':'full','items':[{'range':{'start':{'line':0,'character':0},'end':{'line':0,'character':5}},'severity':2,'message':'test warning'}]}})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join("mock_lsp.py");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(project.as_path().join("diag_target.py"), "x = 1\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "get_file_diagnostics",
        json!({ "file_path": "diag_target.py", "command": "python3", "args": [mock_path.to_string_lossy()] }),
    );
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_workspace_symbols_via_tool_call() {
    let project = project_root();
    let mock_lsp = concat!(
        "#!/usr/bin/env python3\n",
        "import sys, json\n",
        "def read_msg():\n",
        "    h = ''\n",
        "    while True:\n",
        "        c = sys.stdin.buffer.read(1)\n",
        "        if not c: return None\n",
        "        h += c.decode('ascii')\n",
        "        if h.endswith('\\r\\n\\r\\n'): break\n",
        "    length = int([l for l in h.split('\\r\\n') if l.startswith('Content-Length:')][0].split(': ')[1])\n",
        "    return json.loads(sys.stdin.buffer.read(length).decode('utf-8'))\n",
        "def send(r):\n",
        "    out = json.dumps(r)\n",
        "    b = out.encode('utf-8')\n",
        "    sys.stdout.buffer.write(f'Content-Length: {len(b)}\\r\\n\\r\\n'.encode('ascii'))\n",
        "    sys.stdout.buffer.write(b)\n",
        "    sys.stdout.buffer.flush()\n",
        "while True:\n",
        "    msg = read_msg()\n",
        "    if msg is None: break\n",
        "    rid = msg.get('id')\n",
        "    m = msg.get('method', '')\n",
        "    if m == 'initialized': continue\n",
        "    if rid is None: continue\n",
        "    if m == 'initialize':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'workspaceSymbolProvider':True}}})\n",
        "    elif m == 'workspace/symbol':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':[{'name':'TestSymbol','kind':5,'location':{'uri':'file:///test.py','range':{'start':{'line':0,'character':0},'end':{'line':0,'character':10}}}}]})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join("mock_ws_lsp.py");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "search_workspace_symbols",
        json!({ "query": "Test", "command": "python3", "args": [mock_path.to_string_lossy()] }),
    );
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_type_hierarchy_via_tool_call() {
    let project = project_root();
    fs::write(
        project.as_path().join("hierarchy.py"),
        "class Animal:\n    pass\nclass Dog(Animal):\n    pass\nclass Cat(Animal):\n    pass\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "get_type_hierarchy",
        json!({ "name_path": "Animal", "relative_path": "hierarchy.py" }),
    );
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_rename_plan_via_tool_call() {
    let project = project_root();
    fs::write(
        project.as_path().join("rename_target.py"),
        "def old_name():\n    pass\n\nold_name()\n",
    )
    .unwrap();
    let mock_lsp = concat!(
        "#!/usr/bin/env python3\n",
        "import sys, json\n",
        "def read_msg():\n",
        "    h = ''\n",
        "    while True:\n",
        "        c = sys.stdin.buffer.read(1)\n",
        "        if not c: return None\n",
        "        h += c.decode('ascii')\n",
        "        if h.endswith('\\r\\n\\r\\n'): break\n",
        "    length = int([l for l in h.split('\\r\\n') if l.startswith('Content-Length:')][0].split(': ')[1])\n",
        "    return json.loads(sys.stdin.buffer.read(length).decode('utf-8'))\n",
        "def send(r):\n",
        "    out = json.dumps(r)\n",
        "    b = out.encode('utf-8')\n",
        "    sys.stdout.buffer.write(f'Content-Length: {len(b)}\\r\\n\\r\\n'.encode('ascii'))\n",
        "    sys.stdout.buffer.write(b)\n",
        "    sys.stdout.buffer.flush()\n",
        "while True:\n",
        "    msg = read_msg()\n",
        "    if msg is None: break\n",
        "    rid = msg.get('id')\n",
        "    m = msg.get('method', '')\n",
        "    if m == 'initialized': continue\n",
        "    if rid is None: continue\n",
        "    if m == 'initialize':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'renameProvider':{'prepareProvider':True}}}})\n",
        "    elif m == 'textDocument/prepareRename':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'range':{'start':{'line':0,'character':4},'end':{'line':0,'character':12}},'placeholder':'old_name'}})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join("mock_rename_lsp.py");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "plan_symbol_rename",
        json!({ "file_path": "rename_target.py", "line": 1, "column": 5, "new_name": "new_name", "command": "python3", "args": [mock_path.to_string_lossy()] }),
    );
    assert_eq!(payload["success"], json!(true));
}

// ── P0-2: tree-sitter + LSP union regression tests ─────────────────

/// Build a minimal mock LSP server script that returns `lsp_refs` as the
/// `textDocument/references` response. Writes it to `project` and returns
/// an executable path. Linux/macOS only (CI runs on both; Windows is
/// gated elsewhere).
fn write_mock_refs_lsp(
    project: &ProjectRoot,
    name: &str,
    lsp_refs_json: &str,
) -> std::path::PathBuf {
    let script = format!(
        concat!(
            "#!/usr/bin/env python3\n",
            "import sys, json\n",
            "def read_msg():\n",
            "    h = ''\n",
            "    while True:\n",
            "        c = sys.stdin.buffer.read(1)\n",
            "        if not c: return None\n",
            "        h += c.decode('ascii')\n",
            "        if h.endswith('\\r\\n\\r\\n'): break\n",
            "    length = int([l for l in h.split('\\r\\n') if l.startswith('Content-Length:')][0].split(': ')[1])\n",
            "    return json.loads(sys.stdin.buffer.read(length).decode('utf-8'))\n",
            "def send(r):\n",
            "    out = json.dumps(r)\n",
            "    b = out.encode('utf-8')\n",
            "    sys.stdout.buffer.write(f'Content-Length: {{len(b)}}\\r\\n\\r\\n'.encode('ascii'))\n",
            "    sys.stdout.buffer.write(b)\n",
            "    sys.stdout.buffer.flush()\n",
            "while True:\n",
            "    msg = read_msg()\n",
            "    if msg is None: break\n",
            "    rid = msg.get('id')\n",
            "    m = msg.get('method', '')\n",
            "    if m == 'initialized': continue\n",
            "    if rid is None: continue\n",
            "    if m == 'initialize':\n",
            "        send({{'jsonrpc':'2.0','id':rid,'result':{{'capabilities':{{'referencesProvider':True}}}}}})\n",
            "    elif m == 'textDocument/references':\n",
            "        send({{'jsonrpc':'2.0','id':rid,'result':{lsp_refs}}})\n",
            "    elif m == 'shutdown':\n",
            "        send({{'jsonrpc':'2.0','id':rid,'result':None}})\n",
            "    else:\n",
            "        send({{'jsonrpc':'2.0','id':rid,'result':None}})\n",
        ),
        lsp_refs = lsp_refs_json
    );
    let mock_path = project.as_path().join(name);
    fs::write(&mock_path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    mock_path
}

#[test]
fn union_flag_preserves_lsp_only_envelope_when_false() {
    // Contract: union=false (default) keeps the pre-P0-2 LSP-only
    // response shape — `backend="lsp"`, no `sources` payload.
    let project = project_root();
    fs::write(
        project.as_path().join("union_target.py"),
        "class Widget:\n    pass\n\nw = Widget()\n",
    )
    .unwrap();
    // LSP returns one reference (the instantiation on line 3 / 0-based 2).
    let lsp_refs = r#"[{"uri":"file://__PROJECT__/union_target.py","range":{"start":{"line":2,"character":4},"end":{"line":2,"character":10}}}]"#
        .replace("__PROJECT__", project.as_path().to_string_lossy().as_ref());
    let mock = write_mock_refs_lsp(&project, "mock_union_lsp.py", &lsp_refs);
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({
            "file_path": "union_target.py",
            "symbol_name": "Widget",
            "line": 1,
            "column": 6,
            "use_lsp": true,
            "command": "python3",
            "args": [mock.to_string_lossy()],
        }),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["backend"], json!("lsp"));
    assert!(
        payload["data"].get("sources").is_none(),
        "union=false path must not emit the `sources` union breakdown"
    );
}

#[test]
fn union_flag_merges_lsp_and_tree_sitter_refs_when_true() {
    // Contract: union=true adds tree-sitter hits that the LSP path
    // missed, tagged with `source: "tree_sitter"`; LSP-origin hits stay
    // tagged `source: "lsp"`. Backend flips to "union" and `sources`
    // reports the breakdown so downstream telemetry can observe the lift.
    let project = project_root();
    fs::write(
        project.as_path().join("union_target.py"),
        concat!(
            "class Widget:\n", // line 1: declaration
            "    pass\n",      // line 2
            "\n",              // line 3
            "a = Widget()\n",  // line 4: tree-sitter-visible hit
            "b = Widget\n",    // line 5: tree-sitter-visible hit
        ),
    )
    .unwrap();
    // Mock LSP returns a single reference — the instantiation on line 4
    // (0-based line 3). tree-sitter will additionally find line 5, which
    // the LSP mock does not report. The union must surface both.
    let lsp_refs = r#"[{"uri":"file://__PROJECT__/union_target.py","range":{"start":{"line":3,"character":4},"end":{"line":3,"character":10}}}]"#
        .replace("__PROJECT__", project.as_path().to_string_lossy().as_ref());
    let mock = write_mock_refs_lsp(&project, "mock_union_lsp.py", &lsp_refs);
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({
            "file_path": "union_target.py",
            "symbol_name": "Widget",
            "line": 1,
            "column": 6,
            "use_lsp": true,
            "union": true,
            "command": "python3",
            "args": [mock.to_string_lossy()],
        }),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["backend"], json!("union"));
    let sources = &payload["data"]["sources"];
    assert_eq!(sources["lsp"], json!(1), "LSP should contribute 1 ref");
    assert!(
        sources["tree_sitter_added"].as_u64().unwrap_or(0) >= 1,
        "tree-sitter must contribute at least 1 additional ref (line 5 decl-less usage); got {}",
        sources
    );
    let merged = sources["merged"].as_u64().unwrap_or(0);
    assert!(merged >= 2, "merged ref count must be ≥ 2, got {merged}");
    // Every ref carries its provenance tag.
    let refs = payload["data"]["references"]
        .as_array()
        .expect("references array");
    assert!(
        refs.iter().any(|r| r["source"] == "lsp"),
        "at least one ref must be tagged source=lsp"
    );
    assert!(
        refs.iter().any(|r| r["source"] == "tree_sitter"),
        "at least one ref must be tagged source=tree_sitter"
    );
}

// ── P1-1: session-level lock parallelism ────────────────────────────

/// Mock LSP that intentionally sleeps `sleep_ms` before answering the
/// `textDocument/references` request. Used to observe whether the pool
/// serializes distinct (command, args) sessions. Writes executable
/// Python script to `project/{name}` and returns its path.
fn write_sleeping_mock_refs_lsp(
    project: &ProjectRoot,
    name: &str,
    sleep_ms: u64,
) -> std::path::PathBuf {
    let script = format!(
        concat!(
            "#!/usr/bin/env python3\n",
            "import sys, json, time\n",
            "def read_msg():\n",
            "    h = ''\n",
            "    while True:\n",
            "        c = sys.stdin.buffer.read(1)\n",
            "        if not c: return None\n",
            "        h += c.decode('ascii')\n",
            "        if h.endswith('\\r\\n\\r\\n'): break\n",
            "    length = int([l for l in h.split('\\r\\n') if l.startswith('Content-Length:')][0].split(': ')[1])\n",
            "    return json.loads(sys.stdin.buffer.read(length).decode('utf-8'))\n",
            "def send(r):\n",
            "    out = json.dumps(r)\n",
            "    b = out.encode('utf-8')\n",
            "    sys.stdout.buffer.write(f'Content-Length: {{len(b)}}\\r\\n\\r\\n'.encode('ascii'))\n",
            "    sys.stdout.buffer.write(b)\n",
            "    sys.stdout.buffer.flush()\n",
            "while True:\n",
            "    msg = read_msg()\n",
            "    if msg is None: break\n",
            "    rid = msg.get('id')\n",
            "    m = msg.get('method', '')\n",
            "    if m == 'initialized': continue\n",
            "    if rid is None: continue\n",
            "    if m == 'initialize':\n",
            "        send({{'jsonrpc':'2.0','id':rid,'result':{{'capabilities':{{'referencesProvider':True}}}}}})\n",
            "    elif m == 'textDocument/references':\n",
            "        time.sleep({sleep_secs})\n",
            "        send({{'jsonrpc':'2.0','id':rid,'result':[]}})\n",
            "    elif m == 'shutdown':\n",
            "        send({{'jsonrpc':'2.0','id':rid,'result':None}})\n",
            "    else:\n",
            "        send({{'jsonrpc':'2.0','id':rid,'result':None}})\n",
        ),
        sleep_secs = sleep_ms as f64 / 1000.0
    );
    let path = project.as_path().join(name);
    fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

/// Before P1-1: a single `Mutex<HashMap<..>>` serialized every pool
/// request, so calls targeting _different_ LSP sessions (e.g. pyright
/// vs typescript-language-server in a polyglot monorepo) ran
/// back-to-back. After P1-1 the pool is a `DashMap<K, Arc<Mutex<V>>>`
/// and only the targeted session's mutex blocks — two distinct sessions
/// execute in parallel.
///
/// Shape of the test: launch two `find_referencing_symbols` calls in
/// parallel, each against a mock LSP whose `textDocument/references`
/// handler sleeps ~200 ms. If the pool serializes, wall-clock is
/// roughly 2× sleep (~400 ms+). If the pool parallelizes, wall-clock is
/// roughly 1× sleep + overhead (~200 ms + spawn cost). We assert the
/// wall-clock is strictly closer to 1× than to 2×.
#[test]
fn lsp_pool_runs_distinct_sessions_in_parallel() {
    use std::sync::Arc;
    use std::thread;

    let project = project_root();
    fs::write(
        project.as_path().join("t.py"),
        "class X:\n    pass\nx = X()\n",
    )
    .unwrap();

    let sleep_ms: u64 = 200;
    let mock_a = write_sleeping_mock_refs_lsp(&project, "mock_a.py", sleep_ms);
    let mock_b = write_sleeping_mock_refs_lsp(&project, "mock_b.py", sleep_ms);

    let state = Arc::new(make_state(&project));

    // Warm up each session once so the two timed calls only measure the
    // sleeping `references` handler, not the `initialize` handshake.
    let _ = call_tool(
        state.as_ref(),
        "find_referencing_symbols",
        json!({
            "file_path": "t.py",
            "symbol_name": "X",
            "line": 1, "column": 6,
            "use_lsp": true,
            "command": "python3",
            "args": [mock_a.to_string_lossy()],
        }),
    );
    let _ = call_tool(
        state.as_ref(),
        "find_referencing_symbols",
        json!({
            "file_path": "t.py",
            "symbol_name": "X",
            "line": 1, "column": 6,
            "use_lsp": true,
            "command": "python3",
            "args": [mock_b.to_string_lossy()],
        }),
    );

    // Now issue the two calls in parallel against the already-warm
    // sessions. Each sleeps `sleep_ms` server-side.
    let state_a = state.clone();
    let args_a = mock_a.to_string_lossy().to_string();
    let state_b = state.clone();
    let args_b = mock_b.to_string_lossy().to_string();

    let start = std::time::Instant::now();
    let h_a = thread::spawn(move || {
        call_tool(
            state_a.as_ref(),
            "find_referencing_symbols",
            json!({
                "file_path": "t.py",
                "symbol_name": "X",
                "line": 1, "column": 6,
                "use_lsp": true,
                "command": "python3",
                "args": [args_a],
            }),
        )
    });
    let h_b = thread::spawn(move || {
        call_tool(
            state_b.as_ref(),
            "find_referencing_symbols",
            json!({
                "file_path": "t.py",
                "symbol_name": "X",
                "line": 1, "column": 6,
                "use_lsp": true,
                "command": "python3",
                "args": [args_b],
            }),
        )
    });
    let payload_a = h_a.join().unwrap();
    let payload_b = h_b.join().unwrap();
    let elapsed_ms = start.elapsed().as_millis() as u64;

    assert_eq!(payload_a["success"], json!(true));
    assert_eq!(payload_b["success"], json!(true));

    // Serialized path would be ≥ 2 * sleep_ms (~400 ms here). We pick a
    // generous ceiling at 1.75 * sleep_ms to absorb CI variance but
    // still fail the test if the pool silently re-serializes.
    let serialized_floor = 2 * sleep_ms;
    let ceiling = (sleep_ms * 175) / 100;
    assert!(
        elapsed_ms < ceiling,
        "two parallel LSP calls took {elapsed_ms}ms \
         (sleep per call {sleep_ms}ms; serialized floor would be ≥ {serialized_floor}ms). \
         The pool appears to serialize distinct sessions — P1-1 regressed."
    );
}
