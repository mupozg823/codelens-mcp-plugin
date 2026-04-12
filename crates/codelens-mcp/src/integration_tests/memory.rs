use super::*;

// ── Memory tool tests ────────────────────────────────────────────────

#[test]
fn write_and_read_memory() {
    let project = project_root();
    let state = make_state(&project);
    call_tool(
        &state,
        "write_memory",
        json!({"memory_name": "test_note", "content": "hello from test"}),
    );
    let result = call_tool(&state, "read_memory", json!({"memory_name": "test_note"}));
    assert_eq!(
        result["data"]["content"].as_str().unwrap(),
        "hello from test"
    );
}

#[test]
fn delete_memory_removes_file() {
    let project = project_root();
    let state = make_state(&project);
    call_tool(
        &state,
        "write_memory",
        json!({"memory_name": "to_delete", "content": "temp"}),
    );
    let result = call_tool(&state, "delete_memory", json!({"memory_name": "to_delete"}));
    assert_eq!(result["data"]["status"].as_str().unwrap(), "ok");
}

#[test]
fn list_memories_returns_written() {
    let project = project_root();
    let state = make_state(&project);
    call_tool(
        &state,
        "write_memory",
        json!({"memory_name": "alpha", "content": "a"}),
    );
    call_tool(
        &state,
        "write_memory",
        json!({"memory_name": "beta", "content": "b"}),
    );
    let result = call_tool(&state, "list_memories", json!({}));
    let count = result["data"]["count"].as_u64().unwrap_or(0);
    assert!(count >= 2, "expected at least 2 memories, got {count}");
}

#[test]
fn rename_memory_moves_file() {
    let project = project_root();
    let state = make_state(&project);
    call_tool(
        &state,
        "write_memory",
        json!({"memory_name": "old_name", "content": "data"}),
    );
    call_tool(
        &state,
        "rename_memory",
        json!({"old_name": "old_name", "new_name": "new_name"}),
    );
    let result = call_tool(&state, "read_memory", json!({"memory_name": "new_name"}));
    assert_eq!(result["data"]["content"].as_str().unwrap(), "data");
}

#[test]
fn memory_path_traversal_rejected() {
    let project = project_root();
    let state = make_state(&project);
    let result = call_tool(
        &state,
        "write_memory",
        json!({"memory_name": "../escape", "content": "bad"}),
    );
    assert!(
        result["success"].as_bool() == Some(false),
        "path traversal should be rejected"
    );
}
