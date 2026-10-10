//! Requests without `Mcp-Session-Id` used to run as one shared "local"
//! session: a sessionless `prepare_harness_session(project=X)` rebound every
//! other sessionless caller to X, and `x-codelens-project` /
//! `x-codelens-client` were ignored. Protocol 2026-07-28 makes every request
//! sessionless, so each one must carry its own scope.

use super::*;

struct SessionlessFixture {
    app: axum::Router,
    state: Arc<AppState>,
    daemon_project: std::path::PathBuf,
    other_project: std::path::PathBuf,
}

impl SessionlessFixture {
    fn new() -> Self {
        let daemon_project = std::fs::canonicalize(temp_project_dir("sessionless-daemon"))
            .expect("canonical daemon project");
        let other_project = std::fs::canonicalize(temp_project_dir("sessionless-other"))
            .expect("canonical other project");
        std::fs::write(
            other_project.join("other.py"),
            "def other():\n    return 1\n",
        )
        .expect("other fixture");
        let project =
            ProjectRoot::new(daemon_project.to_str().expect("utf-8 path")).expect("daemon project");
        let state = Arc::new(
            AppState::new(project, crate::tool_defs::ToolPreset::Balanced).with_session_store(),
        );
        Self {
            app: build_router(state.clone()),
            state,
            daemon_project,
            other_project,
        }
    }

    async fn call(
        &self,
        name: &str,
        arguments: serde_json::Value,
        headers: &[(&str, &str)],
        meta: Option<serde_json::Value>,
    ) -> serde_json::Value {
        let mut params = json!({"name": name, "arguments": arguments});
        if let Some(meta) = meta {
            params["_meta"] = meta;
        }
        let mut builder = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json");
        for (key, value) in headers {
            builder = builder.header(*key, *value);
        }
        let response = self
            .app
            .clone()
            .oneshot(
                builder
                    .body(axum::body::Body::from(
                        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": params})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers().get("mcp-session-id").is_none(),
            "a sessionless request must not be handed a session id"
        );
        first_tool_payload(&body_string(response).await)
    }

    async fn project_root(&self, headers: &[(&str, &str)]) -> String {
        let config = self
            .call("get_current_config", json!({}), headers, None)
            .await;
        config["data"]["project_root"]
            .as_str()
            .or_else(|| config["project_root"].as_str())
            .unwrap_or_else(|| panic!("project_root in {config:#}"))
            .to_owned()
    }

    async fn client_profile(
        &self,
        headers: &[(&str, &str)],
        meta: Option<serde_json::Value>,
    ) -> String {
        let config = self
            .call("get_current_config", json!({}), headers, meta)
            .await;
        config["data"]["client_profile"]
            .as_str()
            .or_else(|| config["client_profile"].as_str())
            .unwrap_or_else(|| panic!("client_profile in {config:#}"))
            .to_owned()
    }
}

#[tokio::test]
async fn a_sessionless_prepare_does_not_rebind_other_sessionless_callers() {
    let fixture = SessionlessFixture::new();
    let other = fixture.other_project.to_str().unwrap();

    let prepared = fixture
        .call(
            "prepare_harness_session",
            json!({"project": other, "detail": "compact"}),
            &[],
            None,
        )
        .await;

    let data = prepared.get("data").unwrap_or(&prepared);
    assert_eq!(data["binding_scope"], "request", "{prepared:#}");
    assert_eq!(
        fixture.project_root(&[]).await,
        fixture.daemon_project.to_str().unwrap(),
        "another sessionless caller must still see the daemon's project"
    );
}

#[tokio::test]
async fn a_sessionless_request_is_scoped_by_its_project_header() {
    let fixture = SessionlessFixture::new();
    let other = fixture.other_project.to_str().unwrap();

    assert_eq!(
        fixture.project_root(&[("x-codelens-project", other)]).await,
        other
    );
    assert_eq!(
        fixture.project_root(&[]).await,
        fixture.daemon_project.to_str().unwrap()
    );
}

#[tokio::test]
async fn a_sessionless_request_carries_its_client_identity() {
    // Without a client name the profile falls back to the process
    // environment (Claude Code sets CLAUDE_CODE_ENTRYPOINT, CI sets
    // nothing), so only a codex identity tells the request's own name apart
    // from that fallback everywhere.
    let fixture = SessionlessFixture::new();

    assert_eq!(
        fixture
            .client_profile(&[("x-codelens-client", "codex-cli")], None)
            .await,
        "codex"
    );
    let modern_meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "codex-mcp-client", "version": "0.162.0"}
    });
    assert_eq!(
        fixture.client_profile(&[], Some(modern_meta)).await,
        "codex"
    );
}

#[tokio::test]
async fn sessionless_requests_leave_no_session_behind() {
    let fixture = SessionlessFixture::new();
    let other = fixture.other_project.to_str().unwrap();

    for _ in 0..5 {
        fixture.project_root(&[("x-codelens-project", other)]).await;
    }
    fixture
        .call(
            "prepare_harness_session",
            json!({"project": other, "detail": "compact"}),
            &[],
            None,
        )
        .await;

    assert_eq!(fixture.state.active_session_count(), 0);
}

#[tokio::test]
async fn a_header_that_names_no_project_falls_back_instead_of_failing_every_call() {
    // `x-codelens-project: ${PWD}` sends whatever directory the host was
    // launched in. From $HOME (refused as a root) every tool call failed
    // with "refusing to infer the home directory"; a path that does not
    // resolve takes the same rejection path.
    let fixture = SessionlessFixture::new();
    let missing = fixture.daemon_project.join("no-such-dir");
    let missing = missing.to_str().unwrap();

    let config = fixture
        .call(
            "get_current_config",
            json!({}),
            &[("x-codelens-project", missing)],
            None,
        )
        .await;

    let data = config.get("data").unwrap_or(&config);
    assert_eq!(
        data["project_root"],
        fixture.daemon_project.to_str().unwrap(),
        "{config:#}"
    );
}

#[tokio::test]
async fn an_explicit_prepare_of_a_missing_project_still_fails() {
    let fixture = SessionlessFixture::new();
    let missing = fixture.daemon_project.join("no-such-dir");

    let prepared = fixture
        .call(
            "prepare_harness_session",
            json!({"project": missing.to_str().unwrap(), "detail": "compact"}),
            &[],
            None,
        )
        .await;

    assert_ne!(prepared["success"], true, "{prepared:#}");
}

fn percent_encode_path(path: &std::path::Path) -> String {
    path.to_str()
        .unwrap()
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

impl SessionlessFixture {
    async fn project_root_at(&self, uri: &str, headers: &[(&str, &str)]) -> String {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        for (key, value) in headers {
            builder = builder.header(*key, *value);
        }
        let response = self
            .app
            .clone()
            .oneshot(
                builder
                    .body(axum::body::Body::from(
                        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                               "params": {"name": "get_current_config", "arguments": {}}})
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let config = first_tool_payload(&body_string(response).await);
        let data = config.get("data").unwrap_or(&config);
        data["project_root"].as_str().unwrap_or_default().to_owned()
    }
}

#[tokio::test]
async fn a_project_query_parameter_binds_a_non_ascii_path() {
    // A header cannot carry `${PWD}` for a directory named in Korean: Claude
    // Code sends no request at all. The URL query percent-encodes it.
    let fixture = SessionlessFixture::new();
    let korean = std::fs::canonicalize(temp_project_dir("한글-프로젝트")).unwrap();
    std::fs::write(korean.join("main.py"), "def main():\n    return 1\n").unwrap();

    let uri = format!("/mcp?project={}", percent_encode_path(&korean));
    assert_eq!(
        fixture.project_root_at(&uri, &[]).await,
        korean.to_str().unwrap()
    );
}

#[tokio::test]
async fn a_project_header_wins_over_the_query_parameter() {
    let fixture = SessionlessFixture::new();
    let other = fixture.other_project.to_str().unwrap();
    let uri = format!(
        "/mcp?project={}",
        percent_encode_path(&fixture.daemon_project)
    );

    assert_eq!(
        fixture
            .project_root_at(&uri, &[("x-codelens-project", other)])
            .await,
        other
    );
}

#[tokio::test]
async fn a_plus_in_the_project_query_stays_a_plus() {
    let fixture = SessionlessFixture::new();
    let plus = std::fs::canonicalize(temp_project_dir("c++-tools")).unwrap();
    std::fs::write(plus.join("lib.py"), "def f():\n    return 1\n").unwrap();
    let uri = format!("/mcp?project={}", plus.to_str().unwrap());

    assert_eq!(
        fixture.project_root_at(&uri, &[]).await,
        plus.to_str().unwrap()
    );
}
