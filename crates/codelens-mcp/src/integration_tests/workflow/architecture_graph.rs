use super::*;

#[test]
fn review_architecture_workspace_root_renders_module_graph() {
    let project = project_root();
    fs::write(
        project.as_path().join("Cargo.toml"),
        r#"[workspace]
members = ["crates/engine", "crates/mcp"]
"#,
    )
    .unwrap();
    let engine_src = project.as_path().join("crates/engine/src");
    let mcp_src = project.as_path().join("crates/mcp/src");
    fs::create_dir_all(&engine_src).unwrap();
    fs::create_dir_all(&mcp_src).unwrap();
    fs::write(engine_src.join("lib.rs"), "pub struct ProjectGraph;\n").unwrap();
    fs::write(
        mcp_src.join("lib.rs"),
        "use engine::ProjectGraph;\n\npub fn expose(_: ProjectGraph) {}\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "review_architecture",
        json!({
            "path": project.as_path().to_string_lossy(),
            "include_diagram": true,
            "max_nodes": 20
        }),
    );
    assert_eq!(payload["success"], json!(true));
    let analysis_id = payload["data"]["analysis_id"]
        .as_str()
        .expect("analysis_id should be present");

    let stats = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "stats"}),
    );
    assert_eq!(stats["success"], json!(true));
    assert_eq!(
        stats["data"]["content"]["granularity"],
        json!("workspace_modules")
    );
    assert_eq!(stats["data"]["content"]["workspace_member_count"], json!(2));
    assert!(
        stats["data"]["content"]["module_count"]
            .as_u64()
            .unwrap_or_default()
            >= 2,
        "workspace root architecture review should expose module nodes: {stats:?}"
    );

    let diagram = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "diagram"}),
    );
    assert_eq!(diagram["success"], json!(true));
    let content = diagram["data"]["content"]["content"]
        .as_str()
        .expect("diagram content should be a mermaid string");
    assert!(content.contains("crates/engine"));
    assert!(content.contains("crates/mcp"));
    assert!(
        !content.contains("target0["),
        "workspace graph should not collapse the root into one target file node: {content}"
    );
}
