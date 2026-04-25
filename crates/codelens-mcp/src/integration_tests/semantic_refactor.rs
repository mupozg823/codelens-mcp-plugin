use super::*;

#[test]
fn refactor_extract_function_applies_lsp_code_action_workspace_edit() {
    let project = project_root();
    fs::write(project.as_path().join("target.ts"), "const value = 1;\n").unwrap();
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
    assert_eq!(
        fs::read_to_string(project.as_path().join("target.ts")).unwrap(),
        "extracted();\n"
    );
}

#[test]
fn refactor_inline_function_applies_lsp_code_action_workspace_edit() {
    let project = project_root();
    fs::write(project.as_path().join("target.ts"), "const value = 1;\n").unwrap();
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
        "inlined();\n"
    );
}

#[test]
fn refactor_move_to_file_applies_lsp_code_action_workspace_edit() {
    let project = project_root();
    fs::write(project.as_path().join("target.ts"), "const value = 1;\n").unwrap();
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
        "moved();\n"
    );
}

#[test]
fn refactor_change_signature_applies_lsp_code_action_workspace_edit() {
    let project = project_root();
    fs::write(project.as_path().join("target.ts"), "const value = 1;\n").unwrap();
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
        "signatureChanged();\n"
    );
}

#[test]
fn jetbrains_and_roslyn_adapters_fail_closed_until_configured() {
    let project = project_root();
    fs::write(project.as_path().join("target.ts"), "const value = 1;\n").unwrap();
    let state = make_state(&project);

    for backend in ["jetbrains", "roslyn"] {
        let payload = call_tool(
            &state,
            "refactor_inline_function",
            json!({
                "file_path": "target.ts",
                "function_name": "value",
                "line": 1,
                "column": 7,
                "semantic_edit_backend": backend,
                "dry_run": false
            }),
        );
        assert_eq!(payload["success"], json!(false), "{payload}");
        assert!(
            payload["error"]
                .as_str()
                .unwrap_or_default()
                .contains("unsupported_semantic_refactor"),
            "{payload}"
        );
    }
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
