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
fn create_text_file_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    let result = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "envelope_create.txt", "content": "x\n"}),
    );
    assert_raw_fs_envelope(&result, "create_text_file");
}

#[test]
fn delete_lines_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    seed_lines(&project, "envelope_delete.txt");
    let result = call_tool(
        &state,
        "delete_lines",
        json!({"relative_path": "envelope_delete.txt", "start_line": 1, "end_line": 1}),
    );
    assert_raw_fs_envelope(&result, "delete_lines");
}

#[test]
fn insert_at_line_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    seed_lines(&project, "envelope_insert_line.txt");
    let result = call_tool(
        &state,
        "insert_at_line",
        json!({"relative_path": "envelope_insert_line.txt", "line": 1, "content": "new\n"}),
    );
    assert_raw_fs_envelope(&result, "insert_at_line");
}

#[test]
fn insert_before_symbol_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("envelope_before.py");
    fs::write(&path, "def alpha():\n    pass\n").unwrap();
    let result = call_tool(
        &state,
        "insert_before_symbol",
        json!({
            "relative_path": "envelope_before.py",
            "symbol_name": "alpha",
            "content": "# leading\n"
        }),
    );
    assert_raw_fs_envelope(&result, "insert_before_symbol");
}

#[test]
fn insert_after_symbol_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("envelope_after.py");
    fs::write(&path, "def alpha():\n    pass\n").unwrap();
    let result = call_tool(
        &state,
        "insert_after_symbol",
        json!({
            "relative_path": "envelope_after.py",
            "symbol_name": "alpha",
            "content": "# trailing\n"
        }),
    );
    assert_raw_fs_envelope(&result, "insert_after_symbol");
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
fn replace_lines_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    seed_lines(&project, "envelope_replace_lines.txt");
    let result = call_tool(
        &state,
        "replace_lines",
        json!({
            "relative_path": "envelope_replace_lines.txt",
            "start_line": 2,
            "end_line": 2,
            "new_content": "BETA\n"
        }),
    );
    assert_raw_fs_envelope(&result, "replace_lines");
}

#[test]
fn replace_content_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    seed_lines(&project, "envelope_replace_content.txt");
    let result = call_tool(
        &state,
        "replace_content",
        json!({
            "relative_path": "envelope_replace_content.txt",
            "old_text": "alpha",
            "new_text": "ALPHA"
        }),
    );
    assert_raw_fs_envelope(&result, "replace_content");
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

#[test]
fn replace_symbol_body_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("envelope_replace_symbol.py");
    fs::write(&path, "def alpha():\n    return 1\n").unwrap();
    let result = call_tool(
        &state,
        "replace_symbol_body",
        json!({
            "relative_path": "envelope_replace_symbol.py",
            "symbol_name": "alpha",
            "new_body": "    return 2\n"
        }),
    );
    assert_raw_fs_envelope(&result, "replace_symbol_body");
}

#[test]
fn add_import_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("envelope_import.py");
    fs::write(&path, "def alpha():\n    return 1\n").unwrap();
    let result = call_tool(
        &state,
        "add_import",
        json!({
            "file_path": "envelope_import.py",
            "import_statement": "import os"
        }),
    );
    assert_raw_fs_envelope(&result, "add_import");
}

/// `confidence` and `backend_used` are top-level fields in the parsed text
/// payload (see `format_structured_response` in dispatch/response_support.rs).
/// There is no `_meta` wrapper; they sit directly on the returned JSON object.
fn extract_confidence(result: &serde_json::Value) -> f64 {
    result["confidence"].as_f64().unwrap_or(1.0)
}

#[test]
fn create_text_file_filesystem_confidence_is_lowered() {
    let project = project_root();
    let state = make_state(&project);
    let result = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "conf_create.txt", "content": "x\n"}),
    );
    let confidence = extract_confidence(&result);
    assert!(
        confidence <= 0.7 + f64::EPSILON,
        "expected Filesystem confidence ≤ 0.7, got {confidence} (result={result})"
    );
    assert_eq!(result["backend_used"], "filesystem");
}

#[test]
fn delete_lines_filesystem_confidence_is_lowered() {
    let project = project_root();
    let state = make_state(&project);
    seed_lines(&project, "conf_delete.txt");
    let result = call_tool(
        &state,
        "delete_lines",
        json!({"relative_path": "conf_delete.txt", "start_line": 1, "end_line": 1}),
    );
    let confidence = extract_confidence(&result);
    assert!(
        confidence <= 0.7 + f64::EPSILON,
        "expected Filesystem confidence ≤ 0.7, got {confidence} (result={result})"
    );
    assert_eq!(result["backend_used"], "filesystem");
}

#[test]
fn tree_sitter_rename_apply_attempt_returns_validation_error() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("rename_apply.py");
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
                    "file_path": "rename_apply.py",
                    "symbol_name": "alpha",
                    "new_name": "beta",
                    "semantic_edit_backend": "tree-sitter",
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
        "expected validation error mentioning preview-only, got: {text}"
    );
}

#[test]
fn tree_sitter_rename_dry_run_advertises_preview_only() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("rename_dry.py");
    fs::write(&path, "def alpha():\n    return 1\n").unwrap();

    let result = call_tool(
        &state,
        "rename_symbol",
        json!({
            "file_path": "rename_dry.py",
            "symbol_name": "alpha",
            "new_name": "beta",
            "semantic_edit_backend": "tree-sitter",
            "dry_run": true
        }),
    );
    let scope = result.get("data").unwrap_or(&result);
    assert_eq!(scope["authority"], "syntax", "got: {result}");
    assert_eq!(scope["can_preview"], true);
    assert_eq!(scope["can_apply"], false);
    assert_eq!(scope["support"], "syntax_preview");
    assert!(
        scope["blocker_reason"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "expected non-empty blocker_reason, got: {result}"
    );
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
