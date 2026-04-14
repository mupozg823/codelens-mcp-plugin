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

#[cfg(feature = "scip-backend")]
#[test]
fn returns_scip_references_via_tool_call_when_index_is_present() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).unwrap();
    fs::write(
        project.as_path().join("src/main.rs"),
        "pub struct MyStruct;\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("src/lib.rs"),
        "use crate::MyStruct;\n",
    )
    .unwrap();
    write_test_scip_index(&project);

    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({ "file_path": "src/main.rs", "symbol_name": "MyStruct", "max_results": 10 }),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["backend"], json!("scip"));
    assert_eq!(payload["data"]["count"], json!(2));
    let files = payload["data"]["references"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["file_path"].as_str())
        .collect::<Vec<_>>();
    assert!(files.contains(&"src/main.rs"));
    assert!(files.contains(&"src/lib.rs"));
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

#[cfg(feature = "scip-backend")]
#[test]
fn returns_scip_diagnostics_via_tool_call_when_index_is_present() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).unwrap();
    fs::write(
        project.as_path().join("src/main.rs"),
        "pub struct MyStruct;\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("src/lib.rs"),
        "use crate::MyStruct;\n",
    )
    .unwrap();
    write_test_scip_index(&project);

    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "get_file_diagnostics",
        json!({ "file_path": "src/main.rs", "max_results": 10 }),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["backend"], json!("scip"));
    assert_eq!(payload["data"]["count"], json!(1));
    assert_eq!(
        payload["data"]["diagnostics"][0]["message"],
        json!("test scip warning")
    );
    assert_eq!(payload["data"]["diagnostics"][0]["code"], json!("SCIP001"));
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
