use super::super::*;
use crate::env_compat::TEST_ENV_LOCK;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn make_unique_telemetry_path(label: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "codelens-telemetry-{label}-{}-{now}.jsonl",
        std::process::id()
    ))
}

fn read_persisted_events(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).expect("valid telemetry jsonl line"))
        .collect()
}

struct TelemetryEnvGuard {
    prev_sym_enabled: Option<String>,
    prev_enabled: Option<String>,
    prev_sym_path: Option<String>,
    prev_path: Option<String>,
}

impl TelemetryEnvGuard {
    fn install(
        sym_enabled: Option<&str>,
        enabled: Option<&str>,
        sym_path: Option<&Path>,
        path: Option<&Path>,
    ) -> Self {
        let guard = Self {
            prev_sym_enabled: std::env::var("SYMBIOTE_TELEMETRY_ENABLED").ok(),
            prev_enabled: std::env::var("CODELENS_TELEMETRY_ENABLED").ok(),
            prev_sym_path: std::env::var("SYMBIOTE_TELEMETRY_PATH").ok(),
            prev_path: std::env::var("CODELENS_TELEMETRY_PATH").ok(),
        };
        // SAFETY: telemetry env tests serialize access under TEST_ENV_LOCK.
        unsafe {
            match sym_enabled {
                Some(value) => std::env::set_var("SYMBIOTE_TELEMETRY_ENABLED", value),
                None => std::env::remove_var("SYMBIOTE_TELEMETRY_ENABLED"),
            }
            match enabled {
                Some(value) => std::env::set_var("CODELENS_TELEMETRY_ENABLED", value),
                None => std::env::remove_var("CODELENS_TELEMETRY_ENABLED"),
            }
            match sym_path {
                Some(value) => std::env::set_var("SYMBIOTE_TELEMETRY_PATH", value),
                None => std::env::remove_var("SYMBIOTE_TELEMETRY_PATH"),
            }
            match path {
                Some(value) => std::env::set_var("CODELENS_TELEMETRY_PATH", value),
                None => std::env::remove_var("CODELENS_TELEMETRY_PATH"),
            }
        }
        guard
    }
}

impl Drop for TelemetryEnvGuard {
    fn drop(&mut self) {
        // SAFETY: telemetry env tests serialize access under TEST_ENV_LOCK.
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

#[test]
fn unique_telemetry_path() {
    let first = make_unique_telemetry_path("first");
    let second = make_unique_telemetry_path("second");

    assert_ne!(first, second);
    assert_eq!(
        first.extension().and_then(|value| value.to_str()),
        Some("jsonl")
    );
}

#[test]
fn telemetry_writer_persists_single_event() {
    let path = make_unique_telemetry_path("single");
    let writer = TelemetryWriter::with_path(path.clone());
    writer.append_event(&PersistedEvent {
        timestamp_ms: 10,
        tool: "find_symbol",
        surface: "builder-minimal",
        elapsed_ms: 25,
        tokens: 100,
        success: true,
        truncated: false,
        session_id: Some("session-a"),
        phase: Some("review"),
        target_paths: None,
        suggested_next_tools: &[],
        delegate_hint_trigger: None,
        delegate_target_tool: None,
        delegate_handoff_id: None,
        handoff_id: None,
    });

    let events = read_persisted_events(&path);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["tool"], "find_symbol");
    assert_eq!(events[0]["session_id"], "session-a");
    assert_eq!(events[0]["phase"], "review");

    let _ = std::fs::remove_file(path);
}

#[test]
fn telemetry_writer_appends_multiple_events_in_order() {
    let path = make_unique_telemetry_path("append");
    let writer = TelemetryWriter::with_path(path.clone());
    for (timestamp_ms, tool) in [(10, "find_symbol"), (20, "impact_report")] {
        writer.append_event(&PersistedEvent {
            timestamp_ms,
            tool,
            surface: "builder-minimal",
            elapsed_ms: 10,
            tokens: 0,
            success: true,
            truncated: false,
            session_id: None,
            phase: None,
            target_paths: None,
            suggested_next_tools: &[],
            delegate_hint_trigger: None,
            delegate_target_tool: None,
            delegate_handoff_id: None,
            handoff_id: None,
        });
    }

    let events = read_persisted_events(&path);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["tool"], "find_symbol");
    assert_eq!(events[1]["tool"], "impact_report");

    let _ = std::fs::remove_file(path);
}

#[test]
fn telemetry_writer_persists_delegate_hint_fields() {
    let path = make_unique_telemetry_path("delegate");
    let writer = TelemetryWriter::with_path(path.clone());
    let suggested_next_tools = vec!["review_changes".to_owned(), "impact_report".to_owned()];
    writer.append_event(&PersistedEvent {
        timestamp_ms: 10,
        tool: "analyze_change_request",
        surface: "planner-readonly",
        elapsed_ms: 30,
        tokens: 500,
        success: true,
        truncated: true,
        session_id: Some("session-a"),
        phase: Some("review"),
        target_paths: None,
        suggested_next_tools: &suggested_next_tools,
        delegate_hint_trigger: Some("large-change"),
        delegate_target_tool: Some("review_changes"),
        delegate_handoff_id: Some("delegate-123"),
        handoff_id: Some("handoff-456"),
    });

    let events = read_persisted_events(&path);
    assert_eq!(events[0]["delegate_hint_trigger"], "large-change");
    assert_eq!(events[0]["delegate_target_tool"], "review_changes");
    assert_eq!(events[0]["delegate_handoff_id"], "delegate-123");
    assert_eq!(events[0]["handoff_id"], "handoff-456");
    assert_eq!(events[0]["suggested_next_tools"][0], "review_changes");
    assert_eq!(events[0]["suggested_next_tools"][1], "impact_report");

    let _ = std::fs::remove_file(path);
}

#[test]
fn registry_persists_record_call_when_writer_enabled() {
    let path = make_unique_telemetry_path("registry");
    let reg = ToolMetricsRegistry::new_with_writer(Some(TelemetryWriter::with_path(path.clone())));
    let target_paths = vec!["src/lib.rs".to_owned(), "src/main.rs".to_owned()];
    let suggested_next_tools = vec!["review_changes".to_owned()];

    reg.record_call_with_targets_for_session(
        "analyze_change_request",
        55,
        true,
        900,
        "planner-readonly",
        true,
        Some("review"),
        Some("session-a"),
        &target_paths,
        CallTelemetryHints {
            suggested_next_tools: &suggested_next_tools,
            delegate_hint_trigger: Some("large-change"),
            delegate_target_tool: Some("review_changes"),
            delegate_handoff_id: Some("delegate-123"),
            handoff_id: Some("handoff-456"),
        },
    );

    let events = read_persisted_events(&path);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["tool"], "analyze_change_request");
    assert_eq!(events[0]["surface"], "planner-readonly");
    assert_eq!(events[0]["session_id"], "session-a");
    assert_eq!(events[0]["target_paths"][0], "src/lib.rs");
    assert_eq!(events[0]["target_paths"][1], "src/main.rs");
    assert_eq!(events[0]["delegate_target_tool"], "review_changes");

    let _ = std::fs::remove_file(path);
}

#[test]
fn registry_without_writer_is_noop_for_persistence() {
    let path = make_unique_telemetry_path("noop");
    let reg = ToolMetricsRegistry::new_with_writer(None);

    reg.record_call("find_symbol", 20, true);

    assert!(!path.exists());
}

#[test]
fn telemetry_writer_from_env_disabled_by_default() {
    let _env_lock = TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let _guard = TelemetryEnvGuard::install(None, None, None, None);

    assert!(TelemetryWriter::from_env().is_none());
}

#[test]
fn telemetry_writer_from_env_prefers_symbiote_path() {
    let _env_lock = TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let sym_path = make_unique_telemetry_path("symbiote");
    let code_path = make_unique_telemetry_path("codelens");
    let _guard = TelemetryEnvGuard::install(None, None, Some(&sym_path), Some(&code_path));

    let writer = TelemetryWriter::from_env().expect("telemetry writer from env");
    assert_eq!(writer.path(), sym_path.as_path());
}

#[test]
fn telemetry_writer_from_env_accepts_symbiote_enabled_flag() {
    let _env_lock = TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let _guard = TelemetryEnvGuard::install(Some("1"), None, None, None);

    let writer = TelemetryWriter::from_env().expect("telemetry writer from enabled env");
    assert_eq!(
        writer.path(),
        Path::new(".codelens/telemetry/tool_usage.jsonl")
    );
}
