use super::*;
use crate::operation::{OperationWorkClass, ResolvedOperation};

fn event<'a>(tool: &'a str, operation: ResolvedOperation<'a>) -> ToolCallEvent<'a> {
    ToolCallEvent {
        tool,
        operation,
        elapsed_ms: 1,
        tokens: 1,
        success: true,
        surface: "builder-minimal",
        truncated: false,
        phase: None,
        logical_session_id: Some("resolved-operation-test"),
        client_name: None,
        target_paths: &[],
        hints: CallTelemetryHints::default(),
    }
}

fn unique_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "codelens-issue388-{label}-{}-{}.jsonl",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn registry_records_resolved_operation_metadata() {
    let path = unique_path("resolved-operation");
    let registry =
        ToolMetricsRegistry::new_with_writer(Some(TelemetryWriter::with_path(path.clone())));
    registry.record_event(event(
        "search",
        ResolvedOperation::resolved("find_symbol", Some("symbol")).dispatched(),
    ));

    let session = registry.session_snapshot_for("resolved-operation-test");
    assert_eq!(session.core.total_calls, 1);
    assert_eq!(session.call_type.low_level_calls, 1);
    assert_eq!(session.call_type.composite_calls, 0);
    let invocation = &session.timeline[0];
    assert_eq!(invocation.tool, "search");
    assert_eq!(invocation.resolved_target.as_deref(), Some("find_symbol"));
    assert_eq!(invocation.mode.as_deref(), Some("symbol"));
    assert_eq!(invocation.work_class, OperationWorkClass::Primitive);
    assert_eq!(invocation.downstream_call_count, 1);

    let persisted: serde_json::Value = serde_json::from_str(
        std::fs::read_to_string(&path)
            .expect("read telemetry JSONL")
            .trim(),
    )
    .expect("parse telemetry JSONL");
    assert_eq!(persisted["tool"], "search");
    assert_eq!(persisted["resolved_target"], "find_symbol");
    assert_eq!(persisted["mode"], "symbol");
    assert_eq!(persisted["work_class"], "primitive");
    assert_eq!(persisted["downstream_call_count"], 1);

    let _ = std::fs::remove_file(path);
}

#[test]
fn resolved_composite_breaks_low_level_chain() {
    let registry = ToolMetricsRegistry::new_with_writer(None);
    registry.record_event(event(
        "search",
        ResolvedOperation::resolved("find_symbol", Some("symbol")).dispatched(),
    ));
    registry.record_event(event(
        "overview",
        ResolvedOperation::resolved("get_symbols_overview", Some("file")).dispatched(),
    ));
    registry.record_event(event(
        "overview",
        ResolvedOperation::resolved("explore_codebase", Some("explore")).dispatched(),
    ));
    registry.record_event(event(
        "graph",
        ResolvedOperation::resolved("get_callers", Some("callers")).dispatched(),
    ));

    let session = registry.session_snapshot();
    assert_eq!(session.call_type.low_level_calls, 3);
    assert_eq!(session.call_type.composite_calls, 1);
    assert_eq!(session.guidance.repeated_low_level_chain_count, 0);
}
