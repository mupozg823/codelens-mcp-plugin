use super::*;

static ADAPTER_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

#[test]
fn ide_adapters_fail_closed_until_configured() {
    let _lock = ADAPTER_ENV_LOCK.lock().unwrap();
    let project = project_root();
    fs::write(
        project.as_path().join("target.ts"),
        "const value = 1;\nexport {};\n",
    )
    .unwrap();
    let state = make_state(&project);
    let previous_jetbrains = std::env::var("CODELENS_JETBRAINS_ADAPTER_CMD").ok();
    let previous_roslyn = std::env::var("CODELENS_ROSLYN_ADAPTER_CMD").ok();
    unsafe {
        std::env::remove_var("CODELENS_JETBRAINS_ADAPTER_CMD");
        std::env::remove_var("CODELENS_ROSLYN_ADAPTER_CMD");
    }

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

    unsafe {
        match previous_jetbrains {
            Some(value) => std::env::set_var("CODELENS_JETBRAINS_ADAPTER_CMD", value),
            None => std::env::remove_var("CODELENS_JETBRAINS_ADAPTER_CMD"),
        }
        match previous_roslyn {
            Some(value) => std::env::set_var("CODELENS_ROSLYN_ADAPTER_CMD", value),
            None => std::env::remove_var("CODELENS_ROSLYN_ADAPTER_CMD"),
        }
    }
}

#[test]
fn jetbrains_adapter_applies_workspace_edit_when_configured() {
    let _lock = ADAPTER_ENV_LOCK.lock().unwrap();
    let project = project_root();
    let original = "const value = 1;\nexport {};\n";
    fs::write(project.as_path().join("target.ts"), original).unwrap();
    let original_hash = sha256_hex_text(original);
    let adapter = write_semantic_adapter_mock(&project, "adapterInline();");
    let state = make_state(&project);

    let previous = std::env::var("CODELENS_JETBRAINS_ADAPTER_CMD").ok();
    unsafe {
        std::env::set_var("CODELENS_JETBRAINS_ADAPTER_CMD", &adapter);
    }

    let payload = call_tool(
        &state,
        "refactor_inline_function",
        json!({
            "file_path": "target.ts",
            "function_name": "value",
            "line": 1,
            "column": 7,
            "semantic_edit_backend": "jetbrains",
            "dry_run": false
        }),
    );

    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_JETBRAINS_ADAPTER_CMD", value),
            None => std::env::remove_var("CODELENS_JETBRAINS_ADAPTER_CMD"),
        }
    }

    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["semantic_edit_backend"], json!("jetbrains"));
    assert_eq!(payload["data"]["authority"], json!("workspace_edit"));
    assert_eq!(
        payload["data"]["authority_backend"],
        json!("ide-adapter:jetbrains")
    );
    assert_eq!(payload["data"]["support"], json!("authoritative_apply"));
    assert_eq!(payload["data"]["can_preview"], json!(true));
    assert_eq!(payload["data"]["can_apply"], json!(true));
    assert_eq!(payload["data"]["blocker_reason"], json!(null));
    assert_eq!(
        payload["data"]["transaction"]["contract"]["file_hashes_before"]["target.ts"]["sha256"],
        json!(original_hash),
        "{payload}"
    );
    assert_eq!(
        fs::read_to_string(project.as_path().join("target.ts")).unwrap(),
        "adapterInline();\nexport {};\n"
    );
}

#[test]
fn ide_adapter_rejects_opaque_command_without_workspace_edit() {
    let _lock = ADAPTER_ENV_LOCK.lock().unwrap();
    let project = project_root();
    fs::write(
        project.as_path().join("target.ts"),
        "const value = 1;\nexport {};\n",
    )
    .unwrap();
    let adapter = write_opaque_semantic_adapter_mock(&project);
    let state = make_state(&project);

    let previous = std::env::var("CODELENS_JETBRAINS_ADAPTER_CMD").ok();
    unsafe {
        std::env::set_var("CODELENS_JETBRAINS_ADAPTER_CMD", &adapter);
    }

    let payload = call_tool(
        &state,
        "refactor_inline_function",
        json!({
            "file_path": "target.ts",
            "function_name": "value",
            "line": 1,
            "column": 7,
            "semantic_edit_backend": "jetbrains",
            "dry_run": false
        }),
    );

    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_JETBRAINS_ADAPTER_CMD", value),
            None => std::env::remove_var("CODELENS_JETBRAINS_ADAPTER_CMD"),
        }
    }

    assert_eq!(payload["success"], json!(false), "{payload}");
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("returned no inspectable WorkspaceEdit"),
        "{payload}"
    );
}

#[test]
fn roslyn_adapter_rename_uses_workspace_edit_when_configured() {
    let _lock = ADAPTER_ENV_LOCK.lock().unwrap();
    let project = project_root();
    let original = "class OldName {}\nclass Consumer {}\n";
    fs::write(project.as_path().join("target.cs"), original).unwrap();
    let original_hash = sha256_hex_text(original);
    let adapter = write_semantic_adapter_mock(&project, "class NewName {}");
    let state = make_state(&project);

    let previous = std::env::var("CODELENS_ROSLYN_ADAPTER_CMD").ok();
    unsafe {
        std::env::set_var("CODELENS_ROSLYN_ADAPTER_CMD", &adapter);
    }

    let payload = call_tool(
        &state,
        "rename_symbol",
        json!({
            "file_path": "target.cs",
            "symbol_name": "OldName",
            "new_name": "NewName",
            "semantic_edit_backend": "roslyn",
            "dry_run": false
        }),
    );

    unsafe {
        match previous {
            Some(value) => std::env::set_var("CODELENS_ROSLYN_ADAPTER_CMD", value),
            None => std::env::remove_var("CODELENS_ROSLYN_ADAPTER_CMD"),
        }
    }

    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(payload["data"]["semantic_edit_backend"], json!("roslyn"));
    assert_eq!(payload["data"]["authority"], json!("workspace_edit"));
    assert_eq!(
        payload["data"]["authority_backend"],
        json!("roslyn-sidecar")
    );
    assert_eq!(payload["data"]["support"], json!("authoritative_apply"));
    assert_eq!(payload["data"]["can_preview"], json!(true));
    assert_eq!(payload["data"]["can_apply"], json!(true));
    assert_eq!(payload["data"]["blocker_reason"], json!(null));
    assert_eq!(
        payload["data"]["transaction"]["contract"]["file_hashes_before"]["target.cs"]["sha256"],
        json!(original_hash),
        "{payload}"
    );
    assert_eq!(
        fs::read_to_string(project.as_path().join("target.cs")).unwrap(),
        "class NewName {}\nclass Consumer {}\n"
    );
}

#[test]
fn roslyn_adapter_discovers_sidecar_from_adapters_dir() {
    let _lock = ADAPTER_ENV_LOCK.lock().unwrap();
    let project = project_root();
    fs::write(
        project.as_path().join("target.cs"),
        "class OldName {}\nclass Consumer {}\n",
    )
    .unwrap();
    let adapters_dir = project.as_path().join("adapters");
    let sidecar_dir = adapters_dir.join("roslyn-workspace-service");
    fs::create_dir_all(&sidecar_dir).unwrap();
    let adapter = sidecar_dir.join(if cfg!(windows) {
        "codelens-roslyn-workspace-service.exe"
    } else {
        "codelens-roslyn-workspace-service"
    });
    fs::write(&adapter, semantic_adapter_mock("class SidecarName {}")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&adapter, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let state = make_state(&project);

    let previous_cmd = std::env::var("CODELENS_ROSLYN_ADAPTER_CMD").ok();
    let previous_dir = std::env::var("CODELENS_ADAPTERS_DIR").ok();
    unsafe {
        std::env::remove_var("CODELENS_ROSLYN_ADAPTER_CMD");
        std::env::set_var("CODELENS_ADAPTERS_DIR", &adapters_dir);
    }

    let payload = call_tool(
        &state,
        "rename_symbol",
        json!({
            "file_path": "target.cs",
            "symbol_name": "OldName",
            "new_name": "SidecarName",
            "semantic_edit_backend": "roslyn",
            "dry_run": false
        }),
    );

    unsafe {
        match previous_cmd {
            Some(value) => std::env::set_var("CODELENS_ROSLYN_ADAPTER_CMD", value),
            None => std::env::remove_var("CODELENS_ROSLYN_ADAPTER_CMD"),
        }
        match previous_dir {
            Some(value) => std::env::set_var("CODELENS_ADAPTERS_DIR", value),
            None => std::env::remove_var("CODELENS_ADAPTERS_DIR"),
        }
    }

    assert_eq!(payload["success"], json!(true), "{payload}");
    assert_eq!(
        fs::read_to_string(project.as_path().join("target.cs")).unwrap(),
        "class SidecarName {}\nclass Consumer {}\n"
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

fn write_semantic_adapter_mock(
    project: &codelens_engine::ProjectRoot,
    new_text: &str,
) -> std::path::PathBuf {
    let mock_path = project.as_path().join("mock_semantic_adapter.py");
    fs::write(&mock_path, semantic_adapter_mock(new_text)).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    mock_path
}

fn write_opaque_semantic_adapter_mock(
    project: &codelens_engine::ProjectRoot,
) -> std::path::PathBuf {
    let mock_path = project.as_path().join("mock_opaque_semantic_adapter.py");
    fs::write(
        &mock_path,
        r#"#!/usr/bin/env python3
import json

request = json.loads(open(0).read())
print(json.dumps({
    "success": True,
    "message": "opaque adapter command",
    "command": {"title": "Apply refactor inside IDE", "command": "ide.applyRefactor"}
}))
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    mock_path
}

fn semantic_adapter_mock(new_text: &str) -> String {
    format!(
        r#"#!/usr/bin/env python3
import json
import pathlib
import sys

NEW_TEXT = {new_text:?}
request = json.loads(sys.stdin.read())
project_root = pathlib.Path(request["project_root"])
file_path = request["arguments"]["file_path"]
uri = (project_root / file_path).resolve().as_uri()
print(json.dumps({{
    "success": True,
    "message": "mock adapter edit",
    "workspace_edit": {{
        "changes": {{
            uri: [{{
                "range": {{
                    "start": {{"line": 0, "character": 0}},
                    "end": {{"line": 0, "character": 16}}
                }},
                "newText": NEW_TEXT
            }}]
        }}
    }}
}}))
"#,
        new_text = new_text
    )
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
