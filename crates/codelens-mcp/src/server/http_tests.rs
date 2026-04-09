//! Integration tests for the Streamable HTTP transport.
//!
//! Uses tower::ServiceExt::oneshot to test axum handlers without starting a real server.
//! Run with: `cargo test --features http`

#![cfg(feature = "http")]

use super::session::SessionStore;
use super::transport_http::build_router;
use crate::AppState;
use axum::http::{Request, StatusCode};
use codelens_core::ProjectRoot;
use http_body_util::BodyExt;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

fn test_state() -> Arc<AppState> {
    let dir = std::env::temp_dir().join(format!(
        "codelens-http-test-{}-{:?}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::thread::current().id(),
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hello.txt"), "world\n").unwrap();
    let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
    let state = AppState::new(project, crate::tool_defs::ToolPreset::Balanced);
    Arc::new(state.with_session_store())
}

fn temp_project_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "codelens-http-{name}-{}-{:?}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::thread::current().id(),
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn first_resource_text(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("result").cloned())
        .and_then(|result| result.get("contents").cloned())
        .and_then(|contents| contents.as_array().cloned())
        .and_then(|contents| contents.first().cloned())
        .and_then(|content| content.get("text").cloned())
        .and_then(|text| text.as_str().map(ToOwned::to_owned))
        .unwrap_or_default()
}

fn first_tool_payload(body: &str) -> serde_json::Value {
    let value = serde_json::from_str::<serde_json::Value>(body).unwrap_or_default();
    let mut payload = value
        .get("result")
        .and_then(|result| result.get("content"))
        .and_then(|contents| contents.as_array())
        .and_then(|contents| contents.first())
        .and_then(|content| content.get("text"))
        .and_then(|text| text.as_str())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .unwrap_or_default();

    if let Some(structured_content) = value
        .get("result")
        .and_then(|result| result.get("structuredContent"))
        .cloned()
    {
        if !payload.is_object() {
            payload = serde_json::json!({});
        }
        payload
            .as_object_mut()
            .expect("payload object")
            .insert("data".to_owned(), structured_content);
    }

    payload
}

// ── POST /mcp ────────────────────────────────────────────────────────

#[tokio::test]
async fn post_initialize_returns_session_id() {
    let app = build_router(test_state());
    let resp = app
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

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers().get("mcp-session-id").is_some(),
        "initialize should return Mcp-Session-Id header"
    );
    let body = body_string(resp).await;
    assert!(body.contains("\"jsonrpc\":\"2.0\""));
    assert!(body.contains("\"id\":1"));
}

#[tokio::test]
async fn initialize_persists_client_metadata() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("x-codelens-trusted-client", "true")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA","version":"2.1.0"},"profile":"reviewer-graph"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();
    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    let metadata = session.client_metadata();
    assert_eq!(metadata.client_name.as_deref(), Some("HarnessQA"));
    assert_eq!(metadata.client_version.as_deref(), Some("2.1.0"));
    assert_eq!(
        metadata.requested_profile.as_deref(),
        Some("reviewer-graph")
    );
    assert_eq!(metadata.trusted_client, Some(true));
    assert_eq!(metadata.deferred_tool_loading, None);
    assert!(metadata.loaded_namespaces.is_empty());
    assert!(metadata.loaded_tiers.is_empty());
    assert_eq!(metadata.full_tool_exposure, None);
}

#[tokio::test]
async fn initialize_persists_deferred_loading_preference() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA","version":"2.1.0"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();
    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    let metadata = session.client_metadata();
    assert_eq!(metadata.deferred_tool_loading, Some(true));
    assert!(metadata.loaded_namespaces.is_empty());
    assert!(metadata.loaded_tiers.is_empty());
    assert_eq!(metadata.full_tool_exposure, None);
}

#[tokio::test]
async fn initialize_codex_defaults_to_deferred_loading() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();
    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    let metadata = session.client_metadata();
    assert_eq!(metadata.deferred_tool_loading, Some(true));
}

#[tokio::test]
async fn initialize_claude_defaults_to_full_contract() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"Claude Code","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();
    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    let metadata = session.client_metadata();
    assert_eq!(metadata.deferred_tool_loading, Some(false));
}

#[tokio::test]
async fn codex_session_client_name_affects_activate_project_budget() {
    let state = test_state();
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let primitive_list = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"tier":"primitive"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(primitive_list.status(), StatusCode::OK);

    let activate = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"activate_project","arguments":{}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(activate.status(), StatusCode::OK);
    let payload = first_tool_payload(&body_string(activate).await);
    assert_eq!(payload["success"], serde_json::json!(true));
    assert_eq!(
        payload["data"]["auto_surface"],
        serde_json::json!("builder-minimal")
    );
    assert_eq!(payload["data"]["auto_budget"], serde_json::json!(6000));
}

#[tokio::test]
async fn session_binding_rebinds_project_per_request() {
    let project_a = temp_project_dir("project-a");
    let project_b = temp_project_dir("project-b");
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
    let app = build_router(state.clone());

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

    let find_second = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"second_only","include_body":false,"max_matches":5}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let second_body = body_string(find_second).await;
    assert!(second_body.contains("second_only"));
    assert!(second_body.contains("second.py"));

    let find_first = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_a)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"first_only","include_body":false,"max_matches":5}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let first_body = body_string(find_first).await;
    assert!(first_body.contains("first_only"));
    assert!(first_body.contains("first.py"));
}

#[tokio::test]
async fn session_bound_missing_project_fails_closed() {
    let state = test_state();
    let app = build_router(state.clone());

    let init = app
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
    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let missing = temp_project_dir("missing").join("gone");
    state
        .session_store
        .as_ref()
        .unwrap()
        .get(&sid)
        .unwrap()
        .set_project_path(missing.to_string_lossy().to_string());

    let find = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"hello","include_body":false,"max_matches":5}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let body = body_string(find).await;
    assert!(body.contains("automatic rebind failed"));
}

#[tokio::test]
async fn session_profiles_are_isolated_across_tools_list() {
    let state = test_state();
    let app = build_router(state.clone());

    let init_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
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
                    r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
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

    let set_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_a)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"set_profile","arguments":{"profile":"builder-minimal"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(set_a.status(), StatusCode::OK);
    let set_a_body = body_string(set_a).await;
    let set_a_payload = first_tool_payload(&set_a_body);
    assert_eq!(
        set_a_payload["success"],
        serde_json::json!(true),
        "set_profile(session A) failed: {set_a_body}"
    );
    assert_eq!(
        set_a_payload["data"]["current_profile"],
        serde_json::json!("builder-minimal"),
        "unexpected set_profile(session A) payload: {set_a_body}"
    );

    let set_b = app
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
    assert_eq!(set_b.status(), StatusCode::OK);
    let set_b_body = body_string(set_b).await;
    let set_b_payload = first_tool_payload(&set_b_body);
    assert_eq!(
        set_b_payload["success"],
        serde_json::json!(true),
        "set_profile(session B) failed: {set_b_body}"
    );
    assert_eq!(
        set_b_payload["data"]["current_profile"],
        serde_json::json!("reviewer-graph"),
        "unexpected set_profile(session B) payload: {set_b_body}"
    );

    let list_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_a)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":5,"method":"tools/list","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let list_a_body = body_string(list_a).await;
    assert!(list_a_body.contains("\"active_surface\":\"builder-minimal\""));

    let list_b = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":6,"method":"tools/list","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let list_b_body = body_string(list_b).await;
    assert!(list_b_body.contains("\"active_surface\":\"reviewer-graph\""));
}

#[tokio::test]
async fn codex_session_uses_lean_tools_list_contract_by_default() {
    let state = test_state();
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"client_profile\":\"codex\""));
    assert!(body.contains("\"default_contract_mode\":\"lean\""));
    assert!(body.contains("\"include_output_schema\":false"));
    assert!(body.contains("\"include_annotations\":false"));
    assert!(!body.contains("\"outputSchema\""));
    assert!(!body.contains("\"annotations\""));
    assert!(!body.contains("\"visible_namespaces\""));
}

#[tokio::test]
async fn codex_builder_bootstrap_strips_input_schema_descriptions() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::BuilderMinimal,
    ));
    let app = build_router(state.clone());
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
    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"input_schema_descriptions_stripped\":true"));
    assert!(body.contains("\"name\":\"get_ranked_context\""));
    assert!(!body.contains("Symbol name to search for"));
    assert!(!body.contains("Disable semantic/hybrid ranking and use structural signals only"));
}

#[tokio::test]
async fn codex_builder_explicit_output_schema_preserves_input_schema_descriptions() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::BuilderMinimal,
    ));
    let app = build_router(state.clone());
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
    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"includeOutputSchema":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"input_schema_descriptions_stripped\":false"));
    assert!(body.contains("Symbol name to search for"));
    assert!(body.contains("Disable semantic/hybrid ranking and use structural signals only"));
}

#[tokio::test]
async fn claude_session_uses_full_tools_list_contract_by_default() {
    let state = test_state();
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"Claude Code","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"client_profile\":\"claude\""));
    assert!(body.contains("\"default_contract_mode\":\"full\""));
    assert!(body.contains("\"include_output_schema\":true"));
    assert!(body.contains("\"include_annotations\":true"));
    assert!(body.contains("\"outputSchema\""));
    assert!(body.contains("\"annotations\""));
    assert!(body.contains("\"visible_namespaces\""));
}

#[tokio::test]
async fn codex_session_can_restore_tool_annotations_explicitly() {
    let state = test_state();
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"includeAnnotations":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"include_annotations\":true"));
    assert!(body.contains("\"annotations\""));
}

#[tokio::test]
async fn mutation_enabled_daemon_rejects_untrusted_client_mutation() {
    let state = test_state();
    state.configure_daemon_mode(crate::state::RuntimeDaemonMode::MutationEnabled);
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"refactor-full"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let preflight = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"verify_change_readiness","arguments":{"task":"create audit_http.py","changed_files":["audit_http.py"]}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::OK);
    let preflight_body = body_string(preflight).await;
    assert!(preflight_body.contains("\\\"success\\\":true"));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"create_text_file","arguments":{"relative_path":"audit_http.py","content":"print('hi')","overwrite":true}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("requires a trusted HTTP client"));
}

#[tokio::test]
async fn verify_change_readiness_http_response_uses_slim_text_wrapper() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"verify_change_readiness","arguments":{"task":"update hello.txt","changed_files":["hello.txt"]}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let envelope: serde_json::Value = serde_json::from_str(&body).unwrap();
    let text_payload: serde_json::Value = serde_json::from_str(
        envelope["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("{}"),
    )
    .unwrap();
    assert!(text_payload["data"]["analysis_id"].is_string());
    assert!(text_payload["data"]["summary"].is_string());
    assert!(text_payload["data"]["readiness"].is_object());
    assert_eq!(text_payload["routing_hint"], serde_json::json!("async"));
    assert!(text_payload["data"].get("verifier_checks").is_none());
    assert!(text_payload["data"].get("blockers").is_none());
    assert!(envelope["result"]["structuredContent"]["analysis_id"].is_string());
    assert!(envelope["result"]["structuredContent"]["verifier_checks"].is_array());
    assert!(envelope["result"]["structuredContent"]["blockers"].is_array());
}

#[tokio::test]
async fn mutation_enabled_daemon_audits_trusted_client_metadata() {
    let state = test_state();
    state.configure_daemon_mode(crate::state::RuntimeDaemonMode::MutationEnabled);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::RefactorFull,
    ));
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("x-codelens-trusted-client", "true")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA","version":"2.2.0"},"profile":"refactor-full"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    // RefactorFull requires preflight before mutation
    let preflight = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .header("x-codelens-trusted-client", "true")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"verify_change_readiness","arguments":{"task":"create audit_http.py","changed_files":["audit_http.py"]}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::OK);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .header("x-codelens-trusted-client", "true")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"create_text_file","arguments":{"relative_path":"audit_http.py","content":"print('hi')","overwrite":true}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let envelope: serde_json::Value = serde_json::from_str(&body).unwrap();
    let text = envelope["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"success\":true"));
    let audit_path = state.audit_dir().join("mutation-audit.jsonl");
    let audit_body = std::fs::read_to_string(audit_path).unwrap();
    assert!(audit_body.contains("\"trusted_client\":true"));
    assert!(audit_body.contains("\"requested_profile\":\"refactor-full\""));
    assert!(audit_body.contains("\"client_name\":\"HarnessQA\""));
}

#[tokio::test]
async fn deferred_tools_list_uses_preferred_namespaces_for_session() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"deferred_loading_active\":true"));
    assert!(
        body.contains("\"preferred_namespaces\":[\"reports\",\"graph\",\"symbols\",\"session\"]")
    );
    assert!(body.contains("\"preferred_tiers\":[\"workflow\"]"));
    assert!(body.contains("\"loaded_namespaces\":[]"));
    assert!(body.contains("\"loaded_tiers\":[]"));
    assert!(body.contains("\"impact_report\""));
    assert!(!body.contains("\"find_symbol\""));
    assert!(!body.contains("\"read_file\""));
}

#[tokio::test]
async fn refactor_deferred_tools_list_starts_preview_first_for_session() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::RefactorFull,
    ));
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"refactor-full","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"deferred_loading_active\":true"));
    assert!(body.contains("\"preferred_namespaces\":[\"reports\",\"session\"]"));
    assert!(body.contains("\"tool_count\":4"));
    assert!(body.contains("\"verify_change_readiness\""));
    assert!(body.contains("\"refactor_safety_report\""));
    assert!(body.contains("\"safe_rename_report\""));
    assert!(body.contains("\"start_analysis_job\""));
    assert!(!body.contains("\"name\":\"rename_symbol\""));
    assert!(!body.contains("\"name\":\"replace_symbol_body\""));
    assert!(!body.contains("\"name\":\"refactor_extract_function\""));
    assert!(!body.contains("\"name\":\"impact_report\""));
    assert!(!body.contains("\"name\":\"diff_aware_references\""));
    assert!(!body.contains("\"name\":\"unresolved_reference_check\""));
}

#[tokio::test]
async fn deferred_resources_read_tracks_loaded_namespaces_for_session() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let summary = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"codelens://tools/list"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(summary.status(), StatusCode::OK);
    let summary_body = body_string(summary).await;
    let summary_text = first_resource_text(&summary_body);
    assert!(summary_text.contains("\"loaded_namespaces\": []"));
    assert!(summary_text.contains("\"loaded_tiers\": []"));
    assert!(!summary_text.contains("\"filesystem\":"));
    assert!(!summary_text.contains("\"find_symbol\""));

    let expand = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    // Use lsp namespace — get_file_diagnostics is in reviewer-graph but lsp is not preferred
                    r#"{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"codelens://tools/list","namespace":"lsp"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(expand.status(), StatusCode::OK);
    let expand_body = body_string(expand).await;
    let expand_text = first_resource_text(&expand_body);
    assert!(expand_text.contains("\"selected_namespace\": \"lsp\""));
    assert!(expand_text.contains("\"get_file_diagnostics\""));

    let tier_expand = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"codelens://tools/list","tier":"primitive"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(tier_expand.status(), StatusCode::OK);
    let tier_expand_body = body_string(tier_expand).await;
    let tier_expand_text = first_resource_text(&tier_expand_body);
    assert!(tier_expand_text.contains("\"selected_tier\": \"primitive\""));
    assert!(tier_expand_text.contains("\"find_symbol\""));

    let summary_after = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"codelens://tools/list"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(summary_after.status(), StatusCode::OK);
    let summary_after_body = body_string(summary_after).await;
    let summary_after_text = first_resource_text(&summary_after_body);
    assert!(summary_after_text.contains("\"loaded_namespaces\": ["));
    assert!(summary_after_text.contains("\"lsp\""));
    assert!(summary_after_text.contains("\"loaded_tiers\": ["));
    assert!(summary_after_text.contains("\"primitive\""));
    assert!(summary_after_text.contains("\"effective_namespaces\": ["));
    assert!(summary_after_text.contains("\"effective_tiers\": ["));
    assert!(summary_after_text.contains("\"get_file_diagnostics\""));
    assert!(summary_after_text.contains("\"find_symbol\""));

    let session_resource = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"codelens://session/http"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(session_resource.status(), StatusCode::OK);
    let session_body = body_string(session_resource).await;
    let session_text = first_resource_text(&session_body);
    assert!(session_text.contains("\"loaded_namespaces\": ["));
    assert!(session_text.contains("\"lsp\""));
    assert!(session_text.contains("\"loaded_tiers\": ["));
    assert!(session_text.contains("\"primitive\""));
    assert!(session_text.contains("\"full_tool_exposure\": false"));
    assert!(session_text.contains("\"preferred_tiers\": ["));
    assert!(session_text.contains("\"workflow\""));
    assert!(session_text.contains("\"client_profile\": \"generic\""));
    assert!(session_text.contains("\"default_tools_list_contract_mode\": \"full\""));
    assert!(session_text.contains("\"deferred_tier_gate\": true"));
    assert!(session_text.contains("\"requires_tier_listing_before_tool_call\": true"));
}

#[tokio::test]
async fn deferred_session_blocks_hidden_tool_calls_until_namespace_is_loaded() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());
    let file_path = state.project().as_path().join("deferred-hidden.py");
    std::fs::write(&file_path, "def alpha():\n    return 1\n").unwrap();

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    // get_file_diagnostics is in reviewer-graph but namespace "lsp" is not preferred,
    // so it should be hidden by deferred namespace loading.
    let blocked = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"get_file_diagnostics","arguments":{{"file_path":"{}"}}}}}}"#,
                    file_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(blocked.status(), StatusCode::OK);
    let blocked_body = body_string(blocked).await;
    assert!(blocked_body.contains("hidden by deferred loading"));
}

#[tokio::test]
async fn deferred_namespace_load_expands_default_surface_and_allows_calls() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());
    let mock_lsp = concat!(
        "#!/usr/bin/env python3\n",
        "import sys, json\n",
        "def read_msg():\n",
        "    h = ''\n",
        "    while True:\n",
        "        c = sys.stdin.buffer.read(1)\n",
        "        if not c: return None\n",
        "        h += c.decode('ascii')\n",
        "        if h.endswith('\\r\\n\\r\\n'): break\n",
        "    length = int([l for l in h.split('\\r\\n') if l.startswith('Content-Length:')][0].split(': ')[1])\n",
        "    return json.loads(sys.stdin.buffer.read(length).decode('utf-8'))\n",
        "def send(r):\n",
        "    out = json.dumps(r)\n",
        "    b = out.encode('utf-8')\n",
        "    sys.stdout.buffer.write(f'Content-Length: {len(b)}\\r\\n\\r\\n'.encode('ascii'))\n",
        "    sys.stdout.buffer.write(b)\n",
        "    sys.stdout.buffer.flush()\n",
        "while True:\n",
        "    msg = read_msg()\n",
        "    if msg is None: break\n",
        "    rid = msg.get('id')\n",
        "    m = msg.get('method', '')\n",
        "    if m == 'initialized': continue\n",
        "    if rid is None: continue\n",
        "    if m == 'initialize':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'textDocumentSync':1,'diagnosticProvider':{}}}})\n",
        "    elif m == 'textDocument/diagnostic':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'kind':'full','items':[]}})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let unique_name = format!("deferred-open-{}.py", std::process::id());
    let file_path = state.project().as_path().join(&unique_name);
    let mock_path = state.project().as_path().join("mock_lsp.py");
    std::fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&mock_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(&file_path, "def beta():\n    return 2\n").unwrap();

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    // Load tier "primitive" first — then namespace "lsp" becomes visible
    let tier_expand = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"tier":"primitive"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(tier_expand.status(), StatusCode::OK);
    let tier_body = body_string(tier_expand).await;
    assert!(tier_body.contains("\"selected_tier\":\"primitive\""));
    assert!(tier_body.contains("\"find_symbol\""));

    // Now load namespace "lsp" — get_file_diagnostics should appear
    let ns_expand = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{"namespace":"lsp"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(ns_expand.status(), StatusCode::OK);
    let ns_body = body_string(ns_expand).await;
    assert!(ns_body.contains("\"selected_namespace\":\"lsp\""));
    assert!(ns_body.contains("\"get_file_diagnostics\""));

    // Verify expanded namespace allows the tool call using a deterministic mock LSP.
    let allowed = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{{"name":"get_file_diagnostics","arguments":{{"file_path":"{}","command":"python3","args":["{}"]}}}}}}"#,
                    file_path.display(),
                    mock_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(allowed.status(), StatusCode::OK);
    let allowed_body = body_string(allowed).await;
    assert!(
        allowed_body.contains("\\\"success\\\":true"),
        "deferred_namespace body: {allowed_body}"
    );
}

#[tokio::test]
async fn deferred_tier_load_expands_default_surface_and_allows_calls() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());
    let file_path = state.project().as_path().join("deferred-tier.py");
    std::fs::write(&file_path, "def beta():\n    return 2\n").unwrap();

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let blocked = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"find_symbol","arguments":{{"name":"beta","file_path":"{}","include_body":false}}}}}}"#,
                    file_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(blocked.status(), StatusCode::OK);
    let blocked_body = body_string(blocked).await;
    assert!(blocked_body.contains("hidden by deferred loading in tier `primitive`"));

    let expand = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{"tier":"primitive"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(expand.status(), StatusCode::OK);
    let expand_body = body_string(expand).await;
    assert!(expand_body.contains("\"selected_tier\":\"primitive\""));
    assert!(expand_body.contains("\"find_symbol\""));

    let default_list = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":4,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(default_list.status(), StatusCode::OK);
    let default_body = body_string(default_list).await;
    assert!(default_body.contains("\"loaded_tiers\":[\"primitive\"]"));
    assert!(default_body.contains("\"find_symbol\""));

    let allowed = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{{"name":"find_symbol","arguments":{{"name":"beta","file_path":"{}","include_body":false}}}}}}"#,
                    file_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(allowed.status(), StatusCode::OK);
    let allowed_body = body_string(allowed).await;
    assert!(allowed_body.contains("\\\"success\\\":true"));
}

#[tokio::test]
async fn initialize_with_existing_session_resumes_same_session() {
    let state = test_state();
    let app = build_router(state.clone());
    let first = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = first
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let second = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA","version":"2.2.0"},"profile":"planner-readonly"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        second
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok()),
        Some(sid.as_str())
    );
    assert_eq!(
        second
            .headers()
            .get("x-codelens-session-resumed")
            .and_then(|value| value.to_str().ok()),
        Some("true")
    );
    let body = body_string(second).await;
    assert!(body.contains("\"resumed\":true"));
    assert!(body.contains(&sid));

    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    assert_eq!(session.resume_count(), 1);
    let metadata = session.client_metadata();
    assert_eq!(metadata.client_version.as_deref(), Some("2.2.0"));
    assert_eq!(
        metadata.requested_profile.as_deref(),
        Some("planner-readonly")
    );
}

#[tokio::test]
async fn post_invalid_json_returns_parse_error() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from("not json"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains("-32700"),
        "should return JSON-RPC parse error code"
    );
}

#[tokio::test]
async fn post_non_initialize_without_session_works() {
    // Non-initialize requests without session ID should still work
    // (session validation only rejects unknown session IDs, not missing ones)
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains("get_symbols_overview"),
        "tools/list should return tools"
    );
}

#[tokio::test]
async fn post_unknown_session_returns_not_found() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", "nonexistent-session-id")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_with_sse_accept_returns_event_stream() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("accept", "text/event-stream")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers().get("mcp-session-id").is_some(),
        "SSE response should also include session ID"
    );
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.contains("text/event-stream"),
        "Accept: text/event-stream should return SSE content-type, got: {ct}"
    );
}

#[tokio::test]
async fn server_card_exposes_daemon_mode() {
    let state = test_state();
    state.configure_daemon_mode(crate::state::RuntimeDaemonMode::ReadOnly);
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/.well-known/mcp.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"daemon_mode\": \"read-only\""));
    assert!(body.contains("session-client-metadata"));
}

#[tokio::test]
async fn post_notification_returns_no_content() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

// ── GET /mcp (SSE stream) ────────────────────────────────────────────

#[tokio::test]
async fn get_without_session_returns_bad_request() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_with_unknown_session_returns_not_found() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp")
                .header("mcp-session-id", "bogus-id")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── DELETE /mcp (session termination) ────────────────────────────────

#[tokio::test]
async fn delete_returns_no_content() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/mcp")
                .header("mcp-session-id", "any-id")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn delete_removes_session() {
    let state = test_state();
    let session = state.session_store.as_ref().unwrap().create();
    let sid = session.id.clone();

    // Verify session exists
    assert!(state.session_store.as_ref().unwrap().get(&sid).is_some());

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/mcp")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(
        state.session_store.as_ref().unwrap().get(&sid).is_none(),
        "session should be removed after DELETE"
    );
}

// ── Session lifecycle ────────────────────────────────────────────────

#[tokio::test]
async fn full_session_lifecycle() {
    let state = test_state();
    let app = build_router(state.clone());

    // 1. Initialize — get session ID
    let resp = app
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

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(!sid.is_empty());

    // 2. Use session for a tool call
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("get_symbols_overview"));

    // 3. Terminate session
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/mcp")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 4. Verify session is gone
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Session store edge cases ─────────────────────────────────────────

#[test]
fn concurrent_session_creation() {
    let store = SessionStore::new(Duration::from_secs(300));
    let sessions: Vec<_> = (0..100).map(|_| store.create()).collect();

    // All IDs unique
    let mut ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 100, "all 100 session IDs should be unique");
    assert_eq!(store.len(), 100);
}

#[test]
fn session_touch_refreshes_expiry() {
    let store = SessionStore::new(Duration::from_millis(50));
    let session = store.create();
    let id = session.id.clone();

    std::thread::sleep(Duration::from_millis(30));
    // Touch should reset the timer
    store.get(&id); // get() calls touch()
    std::thread::sleep(Duration::from_millis(30));

    // 60ms total but touched at 30ms, so 30ms since touch < 50ms timeout
    assert!(
        store.get(&id).is_some(),
        "session should still be alive after touch"
    );
}

#[test]
fn cleanup_only_removes_expired() {
    let store = SessionStore::new(Duration::from_millis(20));
    let s1 = store.create();
    std::thread::sleep(Duration::from_millis(30));
    let s2 = store.create(); // created after sleep, still fresh

    let removed = store.cleanup();
    assert_eq!(removed, 1, "only the expired session should be removed");
    assert!(store.get(&s1.id).is_none());
    assert!(store.get(&s2.id).is_some());
}
