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

fn seed_lines(project: &codelens_engine::ProjectRoot, name: &str) -> std::path::PathBuf {
    let path = project.as_path().join(name);
    fs::write(&path, "alpha\nbeta\ngamma\ndelta\n").unwrap();
    path
}
#[test]
fn insert_content_default_dispatches_to_line_envelope() {
    let project = project_root();
    let state = make_state(&project);
    seed_lines(&project, "envelope_insert_content.txt");
    let result = call_tool(
        &state,
        "insert_content",
        json!({"relative_path": "envelope_insert_content.txt", "line": 1, "content": "new\n"}),
    );
    assert_raw_fs_envelope(&result, "insert_at_line");
}
#[test]
fn replace_content_unified_default_dispatches_to_text() {
    let project = project_root();
    let state = make_state(&project);
    seed_lines(&project, "envelope_replace_unified.txt");
    let result = call_tool(
        &state,
        "replace_content",
        json!({
            "relative_path": "envelope_replace_unified.txt",
            "old_text": "alpha",
            "new_text": "ALPHA"
        }),
    );
    assert_raw_fs_envelope(&result, "replace_content");
}
/// `confidence` and `backend_used` are top-level fields in the parsed text
/// payload (see `format_structured_response` in dispatch/response_support.rs).
/// There is no `_meta` wrapper; they sit directly on the returned JSON object.
fn extract_confidence(result: &serde_json::Value) -> f64 {
    result["confidence"].as_f64().unwrap_or(1.0)
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
