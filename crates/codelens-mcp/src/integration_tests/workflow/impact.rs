use super::*;

#[cfg(feature = "semantic")]
#[test]
fn impact_report_surfaces_unavailable_semantic_status() {
    let project = project_root();
    fs::write(
        project.as_path().join("impact_semantic_missing.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "impact_report",
        json!({"path": "impact_semantic_missing.py"}),
    );
    assert_eq!(payload["success"], json!(true));
    let analysis_id = payload["data"]["analysis_id"]
        .as_str()
        .expect("analysis_id should be present");
    assert!(
        payload["data"]["available_sections"]
            .as_array()
            .map(|sections| sections.iter().any(|section| section == "semantic_status"))
            .unwrap_or(false)
    );
    assert!(
        payload["data"]["next_actions"]
            .as_array()
            .map(|actions| {
                actions
                    .iter()
                    .filter_map(|value| value.as_str())
                    .any(|value| value.contains("index_embeddings"))
            })
            .unwrap_or(false)
    );

    let section = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "semantic_status"}),
    );
    assert_eq!(section["success"], json!(true));
    #[cfg(feature = "semantic")]
    let expected_status = "unavailable";
    #[cfg(not(feature = "semantic"))]
    let expected_status = "not_compiled";
    assert_eq!(section["data"]["content"]["status"], json!(expected_status));
    #[cfg(feature = "semantic")]
    let expected_reason_fragment = "index_embeddings";
    #[cfg(not(feature = "semantic"))]
    let expected_reason_fragment = "not compiled";
    assert!(
        section["data"]["content"]["reason"]
            .as_str()
            .unwrap_or("")
            .contains(expected_reason_fragment)
    );
}

#[cfg(feature = "semantic")]
#[test]
fn impact_report_uses_existing_embedding_index_for_semantic_status() {
    if !embedding_model_available_for_test() {
        return;
    }
    let project = project_root();
    fs::write(
        project.as_path().join("impact_semantic_ready.py"),
        "def ember_archive_delta():\n    return 1\n",
    )
    .unwrap();
    let _bootstrap = make_state(&project);

    let engine = codelens_engine::EmbeddingEngine::new(&project).unwrap();
    let indexed = engine.index_from_project(&project).unwrap();
    assert!(indexed > 0);
    drop(engine);

    let state = make_state(&project);
    assert!(state.embedding_ref().is_none());

    let payload = call_tool(
        &state,
        "impact_report",
        json!({"path": "impact_semantic_ready.py"}),
    );
    assert_eq!(payload["success"], json!(true));
    let analysis_id = payload["data"]["analysis_id"]
        .as_str()
        .expect("analysis_id should be present");

    let section = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "semantic_status"}),
    );
    assert_eq!(section["success"], json!(true));
    assert_eq!(section["data"]["content"]["status"], json!("ready"));
    assert_eq!(
        section["data"]["content"]["indexed_symbols"],
        json!(indexed)
    );
}
