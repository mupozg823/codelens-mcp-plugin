use super::*;

// ── Mutation tool tests ──────────────────────────────────────────────

#[test]
fn create_text_file_creates_file() {
    let project = project_root();
    let state = make_state(&project);
    let result = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "new_file.txt", "content": "line1\nline2\n"}),
    );
    assert!(result["success"].as_bool().unwrap_or(false));
    let content = fs::read_to_string(project.as_path().join("new_file.txt")).unwrap();
    assert_eq!(content, "line1\nline2\n");
}

#[test]
fn delete_lines_removes_range() {
    let project = project_root();
    let state = make_state(&project);
    fs::write(
        project.as_path().join("lines.txt"),
        "line1\nline2\nline3\nline4\nline5\n",
    )
    .unwrap();
    let result = call_tool(
        &state,
        "delete_lines",
        json!({"relative_path": "lines.txt", "start_line": 2, "end_line": 4}),
    );
    assert!(result["success"].as_bool().unwrap_or(false));
    let content = fs::read_to_string(project.as_path().join("lines.txt")).unwrap();
    assert!(content.contains("line1"));
    assert!(content.contains("line5"));
    assert!(!content.contains("line2"));
    assert!(!content.contains("line3"));
}

#[test]
fn replace_lines_substitutes_range() {
    let project = project_root();
    let state = make_state(&project);
    fs::write(
        project.as_path().join("replace.txt"),
        "aaa\nbbb\nccc\nddd\n",
    )
    .unwrap();
    let result = call_tool(
        &state,
        "replace_lines",
        json!({"relative_path": "replace.txt", "start_line": 2, "end_line": 3, "new_content": "XXX\nYYY\n"}),
    );
    assert!(result["success"].as_bool().unwrap_or(false));
    let content = fs::read_to_string(project.as_path().join("replace.txt")).unwrap();
    assert!(content.contains("aaa"));
    assert!(content.contains("XXX"));
    assert!(!content.contains("bbb"));
}

#[test]
fn critical_mutation_tools_emit_structured_content() {
    let project = project_root();
    let state = make_state(&project);

    fs::write(
        project.as_path().join("rename_structured.py"),
        "def old_name():\n    return 1\n\nold_name()\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("imports_structured.py"),
        "import os\n\nvalue = 1\n",
    )
    .unwrap();

    let create_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(5101)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "create_text_file",
                "arguments": {
                    "relative_path": "created_structured.txt",
                    "content": "hello\n"
                }
            })),
        },
    )
    .expect("create_text_file response");
    let create_value = serde_json::to_value(&create_response).unwrap();
    assert_eq!(
        create_value["result"]["structuredContent"]["created"],
        json!("created_structured.txt")
    );

    let add_import_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(5102)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "add_import",
                "arguments": {
                    "file_path": "imports_structured.py",
                    "import_statement": "import sys"
                }
            })),
        },
    )
    .expect("add_import response");
    let add_import_value = serde_json::to_value(&add_import_response).unwrap();
    assert_eq!(
        add_import_value["result"]["structuredContent"]["success"],
        json!(true)
    );
    assert_eq!(
        add_import_value["result"]["structuredContent"]["file_path"],
        json!("imports_structured.py")
    );
    assert!(add_import_value["result"]["structuredContent"]["content_length"].is_number());

    let rename_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(5103)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "rename_symbol",
                "arguments": {
                    "file_path": "rename_structured.py",
                    "symbol_name": "old_name",
                    "new_name": "new_name",
                    "scope": "file",
                    "dry_run": true
                }
            })),
        },
    )
    .expect("rename_symbol response");
    let rename_value = serde_json::to_value(&rename_response).unwrap();
    assert_eq!(
        rename_value["result"]["structuredContent"]["success"],
        json!(true)
    );
    assert!(rename_value["result"]["structuredContent"]["modified_files"].is_number());
    assert!(rename_value["result"]["structuredContent"]["total_replacements"].is_number());
    assert!(rename_value["result"]["structuredContent"]["edits"].is_array());
}

#[test]
fn mutation_text_and_structured_content_stay_in_sync() {
    let project = project_root();
    let state = make_state(&project);

    fs::write(
        project.as_path().join("rename_parity.py"),
        "def old_name():\n    return 1\n\nold_name()\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("imports_parity.py"),
        "import os\n\nvalue = 1\n",
    )
    .unwrap();

    let create_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(5104)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "create_text_file",
                "arguments": {
                    "relative_path": "created_parity.txt",
                    "content": "hello\n"
                }
            })),
        },
    )
    .expect("create_text_file response");
    let create_structured = serde_json::to_value(&create_response).unwrap();
    let create_text = parse_tool_payload(&extract_tool_text(&create_response));
    assert_eq!(
        create_structured["result"]["structuredContent"]["created"],
        create_text["data"]["created"]
    );

    let add_import_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(5105)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "add_import",
                "arguments": {
                    "file_path": "imports_parity.py",
                    "import_statement": "import sys"
                }
            })),
        },
    )
    .expect("add_import response");
    let add_import_structured = serde_json::to_value(&add_import_response).unwrap();
    let add_import_text = parse_tool_payload(&extract_tool_text(&add_import_response));
    assert_eq!(
        add_import_structured["result"]["structuredContent"]["success"],
        add_import_text["data"]["success"]
    );
    assert_eq!(
        add_import_structured["result"]["structuredContent"]["file_path"],
        add_import_text["data"]["file_path"]
    );

    let rename_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(5106)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "rename_symbol",
                "arguments": {
                    "file_path": "rename_parity.py",
                    "symbol_name": "old_name",
                    "new_name": "new_name",
                    "scope": "file",
                    "dry_run": true
                }
            })),
        },
    )
    .expect("rename_symbol response");
    let rename_structured = serde_json::to_value(&rename_response).unwrap();
    let rename_text = parse_tool_payload(&extract_tool_text(&rename_response));
    assert_eq!(
        rename_structured["result"]["structuredContent"]["success"],
        rename_text["data"]["success"]
    );
    assert_eq!(
        rename_structured["result"]["structuredContent"]["modified_files"],
        rename_text["data"]["modified_files"]
    );
    assert_eq!(
        rename_structured["result"]["structuredContent"]["total_replacements"],
        rename_text["data"]["total_replacements"]
    );
}
