use super::*;

#[test]
fn onboard_project_returns_structure() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).unwrap();
    fs::write(
        project.as_path().join("src/main.py"),
        "class App:\n    def run(self):\n        pass\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "onboard_project", json!({}));
    assert_eq!(payload["success"], json!(true));
    assert!(payload["data"]["directory_structure"].is_array());
    assert!(payload["data"]["key_files"].is_array());
    assert!(payload["data"]["semantic"].get("status").is_some());
}

#[cfg(feature = "semantic")]
#[test]
fn onboard_project_uses_existing_embedding_index_without_loading_engine() {
    if !embedding_model_available_for_test() {
        return;
    }
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).unwrap();
    fs::write(
        project.as_path().join("src/main.py"),
        "class App:\n    def run(self):\n        return 'ok'\n",
    )
    .unwrap();
    let _bootstrap = make_state(&project);

    let engine = codelens_engine::EmbeddingEngine::new(&project).unwrap();
    let indexed = engine.index_from_project(&project).unwrap();
    assert!(indexed > 0);
    drop(engine);

    let state = make_state(&project);

    let payload = call_tool(&state, "onboard_project", json!({}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["semantic"]["status"], json!("ready"));
    assert_eq!(
        payload["data"]["semantic"]["model"],
        json!("MiniLM-L12-CodeSearchNet-INT8")
    );
    assert_eq!(
        payload["data"]["semantic"]["indexed_symbols"],
        json!(indexed)
    );
    assert_eq!(payload["data"]["semantic"]["loaded"], json!(false));
}

#[test]
fn onboard_project_schema_matches_payload_shape() {
    let schema = crate::tool_defs::tool_definition("onboard_project")
        .and_then(|tool| tool.output_schema.as_ref())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let properties = schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    assert!(properties.contains_key("project_root"));
    assert!(properties.contains_key("suggested_next_tools"));
}

#[test]
fn explore_codebase_without_query_delegates_to_onboard() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).unwrap();
    fs::write(project.as_path().join("src/main.py"), "def hello(): pass\n").unwrap();
    let state = make_state(&project);
    let _payload = call_tool(&state, "explore_codebase", json!({}));
    let payload = call_tool(&state, "explore_codebase", json!({}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["workflow"], json!("explore_codebase"));
    assert_eq!(payload["data"]["delegated_tool"], json!("onboard_project"));
}

#[test]
fn review_architecture_without_path_delegates_to_onboard() {
    let project = project_root();
    let state = make_state(&project);
    let payload = call_tool(&state, "review_architecture", json!({}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["workflow"], json!("review_architecture"));
    assert_eq!(payload["data"]["delegated_tool"], json!("onboard_project"));
}
