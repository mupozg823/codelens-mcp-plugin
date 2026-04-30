use super::*;

// ── P1-A: tree-sitter honesty tests ─────────────────────────────────

/// AC5 — For each of the 4 tree-sitter refactor handlers, verify:
///  1. `tree_sitter_caveats` is a non-empty array
///  2. `unknown_args == ["banana"]` for a deliberately-unknown key
///  3. `degraded_reason` at top level equals the expected honesty string
#[test]
fn p1_a_refactor_tools_emit_tree_sitter_caveats_and_unknown_args() {
    let project = project_root();
    // Create a small Python file with a real function so tree-sitter path succeeds
    fs::write(
        project.as_path().join("caveat_target.py"),
        "def greet(name):\n    print(name)\n\ngreet('world')\n",
    )
    .unwrap();
    let state = make_state(&project);

    const DEGRADED_REASON: &str = "tree-sitter heuristic — no semantic analysis";

    // ── refactor_extract_function ──────────────────────────────────────
    {
        let payload = call_tool(
            &state,
            "refactor_extract_function",
            json!({
                "file_path": "caveat_target.py",
                "start_line": 2,
                "end_line": 2,
                "new_name": "print_name",
                "dry_run": true,
                "banana": true
            }),
        );
        let data = payload.get("data").unwrap_or(&payload);
        let caveats = data
            .get("tree_sitter_caveats")
            .expect("tree_sitter_caveats must be present in refactor_extract_function response");
        assert!(
            caveats.as_array().map(|a| !a.is_empty()).unwrap_or(false),
            "tree_sitter_caveats must be a non-empty array: {payload}"
        );
        assert_eq!(
            data["unknown_args"],
            json!(["banana"]),
            "refactor_extract_function: unknown_args mismatch: {payload}"
        );
        assert_eq!(
            data["degraded_reason"],
            json!(DEGRADED_REASON),
            "refactor_extract_function: degraded_reason mismatch: {payload}"
        );
    }

    // ── refactor_inline_function ───────────────────────────────────────
    {
        let payload = call_tool(
            &state,
            "refactor_inline_function",
            json!({
                "file_path": "caveat_target.py",
                "function_name": "greet",
                "dry_run": true,
                "banana": true
            }),
        );
        let data = payload.get("data").unwrap_or(&payload);
        let caveats = data
            .get("tree_sitter_caveats")
            .expect("tree_sitter_caveats must be present in refactor_inline_function response");
        assert!(
            caveats.as_array().map(|a| !a.is_empty()).unwrap_or(false),
            "tree_sitter_caveats must be a non-empty array: {payload}"
        );
        assert_eq!(
            data["unknown_args"],
            json!(["banana"]),
            "refactor_inline_function: unknown_args mismatch: {payload}"
        );
        assert_eq!(
            data["degraded_reason"],
            json!(DEGRADED_REASON),
            "refactor_inline_function: degraded_reason mismatch: {payload}"
        );
    }

    // ── refactor_move_to_file ──────────────────────────────────────────
    {
        let payload = call_tool(
            &state,
            "refactor_move_to_file",
            json!({
                "file_path": "caveat_target.py",
                "symbol_name": "greet",
                "target_file": "dest.py",
                "dry_run": true,
                "banana": true
            }),
        );
        let data = payload.get("data").unwrap_or(&payload);
        let caveats = data
            .get("tree_sitter_caveats")
            .expect("tree_sitter_caveats must be present in refactor_move_to_file response");
        assert!(
            caveats.as_array().map(|a| !a.is_empty()).unwrap_or(false),
            "tree_sitter_caveats must be a non-empty array: {payload}"
        );
        assert_eq!(
            data["unknown_args"],
            json!(["banana"]),
            "refactor_move_to_file: unknown_args mismatch: {payload}"
        );
        assert_eq!(
            data["degraded_reason"],
            json!(DEGRADED_REASON),
            "refactor_move_to_file: degraded_reason mismatch: {payload}"
        );
    }

    // ── refactor_change_signature ──────────────────────────────────────
    {
        let payload = call_tool(
            &state,
            "refactor_change_signature",
            json!({
                "file_path": "caveat_target.py",
                "function_name": "greet",
                "new_parameters": [{"name": "greeting", "type": "str"}],
                "dry_run": true,
                "banana": true
            }),
        );
        let data = payload.get("data").unwrap_or(&payload);
        let caveats = data
            .get("tree_sitter_caveats")
            .expect("tree_sitter_caveats must be present in refactor_change_signature response");
        assert!(
            caveats.as_array().map(|a| !a.is_empty()).unwrap_or(false),
            "tree_sitter_caveats must be a non-empty array: {payload}"
        );
        assert_eq!(
            data["unknown_args"],
            json!(["banana"]),
            "refactor_change_signature: unknown_args mismatch: {payload}"
        );
        assert_eq!(
            data["degraded_reason"],
            json!(DEGRADED_REASON),
            "refactor_change_signature: degraded_reason mismatch: {payload}"
        );
    }
}

#[test]
fn refactor_extract_function_applies_lsp_code_action_workspace_edit() {
    let project = project_root();
    let original = "const value = 1;\nexport {};\n";
    fs::write(project.as_path().join("target.ts"), original).unwrap();
    let original_hash = sha256_hex_text(original);
    let mock_path = write_code_action_mock(&project, "refactor.extract", "extracted();");
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
            "dry_run": false
        }),
    );

    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["edit_authority"]["backend"], json!("lsp"));
    assert_eq!(payload["data"]["authority"], json!("workspace_edit"));
    assert_eq!(payload["data"]["authority_backend"], json!("lsp:python3"));
    assert_eq!(
        payload["data"]["support"],
        json!("conditional_authoritative_apply")
    );
    assert_eq!(payload["data"]["can_preview"], json!(true));
    assert_eq!(payload["data"]["can_apply"], json!(true));
    assert_eq!(payload["data"]["blocker_reason"], json!(null));
    assert_eq!(
        payload["data"]["transaction"]["contract"]["model"],
        json!("transactional_best_effort_with_rollback_evidence")
    );
    assert_eq!(
        payload["data"]["transaction"]["contract"]["file_hashes_before"]["target.ts"]["sha256"],
        json!(original_hash),
        "{payload}"
    );
    assert_eq!(
        fs::read_to_string(project.as_path().join("target.ts")).unwrap(),
        "extracted();\nexport {};\n"
    );
}

#[test]
fn refactor_inline_function_applies_lsp_code_action_workspace_edit() {
    let project = project_root();
    fs::write(
        project.as_path().join("target.ts"),
        "const value = 1;\nexport {};\n",
    )
    .unwrap();
    let mock_path = write_code_action_mock(&project, "refactor.inline", "inlined();");
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "refactor_inline_function",
        json!({
            "file_path": "target.ts",
            "function_name": "value",
            "line": 1,
            "column": 7,
            "semantic_edit_backend": "lsp",
            "command": "python3",
            "args": [mock_path.to_string_lossy()],
            "dry_run": false
        }),
    );

    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["operation"], json!("inline_function"));
    assert_eq!(
        fs::read_to_string(project.as_path().join("target.ts")).unwrap(),
        "inlined();\nexport {};\n"
    );
}

#[test]
fn refactor_move_to_file_applies_lsp_code_action_workspace_edit() {
    let project = project_root();
    fs::write(
        project.as_path().join("target.ts"),
        "const value = 1;\nexport {};\n",
    )
    .unwrap();
    let mock_path = write_code_action_mock(&project, "refactor.rewrite", "moved();");
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "refactor_move_to_file",
        json!({
            "file_path": "target.ts",
            "symbol_name": "value",
            "target_file": "other.ts",
            "line": 1,
            "column": 7,
            "semantic_edit_backend": "lsp",
            "command": "python3",
            "args": [mock_path.to_string_lossy()],
            "dry_run": false
        }),
    );

    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["operation"], json!("move_symbol"));
    assert_eq!(
        fs::read_to_string(project.as_path().join("target.ts")).unwrap(),
        "moved();\nexport {};\n"
    );
}

#[test]
fn refactor_change_signature_applies_lsp_code_action_workspace_edit() {
    let project = project_root();
    fs::write(
        project.as_path().join("target.ts"),
        "const value = 1;\nexport {};\n",
    )
    .unwrap();
    let mock_path = write_code_action_mock(&project, "refactor.rewrite", "signatureChanged();");
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "refactor_change_signature",
        json!({
            "file_path": "target.ts",
            "function_name": "value",
            "new_parameters": [{"name": "next", "type": "number"}],
            "line": 1,
            "column": 7,
            "semantic_edit_backend": "lsp",
            "command": "python3",
            "args": [mock_path.to_string_lossy()],
            "dry_run": false
        }),
    );

    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["operation"], json!("change_signature"));
    assert_eq!(
        fs::read_to_string(project.as_path().join("target.ts")).unwrap(),
        "signatureChanged();\nexport {};\n"
    );
}

/// T3-1: LSP rename evidence is fact-based (substrate-derived), not placeholder.
/// Verifies that `file_hashes_before`, `file_hashes_after`, and `apply_status`
/// are populated from ApplyEvidence when the substrate apply path is taken.
#[test]
fn lsp_rename_evidence_is_fact_based_not_placeholder() {
    let project = project_root();
    let original = "def target_fn():\n    pass\n\ntarget_fn()\n";
    fs::write(project.as_path().join("evidence_target.py"), original).unwrap();
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
        "        send({'jsonrpc':'2.0','id':rid,'result':{'range':{'start':{'line':0,'character':4},'end':{'line':0,'character':13}},'placeholder':'target_fn'}})\n",
        "    elif m == 'textDocument/rename':\n",
        "        uri = msg['params']['textDocument']['uri']\n",
        "        new_name = msg['params']['newName']\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'changes':{uri:[{'range':{'start':{'line':0,'character':4},'end':{'line':0,'character':13}},'newText':new_name},{'range':{'start':{'line':3,'character':0},'end':{'line':3,'character':9}},'newText':new_name}]}}})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join("mock_lsp_rename_evidence_t3_1.py");
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
            "file_path": "evidence_target.py",
            "symbol_name": "target_fn",
            "new_name": "renamed_fn",
            "semantic_edit_backend": "lsp",
            "line": 1,
            "column": 5,
            "command": "python3",
            "args": [mock_path.to_string_lossy()]
        }),
    );

    assert_eq!(payload["success"], json!(true), "{payload}");
    let tx = &payload["data"]["transaction"]["contract"];

    // T3-1: file_hashes_before must be populated with the pre-apply sha256
    let hashes_before = tx["file_hashes_before"]
        .as_object()
        .expect("file_hashes_before should be an object");
    assert!(
        !hashes_before.is_empty(),
        "hashes_before should be populated, got: {tx}"
    );
    assert_eq!(
        hashes_before["evidence_target.py"]["sha256"],
        json!(original_hash),
        "hashes_before sha256 should match pre-apply content"
    );

    // T3-1: file_hashes_after must also be populated (substrate-derived)
    let hashes_after = tx["file_hashes_after"]
        .as_object()
        .expect("file_hashes_after should be an object");
    assert_eq!(
        hashes_before.len(),
        hashes_after.len(),
        "hashes_before and after should have same key set"
    );
    for (path, before) in hashes_before {
        assert!(
            before["sha256"]
                .as_str()
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            "hashes_before[{path}].sha256 should be non-empty"
        );
    }

    // T3-1: apply_status must be substrate-derived, not a placeholder
    assert!(
        matches!(
            tx["apply_status"].as_str(),
            Some("applied") | Some("rolled_back") | Some("no_op")
        ),
        "apply_status should be substrate-derived: {:?}",
        tx["apply_status"]
    );

    // Sanity: file was actually modified
    let updated = fs::read_to_string(project.as_path().join("evidence_target.py")).unwrap();
    assert!(updated.contains("def renamed_fn():"), "{updated}");
    assert!(updated.contains("renamed_fn()"), "{updated}");
}

// ── T9: safe_delete substrate migration tests ──────────────────────────────

/// Returns a minimal LSP mock script (as a String) that reports only the
/// declaration reference at (line=0, char=4..9) so `safe_to_delete == true`.
fn safe_delete_lsp_mock_src() -> String {
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
        "        send({'jsonrpc':'2.0','id':rid,'result':[{'uri':uri,'range':{'start':{'line':0,'character':4},'end':{'line':0,'character':9}}}]})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    )
    .to_owned()
}

fn write_safe_delete_mock(
    project: &codelens_engine::ProjectRoot,
    suffix: &str,
) -> std::path::PathBuf {
    let mock_path = project.as_path().join(format!("mock_lsp_sd_{suffix}.py"));
    fs::write(&mock_path, safe_delete_lsp_mock_src()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    mock_path
}

#[test]
fn safe_delete_substrate_dry_run_advertises_preview_only() {
    let project = project_root();
    let path = project.as_path().join("sd_dry.py");
    fs::write(&path, "def alpha():\n    pass\n").unwrap();
    let mock = write_safe_delete_mock(&project, "dry");
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "propagate_deletions",
        json!({
            "file_path": "sd_dry.py",
            "symbol_name": "alpha",
            "semantic_edit_backend": "lsp",
            "line": 1,
            "column": 5,
            "command": "python3",
            "args": [mock.to_string_lossy()],
            "dry_run": true
        }),
    );
    let scope = payload.get("data").unwrap_or(&payload);
    let tx = &scope["transaction"]["contract"];
    assert_eq!(tx["apply_status"], json!("preview_only"), "{payload}");
}

#[test]
fn safe_delete_substrate_real_apply_returns_evidence() {
    let project = project_root();
    let path = project.as_path().join("sd_apply.py");
    fs::write(&path, "def alpha():\n    pass\n").unwrap();
    let mock = write_safe_delete_mock(&project, "apply");
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "propagate_deletions",
        json!({
            "file_path": "sd_apply.py",
            "symbol_name": "alpha",
            "semantic_edit_backend": "lsp",
            "line": 1,
            "column": 5,
            "command": "python3",
            "args": [mock.to_string_lossy()],
            "dry_run": false
        }),
    );
    let scope = payload.get("data").unwrap_or(&payload);
    let tx = &scope["transaction"]["contract"];
    assert_eq!(tx["apply_status"], json!("applied"), "{payload}");
    assert_eq!(tx["rollback_plan"]["available"], json!(true), "{payload}");
    let hashes_after = tx["file_hashes_after"]
        .as_object()
        .expect("file_hashes_after must be an object");
    assert!(
        !hashes_after.is_empty(),
        "file_hashes_after must be populated"
    );
    let report = tx["rollback_report"]
        .as_array()
        .expect("rollback_report must be an array");
    assert!(
        report.is_empty(),
        "rollback_report should be empty on success"
    );
    let after = fs::read_to_string(&path).unwrap();
    assert!(
        !after.contains("def alpha"),
        "alpha should be deleted: {after:?}"
    );
}

#[cfg(unix)]
#[test]
fn safe_delete_substrate_rollback_when_write_blocked() {
    use std::os::unix::fs::PermissionsExt;
    let project = project_root();
    let path = project.as_path().join("sd_rollback.py");
    fs::write(&path, "def alpha():\n    pass\n").unwrap();
    let mock = write_safe_delete_mock(&project, "rollback");
    let state = make_state(&project);

    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o444);
    fs::set_permissions(&path, perms).unwrap();

    let payload = call_tool(
        &state,
        "propagate_deletions",
        json!({
            "file_path": "sd_rollback.py",
            "symbol_name": "alpha",
            "semantic_edit_backend": "lsp",
            "line": 1,
            "column": 5,
            "command": "python3",
            "args": [mock.to_string_lossy()],
            "dry_run": false
        }),
    );

    // Restore permissions immediately so temp dir cleanup works.
    let mut restore = fs::metadata(&path).unwrap().permissions();
    restore.set_mode(0o644);
    let _ = fs::set_permissions(&path, restore);

    let scope = payload.get("data").unwrap_or(&payload);
    let tx = &scope["transaction"]["contract"];
    assert_eq!(tx["apply_status"], json!("rolled_back"), "{payload}");
    let report = tx["rollback_report"]
        .as_array()
        .expect("rollback_report must be an array");
    assert!(
        !report.is_empty(),
        "rollback_report should be populated on rollback: {payload}"
    );
}

#[test]
fn safe_delete_substrate_dry_run_preserves_outer_fields() {
    let project = project_root();
    let path = project.as_path().join("sd_dry2.py");
    fs::write(&path, "def alpha():\n    pass\n").unwrap();
    let mock = write_safe_delete_mock(&project, "dry2");
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "propagate_deletions",
        json!({
            "file_path": "sd_dry2.py",
            "symbol_name": "alpha",
            "semantic_edit_backend": "lsp",
            "line": 1,
            "column": 5,
            "command": "python3",
            "args": [mock.to_string_lossy()],
            "dry_run": true
        }),
    );
    let scope = payload.get("data").unwrap_or(&payload);
    assert!(
        scope.get("safe_to_delete").is_some(),
        "safe_to_delete must be present: {payload}"
    );
    assert!(
        scope.get("affected_references").is_some(),
        "affected_references must be present: {payload}"
    );
}

fn write_code_action_mock(
    project: &codelens_engine::ProjectRoot,
    kind: &str,
    new_text: &str,
) -> std::path::PathBuf {
    let mock_path = project
        .as_path()
        .join(format!("mock_{}_lsp.py", kind.replace('.', "_")));
    fs::write(&mock_path, code_action_mock(kind, new_text)).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    mock_path
}

fn code_action_mock(kind: &str, new_text: &str) -> String {
    format!(
        r#"#!/usr/bin/env python3
import json
import sys

KIND = {kind:?}
NEW_TEXT = {new_text:?}

def read_message():
    headers = {{}}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, value = line.decode("utf-8").split(":", 1)
        headers[name.strip().lower()] = value.strip()
    body = sys.stdin.buffer.read(int(headers["content-length"]))
    return json.loads(body.decode("utf-8"))

def send(payload):
    body = json.dumps(payload).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {{len(body)}}\r\n\r\n".encode("utf-8"))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()

while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if method == "initialize":
        send({{"jsonrpc":"2.0","id":message["id"],"result":{{"capabilities":{{"codeActionProvider":{{"resolveProvider": True}}}}}}}})
    elif method == "textDocument/codeAction":
        uri = message["params"]["textDocument"]["uri"]
        send({{"jsonrpc":"2.0","id":message["id"],"result":[{{
            "title": "CodeLens test refactor",
            "kind": KIND,
            "edit": {{"changes": {{uri: [{{"range": {{"start": {{"line": 0, "character": 0}}, "end": {{"line": 0, "character": 16}}}}, "newText": NEW_TEXT}}]}}}}
        }}]}})
    elif method == "codeAction/resolve":
        send({{"jsonrpc":"2.0","id":message["id"],"result":message["params"]}})
    elif method == "shutdown":
        send({{"jsonrpc":"2.0","id":message["id"],"result":None}})
    elif method == "exit":
        break
"#,
        kind = kind,
        new_text = new_text
    )
}
