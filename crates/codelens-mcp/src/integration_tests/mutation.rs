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
