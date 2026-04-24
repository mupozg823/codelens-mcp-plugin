use super::*;

#[test]
fn registry_resources_report_projects_and_memory_scopes() {
    let project = project_root();
    let state = make_state(&project);

    let list_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3001)),
            method: "resources/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let list_body = serde_json::to_string(&list_response).unwrap();
    assert!(list_body.contains("codelens://registry/projects"));
    assert!(list_body.contains("codelens://registry/memory-scopes"));
    assert!(list_body.contains("symbiote://registry/projects"));

    let projects_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3002)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://registry/projects"})),
        },
    )
    .unwrap();
    let projects_body = serde_json::to_string(&projects_response).unwrap();
    assert!(projects_body.contains("is_active"));
    assert!(projects_body.contains("has_project_memory"));
    assert!(projects_body.contains("count_active"));

    let scopes_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3003)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://registry/memory-scopes"})),
        },
    )
    .unwrap();
    let scopes_body = serde_json::to_string(&scopes_response).unwrap();
    assert!(scopes_body.contains("\\\"scope\\\": \\\"project\\\""));
    assert!(scopes_body.contains("\\\"scope\\\": \\\"global\\\""));
    assert!(scopes_body.contains("mutation_wired"));
    assert!(scopes_body.contains("Passive scaffold"));
}

#[test]
fn queryable_project_tools_mutate_registry_without_switching_active_project() {
    let active = project_root();
    let secondary = project_root();
    fs::create_dir_all(secondary.as_path().join("src")).unwrap();
    fs::write(
        secondary.as_path().join("src/lib.rs"),
        "pub fn external_entrypoint() {}\n",
    )
    .unwrap();
    let state = make_state(&active);
    let active_path = state.project().as_path().to_string_lossy().to_string();

    let add = call_tool(
        &state,
        "add_queryable_project",
        json!({"path": secondary.as_path().to_string_lossy()}),
    );
    assert_eq!(add["success"], json!(true));
    assert_eq!(add["data"]["added"], json!(true));
    let project_name = add["data"]["name"].as_str().expect("project name");

    let list = call_tool(&state, "list_queryable_projects", json!({}));
    assert_eq!(list["success"], json!(true));
    assert!(list["data"]["count"].as_u64().unwrap_or_default() >= 2);
    assert_eq!(state.project().as_path().to_string_lossy(), active_path);

    let query = call_tool(
        &state,
        "query_project",
        json!({
            "project_name": project_name,
            "symbol_name": "external_entrypoint",
            "max_results": 5
        }),
    );
    assert_eq!(query["success"], json!(true));

    let remove = call_tool(
        &state,
        "remove_queryable_project",
        json!({"name": project_name}),
    );
    assert_eq!(remove["success"], json!(true));
    assert_eq!(remove["data"]["removed"], json!(true));
    assert_eq!(state.project().as_path().to_string_lossy(), active_path);
}
