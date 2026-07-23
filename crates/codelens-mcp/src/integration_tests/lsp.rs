use super::*;

// ── LSP tool tests ───────────────────────────────────────────────────

fn write_mock_diagnostics_lsp(project: &ProjectRoot, name: &str) -> std::path::PathBuf {
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
    let mock_path = project.as_path().join(name);
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    mock_path
}

fn write_mock_pyright_diagnostics_lsp(project: &ProjectRoot, name: &str) -> std::path::PathBuf {
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
        "        uri = msg['params']['textDocument']['uri']\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'kind':'full','uri':uri,'items':[\n",
        "          {'range':{'start':{'line':4,'character':8},'end':{'line':4,'character':15}},'severity':1,'source':'pyright','code':'reportMissingImports','message':'Import \"PySide6\" could not be resolved'},\n",
        "          {'range':{'start':{'line':8,'character':17},'end':{'line':8,'character':31}},'severity':1,'source':'pyright','code':'reportCallIssue','message':'No parameter named \"ffmpeg_threads\"'}\n",
        "        ]}})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join(name);
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    mock_path
}

fn register_mock_lsp(state: &crate::AppState, executable: &std::path::Path) {
    state
        .lsp_pool()
        .register_trusted_lsp_binary("pyright-langserver", executable)
        .expect("register mock LSP executable");
}

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
    assert_eq!(
        payload["data"]["evidence"]["schema_version"],
        json!("codelens-evidence-v1")
    );
    assert_eq!(payload["data"]["evidence"]["domain"], json!("references"));
    assert_eq!(
        payload["data"]["evidence"]["signals"]["precise_used"],
        json!(false)
    );
    assert_eq!(
        payload["data"]["evidence"]["signals"]["fallback_source"],
        json!("tree_sitter")
    );
}

#[test]
fn returns_lsp_diagnostics_via_tool_call() {
    let project = project_root();
    let mock_path = write_mock_diagnostics_lsp(&project, "mock_lsp.py");
    fs::write(project.as_path().join("diag_target.py"), "x = 1\n").unwrap();
    let state = make_state(&project);
    register_mock_lsp(&state, &mock_path);
    let payload = call_tool(
        &state,
        "get_file_diagnostics",
        json!({ "file_path": "diag_target.py", "command": "pyright-langserver", "args": ["--stdio"] }),
    );
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn get_file_diagnostics_rejects_unregistered_python_caller_without_spawning() {
    let project = project_root();
    fs::write(project.as_path().join("caller_guard.py"), "x = 1\n").unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "get_file_diagnostics",
        json!({
            "path": "caller_guard.py",
            "command": "python3",
            "args": ["-c", "raise SystemExit('caller-controlled')"]
        }),
    );

    assert_eq!(payload["success"], json!(false), "{payload}");
    assert_eq!(
        state.lsp_pool().session_count(),
        0,
        "rejected caller input must not spawn an LSP session: {payload}"
    );
}

#[test]
fn get_lsp_recipe_does_not_trust_project_local_typescript_shim() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).unwrap();
    fs::create_dir_all(project.as_path().join("node_modules/.bin")).unwrap();
    fs::write(
        project.as_path().join("src/App.tsx"),
        "export function App() { return null }\n",
    )
    .unwrap();
    let shim = project
        .as_path()
        .join("node_modules/.bin/typescript-language-server");
    fs::write(&shim, "#!/bin/sh\nexit 0\n").unwrap();

    let state = make_state(&project);
    let (raw_payload, _) =
        crate::tools::lsp::get_lsp_recipe(&state, &json!({ "path": "src/App.tsx" }))
            .expect("get_lsp_recipe handler succeeds");
    assert_eq!(
        raw_payload["execution_trust"],
        json!("daemon_environment_or_host_registration"),
        "trust metadata must be present on the raw recipe payload: {raw_payload:#}"
    );
    let payload = call_tool(&state, "get_lsp_recipe", json!({ "path": "src/App.tsx" }));

    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["extension"], json!("tsx"));
    assert_eq!(payload["data"]["language"], json!("typescript"));
    assert_eq!(
        payload["data"]["binary_name"],
        json!("typescript-language-server")
    );
    assert_ne!(
        payload["data"]["resolved_binary_path"],
        json!(shim.display().to_string()),
        "an unregistered project-local shim must not be trusted: {payload:#}"
    );
}

#[test]
fn get_file_diagnostics_accepts_legacy_file_path_with_deprecation_warning() {
    let project = project_root();
    let mock_path = write_mock_diagnostics_lsp(&project, "mock_legacy_diag_lsp.py");
    fs::write(project.as_path().join("legacy_diag.py"), "x = 1\n").unwrap();
    let state = make_state(&project);
    register_mock_lsp(&state, &mock_path);

    let payload = call_tool(
        &state,
        "get_file_diagnostics",
        json!({ "file_path": "legacy_diag.py", "command": "pyright-langserver", "args": ["--stdio"] }),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["deprecation_warnings"]
            .as_array()
            .expect("deprecation_warnings array")
            .len(),
        1
    );
    assert_eq!(
        payload["data"]["deprecation_warnings"][0]["param"],
        json!("file_path")
    );
}

#[test]
fn get_file_diagnostics_respects_pyright_source_suppressions() {
    let project = project_root();
    let mock_path = write_mock_pyright_diagnostics_lsp(&project, "mock_pyright_diag_lsp.py");
    fs::write(
        project.as_path().join("gui.py"),
        concat!(
            "# pyright: reportMissingImports=false\n",
            "\n",
            "def main():\n",
            "    try:\n",
            "        from PySide6.QtWidgets import QApplication\n",
            "    except ImportError:\n",
            "        return 2\n",
            "    return GuiRunConfig(\n",
            "        ffmpeg_threads=2,  # pyright: ignore[reportCallIssue]\n",
            "    )\n",
        ),
    )
    .unwrap();
    let state = make_state(&project);
    register_mock_lsp(&state, &mock_path);

    let payload = call_tool(
        &state,
        "get_file_diagnostics",
        json!({ "path": "gui.py", "command": "pyright-langserver", "args": ["--stdio"] }),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["count"], json!(0));
    assert_eq!(payload["data"]["suppressed_diagnostics_count"], json!(2));
    assert_eq!(
        payload["data"]["suppressed_diagnostics"][0]["suppression"],
        json!("file_pyright_rule_disabled")
    );
    assert_eq!(
        payload["data"]["suppressed_diagnostics"][1]["suppression"],
        json!("line_pyright_ignore")
    );
}

#[test]
fn get_file_diagnostics_classifies_guarded_optional_imports() {
    let project = project_root();
    let mock_path = write_mock_pyright_diagnostics_lsp(&project, "mock_optional_diag_lsp.py");
    fs::write(
        project.as_path().join("gui.py"),
        concat!(
            "# GUI optional dependency wrapper\n",
            "\n",
            "def main():\n",
            "    try:\n",
            "        from PySide6.QtWidgets import QApplication\n",
            "    except ImportError:\n",
            "        return 2\n",
            "    return GuiRunConfig(\n",
            "        ffmpeg_threads=2,\n",
            "    )\n",
        ),
    )
    .unwrap();
    let state = make_state(&project);
    register_mock_lsp(&state, &mock_path);

    let payload = call_tool(
        &state,
        "get_file_diagnostics",
        json!({ "path": "gui.py", "command": "pyright-langserver", "args": ["--stdio"] }),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["count"], json!(2));
    assert_eq!(
        payload["data"]["diagnostics"][0]["classification"],
        json!("optional_dependency_import")
    );
    assert_eq!(
        payload["data"]["diagnostics"][0]["actionability"],
        json!("environmental_optional_dependency")
    );
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
    register_mock_lsp(&state, &mock_path);
    let payload = call_tool(
        &state,
        "search_workspace_symbols",
        json!({ "query": "Test", "command": "pyright-langserver", "args": ["--stdio"] }),
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
    register_mock_lsp(&state, &mock_path);
    let payload = call_tool(
        &state,
        "plan_symbol_rename",
        json!({ "file_path": "rename_target.py", "line": 1, "column": 5, "new_name": "new_name", "command": "pyright-langserver", "args": ["--stdio"] }),
    );
    assert_eq!(payload["success"], json!(true));
}
#[test]
fn resolve_symbol_target_uses_lsp_definition_family() {
    let project = project_root();
    fs::write(
        project.as_path().join("target.rs"),
        "fn greet() {}\nfn main() { greet(); }\n",
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
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'definitionProvider':True,'implementationProvider':True,'typeDefinitionProvider':True}}})\n",
        "    elif m in ['textDocument/definition','textDocument/implementation','textDocument/typeDefinition']:\n",
        "        uri = msg['params']['textDocument']['uri']\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':[{'uri':uri,'range':{'start':{'line':0,'character':3},'end':{'line':0,'character':8}}}]})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join("mock_lsp_resolve.py");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let state = make_state(&project);
    register_mock_lsp(&state, &mock_path);
    let payload = call_tool(
        &state,
        "resolve_symbol_target",
        json!({
            "file_path": "target.rs",
            "line": 2,
            "column": 13,
            "target": "implementation",
            "semantic_backend": "lsp",
            "command": "pyright-langserver",
            "args": ["--stdio"]
        }),
    );

    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["semantic_backend"], json!("lsp"));
    assert_eq!(
        payload["data"]["edit_authority"]["operation"],
        json!("implementation")
    );
    assert_eq!(
        payload["data"]["targets"][0]["file_path"],
        json!("target.rs")
    );
}

#[test]
fn lsp_refactor_without_concrete_workspace_edit_fails_closed() {
    let project = project_root();
    fs::write(
        project.as_path().join("target.ts"),
        "const value = 1;\nconsole.log(value);\n",
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
        "    b = json.dumps(r).encode('utf-8')\n",
        "    sys.stdout.buffer.write(f'Content-Length: {len(b)}\\r\\n\\r\\n'.encode('ascii'))\n",
        "    sys.stdout.buffer.write(b)\n",
        "    sys.stdout.buffer.flush()\n",
        "while True:\n",
        "    msg = read_msg()\n",
        "    if msg is None: break\n",
        "    rid = msg.get('id')\n",
        "    method = msg.get('method', '')\n",
        "    if method == 'initialized': continue\n",
        "    if rid is None: continue\n",
        "    if method == 'initialize': send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'codeActionProvider': True}}})\n",
        "    elif method == 'textDocument/codeAction': send({'jsonrpc':'2.0','id':rid,'result':[]})\n",
        "    elif method == 'shutdown': send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else: send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join("mock_no_code_actions.py");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let state = make_state(&project);
    register_mock_lsp(&state, &mock_path);
    let payload = call_tool(
        &state,
        "refactor_extract_function",
        json!({
            "file_path": "target.ts",
            "start_line": 1,
            "end_line": 1,
            "new_name": "extracted",
            "semantic_edit_backend": "lsp",
            "command": "pyright-langserver",
            "args": ["--stdio"],
            "dry_run": true
        }),
    );

    assert_eq!(payload["success"], json!(false), "{payload}");
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or("")
            .contains("unsupported_semantic_refactor"),
        "{payload}"
    );
}

/// Issue #214 regression: when `find_referencing_symbols` runs on a
/// JS/TS file via the oxc_semantic backend, the response must surface
/// the cross-file limitation so callers do not mistake "no callers in
/// this file" for "no callers exist anywhere". The hint must point at
/// `get_callers` (import_graph backend) for the cross-file case.
#[test]
fn find_referencing_symbols_oxc_response_carries_cross_file_hint() {
    let project = project_root();
    fs::write(
        project.as_path().join("ref_target.ts"),
        "export function refreshEpisodeDonationCaches(seasonId: string) {\n    return seasonId;\n}\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({
            "file_path": "ref_target.ts",
            "symbol_name": "refreshEpisodeDonationCaches",
        }),
    );
    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["backend"], json!("oxc_semantic"));

    // The hint block must be present on every oxc_semantic response —
    // not just when count == 1 — so callers always know cross-file
    // callers require a separate tool.
    assert!(
        payload["data"]["precision_note"]
            .as_str()
            .unwrap_or("")
            .contains("oxc_semantic"),
        "precision_note must explain the oxc_semantic scope: {payload}"
    );
    assert_eq!(
        payload["data"]["cross_file_callers_hint"]["tool"],
        json!("get_callers"),
        "{payload}"
    );
}

/// Issue #214 regression: when oxc_semantic returns only the symbol's
/// own definition row (the prime symptom of the cross-file gap for an
/// exported function), the response must additionally carry a
/// `self_only_warning` so the caller is unambiguously told to follow
/// up with `get_callers`.
#[test]
fn find_referencing_symbols_oxc_definition_only_emits_self_only_warning() {
    let project = project_root();
    // Exported, never called within the file → oxc returns just the
    // definition row.
    fs::write(
        project.as_path().join("isolated_export.ts"),
        "export function refreshEpisodeDonationCaches(seasonId: string) {\n    return seasonId;\n}\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({
            "file_path": "isolated_export.ts",
            "symbol_name": "refreshEpisodeDonationCaches",
        }),
    );
    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["count"], json!(1), "{payload}");
    assert_eq!(
        payload["data"]["self_only_warning"]["code"],
        json!("definition_only"),
        "self_only_warning must be emitted when count == 1 and the row is the definition: {payload}"
    );
    assert_eq!(
        payload["data"]["self_only_warning"]["recommended_action"],
        json!("call_get_callers"),
        "{payload}"
    );
}

/// Issue #201 regression: when `find_referencing_symbols` returns only
/// the definition row, the response previously kept its precise-backend
/// confidence at 0.95 — making the silent miss look like a high-trust
/// "this symbol is unused" answer. Reviewers acting on that signal would
/// drop refactors. Degrade `evidence.confidence` (and the top-level
/// confidence mirror) when self-only is detected, and surface a
/// `degraded_reason` so an evidence-first reader gets the same warning
/// signal as the `self_only_warning` block carries.
#[test]
fn find_referencing_symbols_oxc_self_only_degrades_confidence_and_evidence() {
    let project = project_root();
    fs::write(
        project.as_path().join("isolated_export_for_201.ts"),
        "export function refreshEpisodeDonationCachesV2(seasonId: string) {\n    return seasonId;\n}\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({
            "file_path": "isolated_export_for_201.ts",
            "symbol_name": "refreshEpisodeDonationCachesV2",
        }),
    );
    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["count"], json!(1), "{payload}");

    let evidence_conf = payload["data"]["evidence"]["confidence"]
        .as_f64()
        .expect("evidence.confidence numeric");
    assert!(
        evidence_conf < 0.9,
        "self-only result must degrade evidence.confidence below the precise-path 0.95, got {evidence_conf}: {payload}"
    );
    let top_conf = payload["confidence"]
        .as_f64()
        .expect("top-level confidence numeric");
    assert!(
        (top_conf - evidence_conf).abs() < 1e-6,
        "top-level confidence must mirror evidence.confidence (top {top_conf} vs evidence {evidence_conf}): {payload}"
    );
    let degraded = payload["data"]["evidence"]["degraded_reason"]
        .as_str()
        .unwrap_or_default();
    assert!(
        degraded.contains("single_definition") || degraded.contains("self_only"),
        "evidence.degraded_reason must mark the self-only path, got `{degraded}`: {payload}"
    );
    assert_eq!(
        payload["data"]["evidence"]["confidence_basis"]
            .as_str()
            .unwrap_or_default(),
        "oxc_semantic_self_only",
        "confidence_basis must shift to the self-only label so reviewers can branch on it: {payload}"
    );
}

/// Issue #268 regression: TypeScript request/interface symbols can be
/// genuinely used through structural type annotations, `as Request`
/// casts, and schema parse flows even when the precise single-file
/// backend sees only the definition row. Surface that evidence in the
/// same response so agents do not convert a low precise count into an
/// orphan/dead-code conclusion.
#[test]
fn find_referencing_symbols_oxc_self_only_surfaces_ts_structural_evidence() {
    let project = project_root();
    fs::write(
        project.as_path().join("signature_types.ts"),
        "export interface GifPlanRequest {\n  customName: string;\n  customNumber: string;\n  slogan?: string;\n}\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("gif_route.ts"),
        "import type { GifPlanRequest } from './signature_types';\n\nconst schema = { safeParse(input: unknown) { return { data: input }; } };\nconst parsed = schema.safeParse({});\nconst body = parsed.data as GifPlanRequest;\nexport function handlePlan(input: GifPlanRequest) {\n  return input.customName;\n}\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({
            "file_path": "signature_types.ts",
            "symbol_name": "GifPlanRequest",
        }),
    );

    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(
        payload["data"]["backend"],
        json!("oxc_semantic"),
        "{payload}"
    );
    assert_eq!(payload["data"]["count"], json!(1), "{payload}");
    let structural_count = payload["data"]["structural_reference_evidence"]["count"]
        .as_u64()
        .expect("structural count");
    assert!(
        structural_count >= 3,
        "expected import/cast/annotation evidence, got {structural_count}: {payload}"
    );
    assert_eq!(
        payload["data"]["structural_reference_evidence"]["orphan_conclusion"],
        json!("not_safe_to_mark_unused"),
        "{payload}"
    );
    assert_eq!(
        payload["data"]["structural_usage_warning"]["code"],
        json!("ts_structural_evidence_present"),
        "{payload}"
    );
    assert_eq!(
        payload["data"]["evidence"]["confidence_basis"],
        json!("oxc_self_only_plus_ts_structural_evidence"),
        "{payload}"
    );
    assert!(
        payload["data"]["reference_evidence_count"]
            .as_u64()
            .unwrap_or(0)
            > 1,
        "reference_evidence_count must include structural evidence: {payload}"
    );
}

#[test]
fn find_referencing_symbols_ts_structural_evidence_filters_plain_name_noise() {
    let project = project_root();
    fs::write(
        project.as_path().join("filtered_request_types.ts"),
        "export interface FilteredPlanRequest {\n  customName: string;\n}\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("filtered_route.ts"),
        "import type { FilteredPlanRequest } from './filtered_request_types';\nconst body = parsed.data as FilteredPlanRequest;\nexport function handleFiltered(input: FilteredPlanRequest) {\n  return input.customName;\n}\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("plain_value_use.ts"),
        "export const maybeRuntimeValue = FilteredPlanRequest;\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("python_noise.py"),
        "print(FilteredPlanRequest)\n",
    )
    .unwrap();

    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({
            "file_path": "filtered_request_types.ts",
            "symbol_name": "FilteredPlanRequest",
        }),
    );

    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(
        payload["data"]["evidence"]["confidence_basis"],
        json!("oxc_self_only_plus_ts_structural_evidence"),
        "{payload}"
    );
    assert_eq!(
        payload["data"]["structural_reference_evidence"]["count"],
        json!(3),
        "only import/cast/annotation evidence should be counted: {payload}"
    );
    let evidence_rows = payload["data"]["structural_reference_evidence"]["references"]
        .as_array()
        .expect("structural evidence rows");
    assert!(
        evidence_rows
            .iter()
            .all(|row| row["file_path"] == json!("filtered_route.ts")),
        "plain TS value usage and non-TS files must not become structural evidence: {payload}"
    );
    assert!(
        evidence_rows
            .iter()
            .all(|row| row["evidence_kind"] != json!("text_name_match")),
        "plain name matches must be filtered out of TS structural evidence: {payload}"
    );
}

/// Issue #268 regression for the explicit LSP path: when a TS language
/// server returns zero/one references for a request interface, CodeLens
/// must still attach structural/cast evidence and downgrade the verdict
/// instead of returning a high-confidence empty result.
#[test]
fn find_referencing_symbols_lsp_low_count_surfaces_ts_structural_evidence() {
    let project = project_root();
    fs::write(
        project.as_path().join("request_types.ts"),
        "export interface GeneratePollRequest {\n  customName: string;\n  customNumber: string;\n}\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("poll_route.ts"),
        "import type { GeneratePollRequest } from './request_types';\nconst parsed = { data: {} };\nconst body = parsed.data as GeneratePollRequest;\nexport const consume = (input: GeneratePollRequest) => input.customNumber;\n",
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
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'referencesProvider':True}}})\n",
        "    elif m == 'textDocument/references':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':[]})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join("mock_empty_refs_lsp.py");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let state = make_state(&project);
    register_mock_lsp(&state, &mock_path);
    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({
            "file_path": "request_types.ts",
            "symbol_name": "GeneratePollRequest",
            "use_lsp": true,
            "command": "pyright-langserver",
            "args": ["--stdio"],
        }),
    );

    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["count"], json!(0), "{payload}");
    assert!(
        payload["data"]["structural_reference_evidence"]["count"]
            .as_u64()
            .unwrap_or(0)
            >= 3,
        "LSP low-count response must carry structural evidence: {payload}"
    );
    assert_eq!(
        payload["data"]["evidence"]["confidence_basis"],
        json!("lsp_low_count_plus_ts_structural_evidence"),
        "{payload}"
    );
    assert!(
        payload["confidence"].as_f64().unwrap_or(1.0) < 0.9,
        "LSP low-count + structural evidence must be downgraded from precise confidence: {payload}"
    );
}

// ── D1 (#346 Phase 4): LSP read trio — find_declaration /
//    find_implementations / get_diagnostics_for_symbol ────────────────

/// Mock LSP answering declaration with one location and implementation
/// with two, echoing back the request's document URI so the engine's
/// path conversion resolves to the fixture file.
fn write_mock_navigation_lsp(project: &ProjectRoot, name: &str) -> std::path::PathBuf {
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
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'textDocumentSync':1,'declarationProvider':True,'implementationProvider':True}}})\n",
        "    elif m == 'textDocument/declaration':\n",
        "        uri = msg['params']['textDocument']['uri']\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':[{'uri':uri,'range':{'start':{'line':0,'character':4},'end':{'line':0,'character':9}}}]})\n",
        "    elif m == 'textDocument/implementation':\n",
        "        uri = msg['params']['textDocument']['uri']\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':[\n",
        "            {'uri':uri,'range':{'start':{'line':0,'character':4},'end':{'line':0,'character':9}}},\n",
        "            {'uri':uri,'range':{'start':{'line':4,'character':4},'end':{'line':4,'character':8}}}\n",
        "        ]})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join(name);
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    mock_path
}

#[test]
fn find_declaration_returns_locations_via_mock_lsp() {
    let project = project_root();
    let mock_path = write_mock_navigation_lsp(&project, "mock_nav_lsp.py");
    fs::write(
        project.as_path().join("nav_decl.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    register_mock_lsp(&state, &mock_path);
    let payload = call_tool(
        &state,
        "find_declaration",
        json!({
            "relative_path": "nav_decl.py",
            "symbol_name": "alpha",
            "command": "pyright-langserver",
            "args": ["--stdio"]
        }),
    );
    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["operation"], json!("declaration"));
    assert_eq!(payload["data"]["count"], json!(1), "{payload}");
    let target = &payload["data"]["targets"][0];
    assert!(
        target["file_path"]
            .as_str()
            .unwrap_or("")
            .contains("nav_decl.py"),
        "{payload}"
    );
}

#[test]
fn find_implementations_returns_locations_via_mock_lsp() {
    let project = project_root();
    let mock_path = write_mock_navigation_lsp(&project, "mock_nav_impl_lsp.py");
    fs::write(
        project.as_path().join("nav_impl.py"),
        "def alpha():\n    return 1\n\n\ndef beta():\n    return 2\n",
    )
    .unwrap();
    let state = make_state(&project);
    register_mock_lsp(&state, &mock_path);
    let payload = call_tool(
        &state,
        "find_implementations",
        json!({
            "relative_path": "nav_impl.py",
            "symbol_name": "alpha",
            "command": "pyright-langserver",
            "args": ["--stdio"]
        }),
    );
    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["operation"], json!("implementation"));
    assert_eq!(payload["data"]["count"], json!(2), "{payload}");
}

#[test]
fn navigation_tools_degrade_gracefully_without_lsp() {
    // D1 contract: LSP-absent is a degraded SUCCESS, not an error —
    // the payload carries degraded_reason + fallback_hint steering the
    // caller to index-backed tools.
    let project = project_root();
    fs::write(
        project.as_path().join("nav_degraded.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    for tool in ["find_declaration", "find_implementations"] {
        let payload = call_tool(
            &state,
            tool,
            json!({
                "relative_path": "nav_degraded.py",
                "symbol_name": "alpha",
                "command": "definitely-not-a-real-lsp-binary-xyz"
            }),
        );
        assert_eq!(payload["success"], json!(true), "{tool}: {payload}");
        assert_eq!(payload["data"]["count"], json!(0), "{tool}: {payload}");
        assert!(
            payload["data"]["degraded_reason"]
                .as_str()
                .map(|reason| !reason.is_empty())
                .unwrap_or(false),
            "{tool} must carry degraded_reason: {payload}"
        );
        let hints = payload["data"]["fallback_hint"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(
            hints.iter().any(|hint| hint == "find_symbol")
                && hints.iter().any(|hint| hint == "bm25_symbol_search"),
            "{tool} fallback_hint must steer to index-backed tools: {payload}"
        );
    }
}

#[test]
fn get_diagnostics_for_symbol_filters_to_symbol_span() {
    let project = project_root();
    // Mock emits two diagnostics: line 2 (inside alpha) and line 6
    // (inside beta), 1-based. Asking for beta must return only beta's.
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
        "        send({'jsonrpc':'2.0','id':rid,'result':{'kind':'full','items':[\n",
        "            {'range':{'start':{'line':1,'character':4},'end':{'line':1,'character':9}},'severity':2,'message':'alpha warning'},\n",
        "            {'range':{'start':{'line':5,'character':4},'end':{'line':5,'character':9}},'severity':2,'message':'beta warning'}\n",
        "        ]}})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join("mock_symbol_diag_lsp.py");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(
        project.as_path().join("two_symbols.py"),
        "def alpha():\n    x = 1\n    return x\n\ndef beta():\n    y = 2\n    return y\n",
    )
    .unwrap();
    let state = make_state(&project);
    register_mock_lsp(&state, &mock_path);
    let payload = call_tool(
        &state,
        "get_diagnostics_for_symbol",
        json!({
            "relative_path": "two_symbols.py",
            "symbol_name": "beta",
            "command": "pyright-langserver",
            "args": ["--stdio"]
        }),
    );
    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["count"], json!(1), "{payload}");
    assert_eq!(
        payload["data"]["diagnostics"][0]["message"],
        json!("beta warning"),
        "{payload}"
    );
    assert_eq!(payload["data"]["symbol"]["name"], json!("beta"));
    assert!(
        payload["data"]["symbol"]["span"]["start_line"].is_u64(),
        "symbol span must be reported: {payload}"
    );
}

/// Mock `pyright-langserver` speaking `textDocument/references`. Installed
/// under `node_modules/.bin` so the LSP resolver's per-project fallback finds
/// it (the daemon's PATH has no real pyright), keyed the same as the real
/// server: command `pyright-langserver`, args `["--stdio"]`.
fn write_mock_pyright_references_lsp(project: &ProjectRoot) -> std::path::PathBuf {
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
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'textDocumentSync':1,'referencesProvider':True}}})\n",
        "    elif m == 'textDocument/references':\n",
        "        uri = msg['params']['textDocument']['uri']\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':[\n",
        "          {'uri':uri,'range':{'start':{'line':0,'character':6},'end':{'line':0,'character':12}}},\n",
        "          {'uri':uri,'range':{'start':{'line':4,'character':14},'end':{'line':4,'character':20}}}\n",
        "        ]})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let bin_dir = project.as_path().join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let mock_path = bin_dir.join("pyright-langserver");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    mock_path
}

/// Mock `pyright-langserver` returning FIVE `textDocument/references` spread
/// across two files (three in the queried document, two in a sibling
/// `other.py`) — a cross-file result above the text-channel clip cap. Used to
/// prove the explicit `use_lsp=true` LSP-primary path attaches the full_results
/// marker so the summarizer preserves the whole precise array instead of
/// clipping it to three. The multi-file shape also makes the cold-wait converge
/// immediately (no settle poll), keeping the test fast.
fn write_mock_pyright_multi_ref_lsp(project: &ProjectRoot) -> std::path::PathBuf {
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
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'textDocumentSync':1,'referencesProvider':True}}})\n",
        "    elif m == 'textDocument/references':\n",
        "        uri = msg['params']['textDocument']['uri']\n",
        "        uri2 = uri.rsplit('/', 1)[0] + '/other.py'\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':[\n",
        "          {'uri':uri,'range':{'start':{'line':0,'character':6},'end':{'line':0,'character':12}}},\n",
        "          {'uri':uri,'range':{'start':{'line':4,'character':14},'end':{'line':4,'character':20}}},\n",
        "          {'uri':uri2,'range':{'start':{'line':0,'character':4},'end':{'line':0,'character':10}}},\n",
        "          {'uri':uri2,'range':{'start':{'line':1,'character':4},'end':{'line':1,'character':10}}},\n",
        "          {'uri':uri2,'range':{'start':{'line':2,'character':4},'end':{'line':2,'character':10}}}\n",
        "        ]})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let bin_dir = project.as_path().join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let mock_path = bin_dir.join("pyright-langserver");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    mock_path
}

/// Regression (P8 follow-up): the explicit `use_lsp=true` LSP-primary success
/// path is the sixth complete-result reference path and — unlike the other five
/// — used to skip `mark_full_results`. So a `full_results=true` request over a
/// real cross-file result (count=20 in the parent's live probe) still had its
/// array clipped to the text-channel cap (n=3) and flagged truncated. This
/// drives the LSP-primary path with a mock returning five cross-file references
/// and asserts the marker is attached, every reference survives, and the parent
/// is not flagged truncated.
#[test]
fn use_lsp_full_results_serializes_every_reference_without_truncation() {
    let project = project_root();
    let mock_path = write_mock_pyright_multi_ref_lsp(&project);
    fs::write(
        project.as_path().join("widget.py"),
        "class Widget:\n    pass\n\n\ndef render(w: Widget) -> None:\n    return None\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("other.py"),
        "def a(w):\n    return w\ndef b(w):\n    return w\n",
    )
    .unwrap();
    let state = make_state(&project);
    register_mock_lsp(&state, &mock_path);

    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({
            "file_path": "widget.py",
            "symbol_name": "Widget",
            "use_lsp": true,
            "full_results": true,
        }),
    );
    assert_eq!(payload["success"], json!(true), "{payload}");

    // The LSP-primary path must have answered (not a fallback) …
    assert_eq!(
        payload["data"]["evidence"]["signals"]["precise_used"],
        json!(true),
        "the precise LSP result must be the one served: {payload}"
    );
    // … and it must carry the completeness marker the summarizer honors.
    assert_eq!(
        payload["data"]["full_results"],
        json!(true),
        "use_lsp=true + full_results=true must attach the full_results marker: {payload}"
    );

    // Every one of the five references survives — no clip to the n=3 cap …
    let refs = payload["data"]["references"]
        .as_array()
        .unwrap_or_else(|| panic!("references array missing: {payload}"));
    assert_eq!(
        refs.len(),
        5,
        "all five references must be serialized, not clipped to the cap: {payload}"
    );
    assert_eq!(
        payload["data"]["count"],
        json!(5),
        "count must report the full precise set: {payload}"
    );
    // … and the parent must not be flagged truncated for a complete result.
    assert_ne!(
        payload["data"]["truncated"],
        json!(true),
        "a full_results LSP result must not flag truncated: {payload}"
    );
}

/// Regression [C]: the default reference path (symbol_name only, no `use_lsp`)
/// must return the same deterministic tree-sitter result whether or not the
/// file's language server is warm. Warming pyright must NOT make the next
/// default call route through LSP — that warmth-dependent hijack (which also
/// dropped the full_results marker) was the regression.
#[test]
fn default_path_stays_deterministic_when_pyright_session_is_warm() {
    let project = project_root();
    let mock_path = write_mock_pyright_references_lsp(&project);
    fs::write(
        project.as_path().join("widget.py"),
        "class Widget:\n    pass\n\n\ndef render(w: Widget) -> None:\n    return None\n",
    )
    .unwrap();
    let state = make_state(&project);
    register_mock_lsp(&state, &mock_path);

    // Warm the pool: an explicit use_lsp=true call spawns the pyright shim
    // and leaves a live session keyed (pyright-langserver, ["--stdio"]).
    let warm = call_tool(
        &state,
        "find_referencing_symbols",
        json!({ "file_path": "widget.py", "symbol_name": "Widget", "use_lsp": true }),
    );
    assert_eq!(warm["success"], json!(true), "warm-up call: {warm}");

    // Default path with full_results (the parent's exact probe [C]): with a
    // warm session resident, the answer must still be the tree-sitter result —
    // no LSP hijack, no warm-LSP routing note — AND full_results must be honored
    // so the array is not clipped/flagged truncated (regression [C]+[D]).
    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({ "file_path": "widget.py", "symbol_name": "Widget", "full_results": true }),
    );
    assert_eq!(payload["success"], json!(true), "default call: {payload}");
    assert_ne!(
        payload["data"]["backend"],
        json!("lsp"),
        "a warm session must not hijack the default path into LSP: {payload}"
    );
    assert!(
        payload["data"].get("routing_note").is_none(),
        "the deterministic default path emits no warm-LSP routing note: {payload}"
    );
    // full_results must be honored on the (non-hijacked) tree-sitter path so the
    // response summarizer preserves the array instead of clipping + truncating.
    assert_eq!(
        payload["data"]["full_results"],
        json!(true),
        "full_results marker must be attached so the array is preserved: {payload}"
    );
    // It advertises use_lsp=true as the precision opt-in instead of routing.
    assert_eq!(
        payload["data"]["lsp_precision_hint"]["code"],
        json!("lsp_precision_available"),
        "default path must surface the use_lsp precision hint: {payload}"
    );
}

/// Cold pyright (no warm session): the default path stays on tree-sitter but
/// surfaces a hint steering Python callers toward `use_lsp=true` for
/// annotation-aware precision. Deterministic — the warmth probe never spawns.
#[test]
fn default_path_emits_cold_lsp_hint_for_python_without_warm_server() {
    let project = project_root();
    fs::write(
        project.as_path().join("cold.py"),
        "class Gadget:\n    pass\n\n\ndef use(g: Gadget) -> None:\n    return None\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({ "file_path": "cold.py", "symbol_name": "Gadget" }),
    );
    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(
        payload["data"]["evidence"]["signals"]["fallback_source"],
        json!("tree_sitter"),
        "cold pyright must stay on tree-sitter: {payload}"
    );
    assert_eq!(
        payload["data"]["lsp_precision_hint"]["code"],
        json!("lsp_precision_available"),
        "cold Python default path must emit the annotation-aware hint: {payload}"
    );
    assert_eq!(
        payload["data"]["lsp_precision_hint"]["server"],
        json!("pyright-langserver"),
        "hint must name the pyright server: {payload}"
    );
}

/// Regression [C], real-pyright variant. Skips cleanly when the binary is
/// absent so CI without pyright stays green; when installed, proves that even a
/// genuinely warm pyright session does NOT hijack the default path — the answer
/// stays the deterministic tree-sitter result (use_lsp=true remains the opt-in
/// for precise annotation-aware references).
#[test]
fn default_path_stays_deterministic_with_real_warm_pyright_when_installed() {
    if !codelens_engine::lsp_binary_exists("pyright-langserver") {
        eprintln!(
            "skipping default_path_stays_deterministic_with_real_warm_pyright_when_installed: pyright-langserver not installed"
        );
        return;
    }
    let project = project_root();
    fs::write(
        project.as_path().join("annotated.py"),
        "class Service:\n    pass\n\n\ndef build(s: Service) -> Service:\n    return s\n",
    )
    .unwrap();
    let state = make_state(&project);

    let warm = call_tool(
        &state,
        "find_referencing_symbols",
        json!({ "file_path": "annotated.py", "symbol_name": "Service", "use_lsp": true }),
    );
    assert_eq!(warm["success"], json!(true), "real pyright warm-up: {warm}");

    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({ "file_path": "annotated.py", "symbol_name": "Service" }),
    );
    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_ne!(
        payload["data"]["backend"],
        json!("lsp"),
        "a real warm pyright must not hijack the default path into LSP: {payload}"
    );
    assert!(
        payload["data"].get("routing_note").is_none(),
        "the deterministic default path emits no warm-LSP routing note: {payload}"
    );
}

/// P8 (real pyright): an explicit `use_lsp=true` query must return the COMPLETE
/// cross-file reference set, not the declaration-only cold read. Before P8 the
/// bounded cold-wait short-circuited on pyright's non-empty-but-incomplete first
/// read (the declaration alone, resolved off the just-opened document); the
/// under-report guard then served the tree-sitter text set, so `use_lsp=true`
/// never delivered a precise LSP answer. This drives a real pyright over a
/// three-file fixture (one definition referenced from two other files) and
/// asserts the precise backend spans every referencing file. Skips cleanly when
/// pyright is absent so CI without it stays green.
#[test]
fn use_lsp_returns_complete_cross_file_references_with_real_pyright() {
    if !codelens_engine::lsp_binary_exists("pyright-langserver") {
        eprintln!(
            "skipping use_lsp_returns_complete_cross_file_references_with_real_pyright: pyright-langserver not installed"
        );
        return;
    }
    let project = project_root();
    // Same-file references resolve immediately off the open document; the
    // cross-file references in alpha.py/beta.py only land after pyright's
    // background workspace scan — exactly the race P8's convergence wait closes.
    fs::write(
        project.as_path().join("core.py"),
        "def process(value):\n    return value + 1\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("alpha.py"),
        "from core import process\n\n\ndef run_alpha():\n    return process(1)\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("beta.py"),
        "from core import process\n\n\ndef run_beta():\n    return process(2)\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({
            "file_path": "core.py",
            "symbol_name": "process",
            "use_lsp": true,
            "full_results": true,
        }),
    );
    assert_eq!(payload["success"], json!(true), "{payload}");

    // The precise LSP backend must be the one that answered — not the
    // tree-sitter text fallback the under-report guard used to serve.
    assert_eq!(
        payload["data"]["evidence"]["active_backend"],
        json!("lsp"),
        "precise LSP backend must serve the answer, not a fallback: {payload}"
    );
    assert_eq!(
        payload["data"]["evidence"]["signals"]["precise_used"],
        json!(true),
        "the precise LSP result must be used, not the text fallback: {payload}"
    );
    assert!(
        payload["data"].get("lsp_underreport_warning").is_none(),
        "a complete cross-file LSP result must not trip the under-report guard: {payload}"
    );

    // Completeness: references must span every referencing file, not just the
    // declaration's own file (the cold-read short-circuit symptom).
    let refs = payload["data"]["references"]
        .as_array()
        .unwrap_or_else(|| panic!("references array missing: {payload}"));
    let mut files: Vec<String> = refs
        .iter()
        .filter_map(|r| r["file_path"].as_str().map(str::to_owned))
        .collect();
    files.sort();
    files.dedup();
    for expected in ["alpha.py", "beta.py", "core.py"] {
        assert!(
            files.iter().any(|f| f.ends_with(expected)),
            "cross-file references must include {expected}; got files={files:?}: {payload}"
        );
    }
}

#[test]
fn lsp_read_trio_visible_on_read_surfaces() {
    use crate::tool_defs::{ToolProfile, ToolSurface, is_tool_in_surface};
    for name in [
        "find_declaration",
        "find_implementations",
        "get_diagnostics_for_symbol",
    ] {
        assert!(
            crate::tool_defs::tools().iter().any(|t| t.name == name),
            "{name} must be registered in tools.toml"
        );
        // 2026-07 tool-surface diet: the LSP read trio left the curated
        // reviewer-graph core-20 (still callable via tools/call), so the
        // "visible on read surface" invariant now covers planner + builder.
        for profile in [ToolProfile::PlannerReadonly, ToolProfile::BuilderMinimal] {
            assert!(
                is_tool_in_surface(name, ToolSurface::Profile(profile)),
                "{name} must be visible on {profile:?}"
            );
        }
    }
}
