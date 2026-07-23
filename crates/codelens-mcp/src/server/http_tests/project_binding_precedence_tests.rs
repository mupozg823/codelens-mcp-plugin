use super::*;

struct BindingFixture {
    app: axum::Router,
    state: Arc<AppState>,
    header_project: std::path::PathBuf,
    explicit_project: std::path::PathBuf,
}

struct ToolRequest<'a> {
    session_id: &'a str,
    project_header: &'a std::path::Path,
    id: u64,
    name: &'a str,
    arguments: serde_json::Value,
}

impl BindingFixture {
    fn new() -> Self {
        let header_project = std::fs::canonicalize(temp_project_dir("binding-precedence-header"))
            .expect("canonical header project");
        let explicit_project =
            std::fs::canonicalize(temp_project_dir("binding-precedence-explicit"))
                .expect("canonical explicit project");
        std::fs::write(
            header_project.join("header_fixture.py"),
            "def header_only_marker():\n    return 'header'\n",
        )
        .expect("header fixture");
        std::fs::write(
            explicit_project.join("explicit_fixture.py"),
            "def explicit_only_marker():\n    return 'explicit'\n",
        )
        .expect("explicit fixture");
        let project = ProjectRoot::new(
            header_project
                .to_str()
                .expect("header project must be utf-8"),
        )
        .expect("daemon project");
        let state = Arc::new(
            AppState::new(project, crate::tool_defs::ToolPreset::Balanced).with_session_store(),
        );
        let app = build_router(state.clone());
        Self {
            app,
            state,
            header_project,
            explicit_project,
        }
    }

    async fn initialize(&self, initialize_project: Option<&std::path::Path>) -> String {
        let mut params = json!({"clientInfo": {"name": "binding-precedence-qa"}});
        if let Some(project) = initialize_project {
            params["project"] = json!(project);
        }
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .header(
                        "x-codelens-project",
                        self.header_project
                            .to_str()
                            .expect("header project must be utf-8"),
                    )
                    .body(axum::body::Body::from(
                        json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "method": "initialize",
                            "params": params
                        })
                        .to_string(),
                    ))
                    .expect("initialize request"),
            )
            .await
            .expect("initialize response");
        assert_eq!(response.status(), StatusCode::OK);
        response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("mcp session id")
            .to_owned()
    }

    async fn call_tool(&self, request: ToolRequest<'_>) -> axum::response::Response {
        self.app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .header("mcp-session-id", request.session_id)
                    .header(
                        "x-codelens-project",
                        request
                            .project_header
                            .to_str()
                            .expect("project header must be utf-8"),
                    )
                    .body(axum::body::Body::from(
                        json!({
                            "jsonrpc": "2.0",
                            "id": request.id,
                            "method": "tools/call",
                            "params": {
                                "name": request.name,
                                "arguments": request.arguments
                            }
                        })
                        .to_string(),
                    ))
                    .expect("tool request"),
            )
            .await
            .expect("tool response")
    }
}

#[tokio::test]
async fn explicit_prepare_binding_outlives_conflicting_recurring_header() {
    // Given: a header-bound session that explicitly prepares a different project.
    let fixture = BindingFixture::new();
    let session_id = fixture.initialize(None).await;
    let prepare = fixture
        .call_tool(ToolRequest {
            session_id: &session_id,
            project_header: &fixture.header_project,
            id: 2,
            name: "prepare_harness_session",
            arguments: json!({
                "project": fixture.explicit_project,
                "detail": "compact"
            }),
        })
        .await;
    assert_eq!(prepare.status(), StatusCode::OK);
    let prepare_body = body_string(prepare).await;
    let prepare_envelope: serde_json::Value =
        serde_json::from_str(&prepare_body).expect("prepare response envelope");
    let binding = &prepare_envelope["result"]["structuredContent"]["project"];
    assert_eq!(
        binding["effective_project"],
        json!(fixture.explicit_project)
    );
    assert_eq!(binding["binding_source"], "explicit_tool");
    assert_eq!(
        binding["persistence_semantics"]["scope"],
        "live_http_session"
    );
    assert_eq!(
        binding["persistence_semantics"]["persists_across_requests"],
        true
    );
    assert_eq!(
        binding["persistence_semantics"]["survives_session_resurrection"],
        false
    );

    // When: the host repeats its lower-precedence project header on the next call.
    let find = fixture
        .call_tool(ToolRequest {
            session_id: &session_id,
            project_header: &fixture.header_project,
            id: 3,
            name: "find_symbol",
            arguments: json!({
                "name": "explicit_only_marker",
                "include_body": false
            }),
        })
        .await;
    let body = body_string(find).await;

    // Then: the explicit project remains effective for the live session.
    assert_eq!(
        fixture.state.session_project_path(&session_id).as_deref(),
        fixture.explicit_project.to_str()
    );
    assert!(
        body.contains("explicit_only_marker") && body.contains("explicit_fixture.py"),
        "the recurring header must not replace the explicit project: {body}"
    );
    assert!(!body.contains("header_only_marker"));
}

#[tokio::test]
async fn initialize_project_outlives_conflicting_recurring_header() {
    // Given: initialize params explicitly bind a project that differs from the host header.
    let fixture = BindingFixture::new();
    let session_id = fixture.initialize(Some(&fixture.explicit_project)).await;

    // When: the host repeats its lower-precedence project header on a tool call.
    let find = fixture
        .call_tool(ToolRequest {
            session_id: &session_id,
            project_header: &fixture.header_project,
            id: 2,
            name: "find_symbol",
            arguments: json!({
                "name": "explicit_only_marker",
                "include_body": false
            }),
        })
        .await;
    let body = body_string(find).await;

    // Then: initialize params remain the effective binding.
    assert_eq!(
        fixture.state.session_project_path(&session_id).as_deref(),
        fixture.explicit_project.to_str()
    );
    assert!(
        body.contains("explicit_only_marker") && body.contains("explicit_fixture.py"),
        "the recurring header must not replace initialize params: {body}"
    );
}

#[tokio::test]
async fn prepare_without_project_keeps_header_binding_switchable() {
    // Given: a header-bound session bootstrapped without an explicit project argument.
    let fixture = BindingFixture::new();
    let session_id = fixture.initialize(None).await;
    let prepare = fixture
        .call_tool(ToolRequest {
            session_id: &session_id,
            project_header: &fixture.header_project,
            id: 2,
            name: "prepare_harness_session",
            arguments: json!({"detail": "compact"}),
        })
        .await;
    assert_eq!(prepare.status(), StatusCode::OK);

    // When: the same header-bound host switches to another project.
    let find = fixture
        .call_tool(ToolRequest {
            session_id: &session_id,
            project_header: &fixture.explicit_project,
            id: 3,
            name: "find_symbol",
            arguments: json!({
                "name": "explicit_only_marker",
                "include_body": false
            }),
        })
        .await;
    let body = body_string(find).await;

    // Then: no-project bootstrap must not promote the old header to ExplicitTool.
    assert_eq!(
        fixture.state.session_project_path(&session_id).as_deref(),
        fixture.explicit_project.to_str()
    );
    assert!(
        body.contains("explicit_only_marker") && body.contains("explicit_fixture.py"),
        "the later header must remain able to switch projects: {body}"
    );
}

#[tokio::test]
async fn later_explicit_prepare_can_switch_back_without_header_override() {
    // Given: an explicit prepare moved a header-bound session from A to B.
    let fixture = BindingFixture::new();
    let session_id = fixture.initialize(None).await;
    let first_prepare = fixture
        .call_tool(ToolRequest {
            session_id: &session_id,
            project_header: &fixture.header_project,
            id: 2,
            name: "prepare_harness_session",
            arguments: json!({
                "project": fixture.explicit_project,
                "detail": "compact"
            }),
        })
        .await;
    assert_eq!(first_prepare.status(), StatusCode::OK);

    // When: a later explicit prepare switches back to A.
    let second_prepare = fixture
        .call_tool(ToolRequest {
            session_id: &session_id,
            project_header: &fixture.header_project,
            id: 3,
            name: "prepare_harness_session",
            arguments: json!({
                "project": fixture.header_project,
                "detail": "compact"
            }),
        })
        .await;
    assert_eq!(second_prepare.status(), StatusCode::OK);

    // Then: a conflicting recurring header for B still cannot replace explicit A.
    let find = fixture
        .call_tool(ToolRequest {
            session_id: &session_id,
            project_header: &fixture.explicit_project,
            id: 4,
            name: "find_symbol",
            arguments: json!({
                "name": "header_only_marker",
                "include_body": false
            }),
        })
        .await;
    let body = body_string(find).await;
    assert_eq!(
        fixture.state.session_project_path(&session_id).as_deref(),
        fixture.header_project.to_str()
    );
    assert!(
        body.contains("header_only_marker") && body.contains("header_fixture.py"),
        "the later explicit binding must remain authoritative: {body}"
    );
}

#[tokio::test]
async fn explicit_binding_resurrection_boundary_is_observable() {
    // Given: a live session explicitly moved from recurring header A to project B.
    let fixture = BindingFixture::new();
    let session_id = fixture.initialize(None).await;
    let prepare = fixture
        .call_tool(ToolRequest {
            session_id: &session_id,
            project_header: &fixture.header_project,
            id: 2,
            name: "prepare_harness_session",
            arguments: json!({
                "project": fixture.explicit_project,
                "detail": "compact"
            }),
        })
        .await;
    assert_eq!(prepare.status(), StatusCode::OK);

    // When: the live session record is lost and the recurring A header resurrects it.
    fixture
        .state
        .session_store
        .as_ref()
        .expect("session store")
        .remove(&session_id);
    let find = fixture
        .call_tool(ToolRequest {
            session_id: &session_id,
            project_header: &fixture.header_project,
            id: 3,
            name: "find_symbol",
            arguments: json!({
                "name": "header_only_marker",
                "include_body": false
            }),
        })
        .await;
    assert_eq!(
        find.headers()
            .get("x-codelens-session-resurrected")
            .and_then(|value| value.to_str().ok()),
        Some("1"),
        "the fallback to the recurring header must be observable as resurrection"
    );
    let body = body_string(find).await;

    // Then: the documented live-session boundary is honored, not misreported as durable.
    assert!(
        body.contains("header_only_marker") && body.contains("header_fixture.py"),
        "a recreated session must use its reasserted request header: {body}"
    );
    assert!(!body.contains("explicit_only_marker"));
}
