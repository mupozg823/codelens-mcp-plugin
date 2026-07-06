use crate::AppState;
use crate::mutation_gate::is_verifier_source_tool;
use serde_json::{Map, Value, json};

pub(crate) fn attach_index_freshness(
    name: &str,
    state: &AppState,
    payload: &mut Value,
    lean: bool,
) -> bool {
    if !matches!(
        name,
        "find_referencing_symbols"
            | "find_symbol"
            | "get_ranked_context"
            | "get_symbols_overview"
            | "onboard_project"
    ) {
        return false;
    }

    let Some(obj) = payload.as_object_mut() else {
        return false;
    };
    if obj.contains_key("index_freshness") {
        return false;
    }
    let Some(freshness) = crate::tool_runtime::index_freshness_hint(state) else {
        return false;
    };

    let refresh_recommended = freshness
        .get("refresh_recommended")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let staleness_is_fresh = freshness
        .get("staleness_hint")
        .and_then(|v| v.as_str())
        .is_some_and(|hint| hint == "fresh");
    if super::should_attach_index_freshness(lean, staleness_is_fresh) {
        obj.insert("index_freshness".to_owned(), freshness);
    }
    refresh_recommended
}

pub(crate) fn record_verifier_preflight(
    name: &str,
    active_surface: &str,
    logical_session_id: &str,
    arguments: &Value,
    state: &AppState,
    payload: &mut Value,
) {
    if !is_verifier_source_tool(name) {
        return;
    }

    state.record_recent_preflight_from_payload(
        name,
        active_surface,
        logical_session_id,
        arguments,
        payload,
    );
    let Some(run_id) = arguments
        .get("orchestration_run_id")
        .and_then(|value| value.as_str())
    else {
        return;
    };

    let mutation_ready = payload
        .get("readiness")
        .and_then(|readiness| readiness.get("mutation_ready"))
        .and_then(|value| value.as_str())
        .unwrap_or("caution");
    let blocker_count = payload
        .get("blocker_count")
        .and_then(|value| value.as_u64())
        .unwrap_or_else(|| {
            payload
                .get("blockers")
                .and_then(|value| value.as_array())
                .map(|items| items.len() as u64)
                .unwrap_or_default()
        });
    let passed = blocker_count == 0 && mutation_ready != "blocked";
    let event_name = if passed {
        "verification_passed"
    } else {
        "verification_failed"
    };
    let to_state = if passed { "completed" } else { "failed" };
    let mut extra = Map::new();
    extra.insert("tool".to_owned(), json!(name));
    extra.insert(
        "verification_analysis_id".to_owned(),
        payload.get("analysis_id").cloned().unwrap_or(Value::Null),
    );
    extra.insert("mutation_ready".to_owned(), json!(mutation_ready));
    extra.insert("blocker_count".to_owned(), json!(blocker_count));
    if let Some(event) = state.append_orchestration_event_for_current_scope(
        logical_session_id,
        run_id,
        event_name,
        Some("mutation_applied"),
        to_state,
        extra,
    ) && let Some(obj) = payload.as_object_mut()
    {
        obj.entry("orchestration_event".to_owned()).or_insert(event);
    }
}
