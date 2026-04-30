use super::*;

#[test]
fn delegate_handoff_id_persists_across_planner_and_builder_sessions_in_telemetry() {
    struct TelemetryEnvGuard {
        prev_sym_enabled: Option<String>,
        prev_enabled: Option<String>,
        prev_sym_path: Option<String>,
        prev_path: Option<String>,
    }

    impl TelemetryEnvGuard {
        fn install(path: &std::path::Path) -> Self {
            let guard = Self {
                prev_sym_enabled: std::env::var("SYMBIOTE_TELEMETRY_ENABLED").ok(),
                prev_enabled: std::env::var("CODELENS_TELEMETRY_ENABLED").ok(),
                prev_sym_path: std::env::var("SYMBIOTE_TELEMETRY_PATH").ok(),
                prev_path: std::env::var("CODELENS_TELEMETRY_PATH").ok(),
            };
            unsafe {
                std::env::remove_var("SYMBIOTE_TELEMETRY_ENABLED");
                std::env::set_var("CODELENS_TELEMETRY_ENABLED", "1");
                std::env::remove_var("SYMBIOTE_TELEMETRY_PATH");
                std::env::set_var("CODELENS_TELEMETRY_PATH", path);
            }
            guard
        }
    }

    impl Drop for TelemetryEnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prev_sym_enabled {
                    Some(value) => std::env::set_var("SYMBIOTE_TELEMETRY_ENABLED", value),
                    None => std::env::remove_var("SYMBIOTE_TELEMETRY_ENABLED"),
                }
                match &self.prev_enabled {
                    Some(value) => std::env::set_var("CODELENS_TELEMETRY_ENABLED", value),
                    None => std::env::remove_var("CODELENS_TELEMETRY_ENABLED"),
                }
                match &self.prev_sym_path {
                    Some(value) => std::env::set_var("SYMBIOTE_TELEMETRY_PATH", value),
                    None => std::env::remove_var("SYMBIOTE_TELEMETRY_PATH"),
                }
                match &self.prev_path {
                    Some(value) => std::env::set_var("CODELENS_TELEMETRY_PATH", value),
                    None => std::env::remove_var("CODELENS_TELEMETRY_PATH"),
                }
            }
        }
    }

    let _env_lock = crate::env_compat::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let telemetry_path = std::env::temp_dir().join(format!(
        "codelens-delegate-telemetry-{}-{:?}.jsonl",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::thread::current().id()
    ));
    let _env_guard = TelemetryEnvGuard::install(&telemetry_path);

    let project = project_root();
    fs::write(
        project.as_path().join("rename_delegate_telemetry.py"),
        "def old_name():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "refactor-full"}),
        "planner-session",
    );
    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "refactor-full"}),
        "builder-session",
    );

    let planner_payload = call_tool_with_session(
        &state,
        "safe_rename_report",
        json!({
            "file_path": "rename_delegate_telemetry.py",
            "symbol": "old_name",
            "new_name": "new_name"
        }),
        "planner-session",
    );
    assert_eq!(planner_payload["success"], json!(true));

    let delegate_call = planner_payload["suggested_next_calls"]
        .as_array()
        .and_then(|calls| {
            calls.iter().find(|call| {
                call.get("tool").and_then(|value| value.as_str())
                    == Some("delegate_to_codex_builder")
            })
        })
        .cloned()
        .expect("delegate_to_codex_builder should include a scaffold payload");

    let handoff_id = delegate_call["arguments"]["handoff_id"]
        .as_str()
        .expect("delegate scaffold should include handoff_id")
        .to_owned();
    let builder_arguments = delegate_call["arguments"]["delegate_arguments"].clone();

    let builder_payload = call_tool_with_session(
        &state,
        "rename_symbol",
        builder_arguments,
        "builder-session",
    );
    assert!(
        builder_payload["data"].is_object()
            || builder_payload.get("suggested_next_tools").is_some(),
        "builder response should remain structured for telemetry correlation: {builder_payload}"
    );

    let contents = std::fs::read_to_string(&telemetry_path).expect("read telemetry jsonl");
    let events: Vec<serde_json::Value> = contents
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse telemetry jsonl line"))
        .collect();

    let planner_event = events
        .iter()
        .find(|event| {
            event["session_id"] == json!("planner-session")
                && event["tool"] == json!("safe_rename_report")
                && event["delegate_handoff_id"].as_str() == Some(handoff_id.as_str())
        })
        .cloned()
        .expect("planner event should persist delegate handoff metadata");
    assert_eq!(
        planner_event["delegate_hint_trigger"],
        json!("preferred_executor_boundary")
    );

    let builder_event = events
        .iter()
        .find(|event| {
            event["session_id"] == json!("builder-session")
                && event["tool"] == json!("rename_symbol")
                && event["handoff_id"].as_str() == Some(handoff_id.as_str())
        })
        .cloned()
        .expect("builder event should persist the replayed handoff_id");
    assert_ne!(planner_event["session_id"], builder_event["session_id"]);

    let _ = std::fs::remove_dir_all(telemetry_path.parent().unwrap());
}

#[test]
fn session_scoped_preflight_does_not_cross_sessions() {
    let project = project_root();
    fs::write(project.as_path().join("session_gate.py"), "print('old')\n").unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let preflight = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update session-gated file",
            "changed_files": ["session_gate.py"]
        }),
        "session-a",
    );
    assert_eq!(preflight["success"], json!(true));

    let payload = call_tool_with_session(
        &state,
        "replace_content",
        json!({
            "relative_path": "session_gate.py",
            "old_text": "old",
            "new_text": "new"
        }),
        "session-b",
    );
    assert_eq!(payload["success"], json!(false));
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or("")
            .contains("requires a fresh preflight")
    );
}

#[test]
fn export_session_markdown_is_safe_for_unknown_session_id() {
    let project = project_root();
    fs::write(
        project.as_path().join("planner_md_unknown.py"),
        "print('md')\n",
    )
    .unwrap();
    let state = make_state(&project);

    let markdown = call_tool(
        &state,
        "export_session_markdown",
        json!({"session_id": "missing-session", "name": "missing-session"}),
    );
    assert_eq!(markdown["success"], json!(false));
    assert_eq!(
        markdown["error"],
        json!("Not found: unknown session_id `missing-session`")
    );
}
