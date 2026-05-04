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
    let payload = call_tool(
        &state,
        "get_file_diagnostics",
        json!({ "file_path": "diag_target.py", "command": "python3", "args": [mock_path.to_string_lossy()] }),
    );
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn get_file_diagnostics_accepts_legacy_file_path_with_deprecation_warning() {
    let project = project_root();
    let mock_path = write_mock_diagnostics_lsp(&project, "mock_legacy_diag_lsp.py");
    fs::write(project.as_path().join("legacy_diag.py"), "x = 1\n").unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "get_file_diagnostics",
        json!({ "file_path": "legacy_diag.py", "command": "python3", "args": [mock_path.to_string_lossy()] }),
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

#[test]
fn rename_symbol_uses_opt_in_lsp_semantic_edit_backend() {
    let project = project_root();
    let original = "def old_name():\n    pass\n\nold_name()\n";
    fs::write(project.as_path().join("rename_target.py"), original).unwrap();
    let original_hash = sha256_hex_text(original);
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
        "    elif m == 'textDocument/rename':\n",
        "        uri = msg['params']['textDocument']['uri']\n",
        "        new_name = msg['params']['newName']\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'changes':{uri:[{'range':{'start':{'line':0,'character':4},'end':{'line':0,'character':12}},'newText':new_name},{'range':{'start':{'line':3,'character':0},'end':{'line':3,'character':8}},'newText':new_name}]}}})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join("mock_lsp_rename_apply.py");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "rename_symbol",
        json!({
            "file_path": "rename_target.py",
            "symbol_name": "old_name",
            "new_name": "new_name",
            "semantic_edit_backend": "lsp",
            "line": 1,
            "column": 5,
            "command": "python3",
            "args": [mock_path.to_string_lossy()]
        }),
    );

    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["semantic_edit_backend"], json!("lsp"));
    assert_eq!(payload["data"]["authority"], json!("workspace_edit"));
    assert_eq!(payload["data"]["authority_backend"], json!("lsp:python3"));
    assert_eq!(payload["data"]["support"], json!("authoritative_apply"));
    assert_eq!(payload["data"]["can_preview"], json!(true));
    assert_eq!(payload["data"]["can_apply"], json!(true));
    assert_eq!(payload["data"]["blocker_reason"], json!(null));
    assert_eq!(
        payload["data"]["transaction"]["contract"]["backend_id"],
        json!("lsp:python3")
    );
    assert_eq!(
        payload["data"]["transaction"]["contract"]["file_hashes_before"]["rename_target.py"]["sha256"],
        json!(original_hash),
        "{payload}"
    );
    assert_eq!(
        payload["data"]["edit_authority"],
        json!({
            "kind": "authoritative_lsp",
            "operation": "rename",
            "embedding_used": false,
            "search_used": false,
            "position_source": "explicit",
            "validator": "lsp_textDocument_rename"
        })
    );
    assert_eq!(payload["data"]["total_replacements"], json!(2));
    let updated = fs::read_to_string(project.as_path().join("rename_target.py")).unwrap();
    assert!(updated.contains("def new_name():"));
    assert!(updated.contains("new_name()"));
}

#[test]
fn propagate_deletions_uses_lsp_safe_delete_check() {
    let project = project_root();
    fs::write(
        project.as_path().join("delete_target.py"),
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
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'referencesProvider':True}}})\n",
        "    elif m == 'textDocument/references':\n",
        "        uri = msg['params']['textDocument']['uri']\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':[{'uri':uri,'range':{'start':{'line':0,'character':4},'end':{'line':0,'character':12}}},{'uri':uri,'range':{'start':{'line':3,'character':0},'end':{'line':3,'character':8}}}]})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join("mock_lsp_safe_delete.py");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "propagate_deletions",
        json!({
            "file_path": "delete_target.py",
            "symbol_name": "old_name",
            "semantic_edit_backend": "lsp",
            "line": 1,
            "column": 5,
            "command": "python3",
            "args": [mock_path.to_string_lossy()],
            "dry_run": true
        }),
    );

    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["semantic_edit_backend"], json!("lsp"));
    assert_eq!(payload["data"]["authority"], json!("semantic_readonly"));
    assert_eq!(payload["data"]["authority_backend"], json!("lsp:python3"));
    assert_eq!(payload["data"]["support"], json!("authoritative_check"));
    assert_eq!(payload["data"]["can_preview"], json!(true));
    assert_eq!(payload["data"]["can_apply"], json!(false));
    assert_eq!(payload["data"]["blocker_reason"], json!(null));
    assert_eq!(
        payload["data"]["transaction"]["contract"]["verification_result"]["references_checked"],
        json!(true)
    );
    assert_eq!(
        payload["data"]["edit_authority"],
        json!({
            "kind": "authoritative_lsp",
            "operation": "safe_delete_check",
            "embedding_used": false,
            "search_used": false,
            "position_source": "explicit",
            "validator": "lsp_textDocument_references"
        })
    );
    assert_eq!(payload["data"]["safe_to_delete"], json!(false));
    assert_eq!(payload["data"]["total_references"], json!(1), "{payload}");
    assert_eq!(
        payload["data"]["declaration_references"],
        json!(1),
        "{payload}"
    );
}

#[test]
fn propagate_deletions_lsp_safe_delete_apply_removes_isolated_symbol() {
    let project = project_root();
    fs::write(
        project.as_path().join("delete_apply.py"),
        "def old_name():\n    return 1\n\nvalue = 2\n",
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
        "        uri = msg['params']['textDocument']['uri']\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':[{'uri':uri,'range':{'start':{'line':0,'character':4},'end':{'line':0,'character':12}}}]})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join("mock_lsp_safe_delete_apply.py");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "propagate_deletions",
        json!({
            "file_path": "delete_apply.py",
            "symbol_name": "old_name",
            "semantic_edit_backend": "lsp",
            "line": 1,
            "column": 5,
            "command": "python3",
            "args": [mock_path.to_string_lossy()],
            "dry_run": false
        }),
    );

    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["safe_delete_action"], json!("applied"));
    assert_eq!(payload["data"]["transaction"]["modified_files"], json!(1));
    let updated = fs::read_to_string(project.as_path().join("delete_apply.py")).unwrap();
    assert!(!updated.contains("old_name"));
    assert!(updated.contains("value = 2"));
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
    let payload = call_tool(
        &state,
        "resolve_symbol_target",
        json!({
            "file_path": "target.rs",
            "line": 2,
            "column": 13,
            "target": "implementation",
            "semantic_backend": "lsp",
            "command": "python3",
            "args": [mock_path.to_string_lossy()]
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
    let payload = call_tool(
        &state,
        "refactor_extract_function",
        json!({
            "file_path": "target.ts",
            "start_line": 1,
            "end_line": 1,
            "new_name": "extracted",
            "semantic_edit_backend": "lsp",
            "command": "python3",
            "args": [mock_path.to_string_lossy()],
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
