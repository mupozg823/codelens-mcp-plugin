use super::*;

async fn initialize_deferred_builder_session(app: &axum::Router) -> String {
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"},"profile":"builder-minimal","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    init.headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .expect("initialize must return an MCP session id")
        .to_owned()
}

#[tokio::test]
async fn deferred_builder_initial_listing_includes_active_default_ranked_tools() {
    // Given a fresh deferred builder session with no namespace or tier expansion.
    let state = test_state();
    let app = build_router(state);
    let sid = initialize_deferred_builder_session(&app).await;

    // When the client requests the initial tool listing.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Then every active default-ranked readiness entrypoint is advertised.
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response).await;
    let envelope: serde_json::Value = serde_json::from_str(&body).unwrap();
    let tool_names = envelope["result"]["tools"]
        .as_array()
        .expect("tools/list result")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(
        tool_names.contains(&"verify_change_readiness"),
        "default-ranked active-surface tool must be listed before expansion: {tool_names:?}"
    );
}

#[tokio::test]
async fn prepare_harness_reports_active_default_ranked_tools_as_visible() {
    // Given a fresh deferred builder session and two default-ranked entrypoints.
    let state = test_state();
    let app = build_router(state.clone());
    let sid = initialize_deferred_builder_session(&app).await;

    // When bootstrap checks their visibility before any explicit expansion.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", sid)
                .body(axum::body::Body::from(
                    json!({
                        "jsonrpc": "2.0",
                        "id": 2,
                        "method": "tools/call",
                        "params": {
                            "name": "prepare_harness_session",
                            "arguments": {
                                "project": state.project().as_path(),
                                "profile": "builder-minimal",
                                "detail": "full",
                                "preferred_entrypoints": [
                                    "diagnose",
                                    "verify_change_readiness"
                                ]
                            }
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Then listing metadata agrees with the call gate for both entrypoints.
    assert_eq!(response.status(), StatusCode::OK);
    let payload = first_tool_payload(&body_string(response).await);
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["routing"]["preferred_entrypoints_visible"],
        json!(["diagnose", "verify_change_readiness"])
    );
    assert!(
        payload["data"]["routing"]["preferred_entrypoints_omitted"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "visible default-ranked tools must not emit deferred-loading omissions"
    );
}
