use super::*;

fn assert_raw_fs_envelope(result: &serde_json::Value, expected_op: &str) {
    // The response text is formatted as { success, data: { ... payload fields ... }, ... }.
    // Envelope fields land inside result["data"] after the response pipeline.
    let data = &result["data"];
    assert_eq!(
        data["authority"], "syntax",
        "expected authority=syntax for {expected_op}, got {:?}",
        data["authority"]
    );
    assert_eq!(
        data["can_preview"], true,
        "expected can_preview=true for {expected_op}"
    );
    assert_eq!(
        data["can_apply"], true,
        "expected can_apply=true for {expected_op}"
    );
    let edit_authority = &data["edit_authority"];
    assert_eq!(
        edit_authority["kind"], "raw_fs",
        "expected edit_authority.kind=raw_fs for {expected_op}"
    );
    assert_eq!(
        edit_authority["operation"], expected_op,
        "expected edit_authority.operation={expected_op}"
    );
    assert!(
        edit_authority["validator"].is_null(),
        "expected edit_authority.validator=null for {expected_op}"
    );
}

// Line-edit family tombstoned (#346); the raw_fs envelope contract is
// carried by the remaining symbolic edit core.

/// Phase 3 pin (#346, plan Test 3.2): calling a tombstoned name through
/// the full dispatch path returns the replacement guidance, not a bare
/// unknown-tool error (and certainly not a panic).
#[test]
fn tombstoned_tool_call_returns_replacement_guidance() {
    let project = project_root();
    let state = make_state(&project);
    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "insert_at_line",
                "arguments": {"relative_path": "x.py", "line": 1, "content": "y"}
            })),
        },
    )
    .expect("tools/call returns a JSON-RPC response");
    let value = serde_json::to_value(&response).expect("serialize");
    assert_eq!(
        value["error"]["code"],
        json!(-32601),
        "tombstoned names keep the unknown-tool JSON-RPC code"
    );
    let err = value["error"]["message"].as_str().unwrap_or("");
    assert!(
        err.contains("insert_at_line") && err.contains("#346") && err.contains("Edit"),
        "tombstone guidance must name the removed tool and replacement path, got {err:?}"
    );
}

#[test]
fn insert_after_symbol_reports_raw_fs_envelope() {
    let project = project_root();
    let state = make_state(&project);
    fs::write(
        project.as_path().join("envelope_insert_sym.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let result = call_tool(
        &state,
        "insert_after_symbol",
        json!({
            "relative_path": "envelope_insert_sym.py",
            "symbol_name": "alpha",
            "content": "\ndef beta():\n    return 2\n"
        }),
    );
    assert_raw_fs_envelope(&result, "insert_after_symbol");
}

#[test]
fn unset_backend_apply_attempt_returns_validation_error() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("rename_unset.py");
    fs::write(&path, "def alpha():\n    return 1\n").unwrap();

    let response = crate::server::router::handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "rename_symbol",
                "arguments": {
                    "_session_id": default_session_id(&state),
                    "file_path": "rename_unset.py",
                    "symbol_name": "alpha",
                    "new_name": "beta",
                    "dry_run": false
                }
            })),
        },
    )
    .expect("tools/call should return a response");

    let value = serde_json::to_value(&response).expect("serialize");
    let text = value["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.contains("preview-only") || text.contains("Validation"),
        "expected validation error when backend unset and dry_run=false, got: {text}"
    );
}
