use super::*;
use std::path::PathBuf;

// ── Persistence tests ────────────────────────────────────────────────

fn unique_telemetry_path(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "codelens-telemetry-test-{label}-{}-{:?}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::thread::current().id(),
    ));
    dir.join("tool_usage.jsonl")
}

fn event<'a>(tool: &'a str, surface: &'a str) -> ToolCallEvent<'a> {
    ToolCallEvent {
        tool,
        operation: crate::operation::ResolvedOperation::direct(tool).dispatched(),
        elapsed_ms: 0,
        tokens: 0,
        success: true,
        surface,
        truncated: false,
        phase: None,
        logical_session_id: None,
        client_name: None,
        target_paths: &[],
        hints: CallTelemetryHints::default(),
    }
}

#[test]
fn telemetry_writer_persists_single_event() {
    let path = unique_telemetry_path("single");
    let writer = TelemetryWriter::with_path(path.clone());
    assert_eq!(writer.path(), path.as_path());

    writer.append_event(&PersistedEvent {
        timestamp_ms: 123,
        tool: "find_symbol",
        resolved_target: Some("find_symbol"),
        mode: None,
        work_class: crate::operation::operation_work_class("find_symbol"),
        downstream_call_count: 1,
        surface: "planner-readonly",
        elapsed_ms: 42,
        tokens: 500,
        success: true,
        truncated: false,
        session_id: Some("session-a"),
        client_name: None,
        phase: Some("plan"),
        recording_origin: "test",
        target_paths: None,
        suggested_next_tools: &[],
        delegate_hint_trigger: None,
        delegate_target_tool: None,
        delegate_handoff_id: None,
        handoff_id: None,
    });

    let contents = std::fs::read_to_string(&path).expect("read jsonl");
    let parsed: serde_json::Value =
        serde_json::from_str(contents.trim()).expect("parse single jsonl line");
    assert_eq!(parsed["tool"], "find_symbol");
    assert_eq!(parsed["resolved_target"], "find_symbol");
    assert_eq!(parsed["work_class"], "primitive");
    assert_eq!(parsed["downstream_call_count"], 1);
    assert!(parsed.get("mode").is_none());
    assert_eq!(parsed["surface"], "planner-readonly");
    assert_eq!(parsed["elapsed_ms"], 42);
    assert_eq!(parsed["tokens"], 500);
    assert_eq!(parsed["success"], true);
    assert_eq!(parsed["truncated"], false);
    assert_eq!(parsed["session_id"], "session-a");
    assert_eq!(parsed["phase"], "plan");
    assert_eq!(parsed["timestamp_ms"], 123);

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn telemetry_writer_appends_multiple_events_in_order() {
    let path = unique_telemetry_path("multi");
    let writer = TelemetryWriter::with_path(path.clone());

    for i in 0..3u64 {
        writer.append_event(&PersistedEvent {
            timestamp_ms: i,
            tool: "get_ranked_context",
            resolved_target: Some("get_ranked_context"),
            mode: None,
            work_class: crate::operation::operation_work_class("get_ranked_context"),
            downstream_call_count: 1,
            surface: "primitive",
            elapsed_ms: i,
            tokens: (i * 10) as usize,
            success: true,
            truncated: false,
            session_id: None,
            client_name: None,
            phase: None,
            recording_origin: "test",
            target_paths: None,
            suggested_next_tools: &[],
            delegate_hint_trigger: None,
            delegate_target_tool: None,
            delegate_handoff_id: None,
            handoff_id: None,
        });
    }

    let contents = std::fs::read_to_string(&path).expect("read jsonl");
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines.len(), 3);
    for (i, line) in lines.iter().enumerate() {
        let parsed: serde_json::Value = serde_json::from_str(line).expect("parse jsonl line");
        assert_eq!(parsed["timestamp_ms"], i as u64);
        // phase is None — field must be skipped entirely.
        assert!(
            parsed.get("phase").is_none(),
            "phase should be omitted when None"
        );
    }

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn telemetry_writer_persists_delegate_hint_fields() {
    let path = unique_telemetry_path("delegate");
    let writer = TelemetryWriter::with_path(path.clone());
    let suggested = vec![
        "delegate_to_codex_builder".to_owned(),
        "rename_symbol".to_owned(),
    ];

    writer.append_event(&PersistedEvent {
        timestamp_ms: 321,
        tool: "safe_rename_report",
        resolved_target: Some("safe_rename_report"),
        mode: None,
        work_class: crate::operation::operation_work_class("safe_rename_report"),
        downstream_call_count: 1,
        surface: "refactor-full",
        elapsed_ms: 18,
        tokens: 144,
        success: true,
        truncated: false,
        session_id: Some("planner-a"),
        client_name: None,
        phase: Some("review"),
        recording_origin: "test",
        target_paths: None,
        suggested_next_tools: &suggested,
        delegate_hint_trigger: Some("preferred_executor_boundary"),
        delegate_target_tool: Some("rename_symbol"),
        delegate_handoff_id: Some("codelens-handoff-1"),
        handoff_id: Some("codelens-handoff-1"),
    });

    let contents = std::fs::read_to_string(&path).expect("read jsonl");
    let parsed: serde_json::Value =
        serde_json::from_str(contents.trim()).expect("parse single jsonl line");
    assert_eq!(
        parsed["suggested_next_tools"],
        serde_json::json!(["delegate_to_codex_builder", "rename_symbol"])
    );
    assert_eq!(
        parsed["delegate_hint_trigger"],
        "preferred_executor_boundary"
    );
    assert_eq!(parsed["delegate_target_tool"], "rename_symbol");
    assert_eq!(parsed["delegate_handoff_id"], "codelens-handoff-1");
    assert_eq!(parsed["handoff_id"], "codelens-handoff-1");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn registry_persists_record_call_when_writer_enabled() {
    let path = unique_telemetry_path("registry");
    let registry =
        ToolMetricsRegistry::new_with_writer(Some(TelemetryWriter::with_path(path.clone())));

    registry.record_event(ToolCallEvent {
        elapsed_ms: 27,
        tokens: 309,
        phase: Some("plan"),
        ..event("find_symbol", "primitive")
    });
    registry.record_event(ToolCallEvent {
        elapsed_ms: 14,
        success: false,
        truncated: true,
        ..event("rename_symbol", "refactor-full")
    });

    // In-memory metrics still work
    let session = registry.session_snapshot();
    assert_eq!(session.core.total_calls, 2);
    assert_eq!(session.core.error_count, 1);

    // Persisted jsonl has both events
    let contents = std::fs::read_to_string(&path).expect("read jsonl");
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines.len(), 2);

    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["tool"], "find_symbol");
    assert_eq!(first["elapsed_ms"], 27);
    assert_eq!(first["tokens"], 309);
    assert_eq!(first["success"], true);
    assert_eq!(first["phase"], "plan");

    let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(second["tool"], "rename_symbol");
    assert_eq!(second["success"], false);
    assert_eq!(second["truncated"], true);
    assert!(second.get("phase").is_none());

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn registry_records_structured_event_without_jsonl_schema_drift() {
    let path = unique_telemetry_path("structured-event");
    let registry =
        ToolMetricsRegistry::new_with_writer(Some(TelemetryWriter::with_path(path.clone())));
    let target_paths = vec!["crates/codelens-mcp/src/telemetry.rs".to_owned()];
    let suggested_next_tools = vec!["verify_change_readiness".to_owned()];

    registry.record_event(ToolCallEvent {
        tool: "safe_rename_report",
        operation: crate::operation::ResolvedOperation::direct("safe_rename_report").dispatched(),
        elapsed_ms: 31,
        tokens: 512,
        success: true,
        surface: "refactor-full",
        truncated: false,
        phase: Some("review"),
        logical_session_id: Some("session-structured"),
        client_name: Some("CodexHarness"),
        target_paths: &target_paths,
        hints: CallTelemetryHints {
            suggested_next_tools: &suggested_next_tools,
            delegate_hint_trigger: Some("preferred_executor_boundary"),
            delegate_target_tool: Some("rename_symbol"),
            delegate_handoff_id: Some("handoff-structured"),
            handoff_id: Some("handoff-structured"),
        },
    });

    let session = registry.session_snapshot_for("session-structured");
    assert_eq!(session.core.total_calls, 1);
    assert_eq!(session.core.total_tokens, 512);
    assert_eq!(session.timeline[0].target_paths, target_paths);

    let contents = std::fs::read_to_string(&path).expect("read jsonl");
    let persisted: serde_json::Value = serde_json::from_str(contents.trim()).expect("parse jsonl");
    assert_eq!(persisted["tool"], "safe_rename_report");
    assert_eq!(persisted["surface"], "refactor-full");
    assert_eq!(persisted["session_id"], "session-structured");
    assert_eq!(persisted["client_name"], "CodexHarness");
    assert_eq!(persisted["target_paths"], serde_json::json!(target_paths));
    assert_eq!(persisted["recording_origin"], "test");
    assert_eq!(
        persisted["suggested_next_tools"],
        serde_json::json!(suggested_next_tools)
    );
    assert_eq!(persisted["delegate_target_tool"], "rename_symbol");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn registry_new_does_not_persist_when_test_enables_telemetry_env() {
    let _guard = crate::env_compat::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let path = unique_telemetry_path("test-env");
    let previous_path = std::env::var("CODELENS_TELEMETRY_PATH").ok();
    let previous_enabled = std::env::var("CODELENS_TELEMETRY_ENABLED").ok();
    let previous_symbiote_path = std::env::var("SYMBIOTE_TELEMETRY_PATH").ok();
    let previous_symbiote_enabled = std::env::var("SYMBIOTE_TELEMETRY_ENABLED").ok();

    unsafe {
        std::env::set_var("CODELENS_TELEMETRY_PATH", &path);
        std::env::set_var("CODELENS_TELEMETRY_ENABLED", "1");
        std::env::remove_var("SYMBIOTE_TELEMETRY_PATH");
        std::env::remove_var("SYMBIOTE_TELEMETRY_ENABLED");
    }

    let registry = ToolMetricsRegistry::new();
    registry.record_call("find_symbol", 10, true);
    let persisted = path.exists();

    unsafe {
        match previous_path {
            Some(value) => std::env::set_var("CODELENS_TELEMETRY_PATH", value),
            None => std::env::remove_var("CODELENS_TELEMETRY_PATH"),
        }
        match previous_enabled {
            Some(value) => std::env::set_var("CODELENS_TELEMETRY_ENABLED", value),
            None => std::env::remove_var("CODELENS_TELEMETRY_ENABLED"),
        }
        match previous_symbiote_path {
            Some(value) => std::env::set_var("SYMBIOTE_TELEMETRY_PATH", value),
            None => std::env::remove_var("SYMBIOTE_TELEMETRY_PATH"),
        }
        match previous_symbiote_enabled {
            Some(value) => std::env::set_var("SYMBIOTE_TELEMETRY_ENABLED", value),
            None => std::env::remove_var("SYMBIOTE_TELEMETRY_ENABLED"),
        }
    }
    let _ = std::fs::remove_dir_all(path.parent().expect("telemetry path has parent"));

    assert!(!persisted, "test registry must not write runtime telemetry");
}

#[test]
fn registry_persists_when_test_telemetry_is_explicitly_enabled() {
    let _guard = crate::env_compat::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let path = unique_telemetry_path("test-opt-in");
    let previous_path = std::env::var("CODELENS_TELEMETRY_PATH").ok();
    let previous_enabled = std::env::var("CODELENS_TELEMETRY_ENABLED").ok();
    let previous_test_enabled = std::env::var("CODELENS_TEST_TELEMETRY_ENABLED").ok();
    let previous_symbiote_path = std::env::var("SYMBIOTE_TELEMETRY_PATH").ok();
    let previous_symbiote_enabled = std::env::var("SYMBIOTE_TELEMETRY_ENABLED").ok();

    unsafe {
        std::env::set_var("CODELENS_TELEMETRY_PATH", &path);
        std::env::set_var("CODELENS_TELEMETRY_ENABLED", "1");
        std::env::set_var("CODELENS_TEST_TELEMETRY_ENABLED", "1");
        std::env::remove_var("SYMBIOTE_TELEMETRY_PATH");
        std::env::remove_var("SYMBIOTE_TELEMETRY_ENABLED");
    }

    let registry = ToolMetricsRegistry::new();
    registry.record_call("find_symbol", 10, true);
    let persisted = path.exists();

    unsafe {
        match previous_path {
            Some(value) => std::env::set_var("CODELENS_TELEMETRY_PATH", value),
            None => std::env::remove_var("CODELENS_TELEMETRY_PATH"),
        }
        match previous_enabled {
            Some(value) => std::env::set_var("CODELENS_TELEMETRY_ENABLED", value),
            None => std::env::remove_var("CODELENS_TELEMETRY_ENABLED"),
        }
        match previous_test_enabled {
            Some(value) => std::env::set_var("CODELENS_TEST_TELEMETRY_ENABLED", value),
            None => std::env::remove_var("CODELENS_TEST_TELEMETRY_ENABLED"),
        }
        match previous_symbiote_path {
            Some(value) => std::env::set_var("SYMBIOTE_TELEMETRY_PATH", value),
            None => std::env::remove_var("SYMBIOTE_TELEMETRY_PATH"),
        }
        match previous_symbiote_enabled {
            Some(value) => std::env::set_var("SYMBIOTE_TELEMETRY_ENABLED", value),
            None => std::env::remove_var("SYMBIOTE_TELEMETRY_ENABLED"),
        }
    }
    let _ = std::fs::remove_dir_all(path.parent().expect("telemetry path has parent"));

    assert!(
        persisted,
        "explicit test telemetry must write its temp path"
    );
}

#[test]
fn registry_without_writer_is_noop_for_persistence() {
    let registry = ToolMetricsRegistry::new_with_writer(None);
    registry.record_event(ToolCallEvent {
        elapsed_ms: 10,
        tokens: 100,
        ..event("find_symbol", "primitive")
    });
    // In-memory must still work
    let session = registry.session_snapshot();
    assert_eq!(session.core.total_calls, 1);
    assert_eq!(session.core.total_tokens, 100);
    // No panic, nothing to verify on disk.
}

#[test]
fn telemetry_writer_from_env_disabled_by_default() {
    let _guard = crate::env_compat::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    // Save and clear env
    let prev_sym_enabled = std::env::var("SYMBIOTE_TELEMETRY_ENABLED").ok();
    let prev_sym_path = std::env::var("SYMBIOTE_TELEMETRY_PATH").ok();
    let prev_enabled = std::env::var("CODELENS_TELEMETRY_ENABLED").ok();
    let prev_path = std::env::var("CODELENS_TELEMETRY_PATH").ok();
    // SAFETY: tests in this file do not run in parallel across env
    // boundaries for this variable, and we restore afterwards.
    unsafe {
        std::env::remove_var("SYMBIOTE_TELEMETRY_ENABLED");
        std::env::remove_var("SYMBIOTE_TELEMETRY_PATH");
        std::env::remove_var("CODELENS_TELEMETRY_ENABLED");
        std::env::remove_var("CODELENS_TELEMETRY_PATH");
    }

    assert!(TelemetryWriter::from_env().is_none());

    unsafe {
        if let Some(val) = prev_sym_enabled {
            std::env::set_var("SYMBIOTE_TELEMETRY_ENABLED", val);
        }
        if let Some(val) = prev_sym_path {
            std::env::set_var("SYMBIOTE_TELEMETRY_PATH", val);
        }
        if let Some(val) = prev_enabled {
            std::env::set_var("CODELENS_TELEMETRY_ENABLED", val);
        }
        if let Some(val) = prev_path {
            std::env::set_var("CODELENS_TELEMETRY_PATH", val);
        }
    }
}

#[test]
fn telemetry_writer_from_env_prefers_symbiote_path() {
    let _guard = crate::env_compat::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let prev_sym_path = std::env::var("SYMBIOTE_TELEMETRY_PATH").ok();
    let prev_path = std::env::var("CODELENS_TELEMETRY_PATH").ok();
    unsafe {
        std::env::set_var("SYMBIOTE_TELEMETRY_PATH", "/tmp/symbiote-telemetry.jsonl");
        std::env::set_var("CODELENS_TELEMETRY_PATH", "/tmp/codelens-telemetry.jsonl");
    }

    let writer = TelemetryWriter::from_env().expect("telemetry writer should be configured");
    assert_eq!(
        writer.path(),
        std::path::Path::new("/tmp/symbiote-telemetry.jsonl")
    );

    unsafe {
        match prev_sym_path {
            Some(val) => std::env::set_var("SYMBIOTE_TELEMETRY_PATH", val),
            None => std::env::remove_var("SYMBIOTE_TELEMETRY_PATH"),
        }
        match prev_path {
            Some(val) => std::env::set_var("CODELENS_TELEMETRY_PATH", val),
            None => std::env::remove_var("CODELENS_TELEMETRY_PATH"),
        }
    }
}

#[test]
fn telemetry_writer_from_env_accepts_symbiote_enabled_flag() {
    let _guard = crate::env_compat::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let prev_sym_enabled = std::env::var("SYMBIOTE_TELEMETRY_ENABLED").ok();
    let prev_enabled = std::env::var("CODELENS_TELEMETRY_ENABLED").ok();
    let prev_sym_path = std::env::var("SYMBIOTE_TELEMETRY_PATH").ok();
    let prev_path = std::env::var("CODELENS_TELEMETRY_PATH").ok();
    unsafe {
        std::env::set_var("SYMBIOTE_TELEMETRY_ENABLED", "1");
        std::env::remove_var("CODELENS_TELEMETRY_ENABLED");
        std::env::remove_var("SYMBIOTE_TELEMETRY_PATH");
        std::env::remove_var("CODELENS_TELEMETRY_PATH");
    }

    let writer =
        TelemetryWriter::from_env().expect("symbiote enabled flag should configure telemetry");
    assert_eq!(
        writer.path(),
        std::path::Path::new(".codelens/telemetry/tool_usage.jsonl")
    );

    unsafe {
        match prev_sym_enabled {
            Some(val) => std::env::set_var("SYMBIOTE_TELEMETRY_ENABLED", val),
            None => std::env::remove_var("SYMBIOTE_TELEMETRY_ENABLED"),
        }
        match prev_enabled {
            Some(val) => std::env::set_var("CODELENS_TELEMETRY_ENABLED", val),
            None => std::env::remove_var("CODELENS_TELEMETRY_ENABLED"),
        }
        match prev_sym_path {
            Some(val) => std::env::set_var("SYMBIOTE_TELEMETRY_PATH", val),
            None => std::env::remove_var("SYMBIOTE_TELEMETRY_PATH"),
        }
        match prev_path {
            Some(val) => std::env::set_var("CODELENS_TELEMETRY_PATH", val),
            None => std::env::remove_var("CODELENS_TELEMETRY_PATH"),
        }
    }
}
