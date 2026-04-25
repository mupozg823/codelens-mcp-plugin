use super::*;

#[tokio::test]
async fn analysis_jobs_follow_session_bound_project_scope() {
    let project_a = temp_project_dir("analysis-a");
    let project_b = temp_project_dir("analysis-b");
    std::fs::write(
        project_a.join("first.py"),
        "def first_only():\n    return 1\n",
    )
    .unwrap();
    std::fs::write(
        project_b.join("second.py"),
        "def second_only():\n    return 2\n",
    )
    .unwrap();

    let project = ProjectRoot::new(project_a.to_str().unwrap()).unwrap();
    let state = Arc::new(
        AppState::new(project, crate::tool_defs::ToolPreset::Balanced).with_session_store(),
    );
    let app = build_router(state);

    let init_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid_a = init_a
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let init_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid_b = init_b
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let activate_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"activate_project","arguments":{{"project":"{}"}}}}}}"#,
                    project_b.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(activate_b.status(), StatusCode::OK);

    let set_profile_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"set_profile","arguments":{"profile":"reviewer-graph"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(set_profile_b.status(), StatusCode::OK);

    let start = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"start_analysis_job","arguments":{"kind":"impact_report","path":"second.py","profile_hint":"reviewer-graph"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start.status(), StatusCode::OK);
    let start_payload = first_tool_payload(&body_string(start).await);
    let job_id = start_payload["data"]["job_id"]
        .as_str()
        .expect("job id")
        .to_owned();
    assert!(start_payload["data"]["summary_resource"].is_null());
    assert_eq!(
        start_payload["data"]["section_handles"],
        serde_json::json!([])
    );

    // Poll schedule: fewer calls, longer sleeps to keep us well under the
    // 300-calls/minute per-session rate limit. Total wall budget: ~30 s
    // (200 polls x 150 ms), which is plenty for impact_report on a
    // 2-file tempdir project even on congested CI.
    let mut analysis_id = None;
    let mut last_poll_payload = None;
    for _ in 0..200 {
        let poll = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .header("mcp-session-id", &sid_b)
                    .body(axum::body::Body::from(format!(
                        r#"{{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{{"name":"get_analysis_job","arguments":{{"job_id":"{}"}}}}}}"#,
                        job_id
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        let poll_payload = first_tool_payload(&body_string(poll).await);
        last_poll_payload = Some(poll_payload.clone());
        if let Some(id) = poll_payload["data"]["analysis_id"].as_str() {
            assert!(
                poll_payload["data"]["summary_resource"]["uri"]
                    .as_str()
                    .map(|uri| uri.ends_with("/summary"))
                    .unwrap_or(false)
            );
            assert!(
                poll_payload["data"]["section_handles"]
                    .as_array()
                    .map(|items| !items.is_empty())
                    .unwrap_or(false)
            );
            analysis_id = Some(id.to_owned());
            break;
        }
        if matches!(
            poll_payload["data"]["status"].as_str(),
            Some("error") | Some("cancelled")
        ) {
            panic!("analysis job did not complete successfully: {poll_payload}");
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    let analysis_id = analysis_id.unwrap_or_else(|| {
        panic!(
            "analysis_id after completion; last poll payload: {}",
            last_poll_payload.unwrap_or_default()
        )
    });
    let section = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{{"name":"get_analysis_section","arguments":{{"analysis_id":"{}","section":"impact_rows"}}}}}}"#,
                    analysis_id
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let section_payload = first_tool_payload(&body_string(section).await);
    assert_eq!(section_payload["success"], serde_json::json!(true));
    assert!(
        section_payload["data"]["content"]
            .to_string()
            .contains("second.py")
    );

    let foreign_poll = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_a)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{{"name":"get_analysis_job","arguments":{{"job_id":"{}"}}}}}}"#,
                    job_id
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let foreign_body = body_string(foreign_poll).await;
    assert!(foreign_body.contains("unknown job_id"));
}
