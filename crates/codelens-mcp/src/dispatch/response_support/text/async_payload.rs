use super::summarize::summarize_structured_content;
use crate::protocol::ToolCallResponse;
use serde_json::{Map, Value};

pub(super) fn slim_text_payload_for_async_job(
    resp: &ToolCallResponse,
    structured_content: Option<&Value>,
) -> Option<Value> {
    let data = structured_content?.as_object()?;
    data.get("job_id")?.as_str()?;

    let mut payload = Map::new();
    payload.insert("success".to_owned(), Value::Bool(resp.success));
    insert_if_present(
        &mut payload,
        "backend_used",
        resp.backend_used.clone().map(Value::String),
    );
    insert_if_present(&mut payload, "confidence", resp.confidence.map(Value::from));
    insert_if_present(
        &mut payload,
        "degraded_reason",
        resp.degraded_reason.clone().map(Value::String),
    );
    insert_if_present(
        &mut payload,
        "source",
        resp.source
            .as_ref()
            .and_then(|source| serde_json::to_value(source).ok()),
    );
    insert_if_present(
        &mut payload,
        "freshness",
        resp.freshness
            .as_ref()
            .and_then(|freshness| serde_json::to_value(freshness).ok()),
    );
    insert_if_present(&mut payload, "partial", resp.partial.map(Value::Bool));
    insert_if_present(
        &mut payload,
        "token_estimate",
        resp.token_estimate.map(Value::from),
    );
    insert_if_present(
        &mut payload,
        "budget_hint",
        resp.budget_hint.clone().map(Value::String),
    );
    insert_if_present(
        &mut payload,
        "routing_hint",
        resp.routing_hint
            .as_ref()
            .and_then(|hint| serde_json::to_value(hint).ok()),
    );
    insert_if_present(&mut payload, "elapsed_ms", resp.elapsed_ms.map(Value::from));
    insert_if_present(
        &mut payload,
        "suggested_next_tools",
        resp.suggested_next_tools
            .as_ref()
            .and_then(|tools| serde_json::to_value(tools).ok()),
    );
    insert_if_present(
        &mut payload,
        "suggestion_reasons",
        resp.suggestion_reasons
            .as_ref()
            .and_then(|reasons| serde_json::to_value(reasons).ok()),
    );
    insert_if_present(
        &mut payload,
        "suggested_next_calls",
        resp.suggested_next_calls
            .as_ref()
            .filter(|calls| !calls.is_empty())
            .and_then(|calls| serde_json::to_value(calls).ok()),
    );
    insert_if_present(
        &mut payload,
        "decisions",
        (!resp.decisions.is_empty()).then(|| Value::Array(resp.decisions.clone())),
    );

    let mut text_data = Map::new();
    copy_summarized_field(&mut text_data, data, "job_id");
    copy_summarized_field(&mut text_data, data, "kind");
    copy_summarized_field(&mut text_data, data, "status");
    copy_summarized_field(&mut text_data, data, "progress");
    copy_summarized_field(&mut text_data, data, "current_step");
    copy_summarized_field(&mut text_data, data, "profile_hint");
    copy_summarized_field(&mut text_data, data, "analysis_id");
    copy_summarized_field(&mut text_data, data, "estimated_sections");
    copy_summarized_field(&mut text_data, data, "summary_resource");
    copy_summarized_field(&mut text_data, data, "section_handles");
    copy_summarized_field(&mut text_data, data, "error");
    if !text_data.is_empty() {
        payload.insert("data".to_owned(), Value::Object(text_data));
    }

    Some(Value::Object(payload))
}

pub(super) fn slim_text_payload_for_async_handle(
    resp: &ToolCallResponse,
    structured_content: Option<&Value>,
) -> Option<Value> {
    let data = structured_content?.as_object()?;
    data.get("analysis_id")?.as_str()?;

    let mut payload = Map::new();
    payload.insert("success".to_owned(), Value::Bool(resp.success));
    insert_if_present(
        &mut payload,
        "backend_used",
        resp.backend_used.clone().map(Value::String),
    );
    insert_if_present(&mut payload, "confidence", resp.confidence.map(Value::from));
    insert_if_present(
        &mut payload,
        "degraded_reason",
        resp.degraded_reason.clone().map(Value::String),
    );
    insert_if_present(
        &mut payload,
        "source",
        resp.source
            .as_ref()
            .and_then(|source| serde_json::to_value(source).ok()),
    );
    insert_if_present(&mut payload, "partial", resp.partial.map(Value::Bool));
    insert_if_present(
        &mut payload,
        "freshness",
        resp.freshness
            .as_ref()
            .and_then(|freshness| serde_json::to_value(freshness).ok()),
    );
    insert_if_present(
        &mut payload,
        "staleness_ms",
        resp.staleness_ms.map(Value::from),
    );
    insert_if_present(
        &mut payload,
        "token_estimate",
        resp.token_estimate.map(Value::from),
    );
    insert_if_present(
        &mut payload,
        "suggested_next_tools",
        resp.suggested_next_tools
            .as_ref()
            .and_then(|tools| serde_json::to_value(tools).ok()),
    );
    insert_if_present(
        &mut payload,
        "budget_hint",
        resp.budget_hint.clone().map(Value::String),
    );
    insert_if_present(
        &mut payload,
        "routing_hint",
        resp.routing_hint
            .as_ref()
            .and_then(|hint| serde_json::to_value(hint).ok()),
    );
    insert_if_present(&mut payload, "elapsed_ms", resp.elapsed_ms.map(Value::from));
    insert_if_present(
        &mut payload,
        "suggestion_reasons",
        resp.suggestion_reasons
            .as_ref()
            .and_then(|reasons| serde_json::to_value(reasons).ok()),
    );
    insert_if_present(
        &mut payload,
        "suggested_next_calls",
        resp.suggested_next_calls
            .as_ref()
            .filter(|calls| !calls.is_empty())
            .and_then(|calls| serde_json::to_value(calls).ok()),
    );
    insert_if_present(
        &mut payload,
        "decisions",
        (!resp.decisions.is_empty()).then(|| Value::Array(resp.decisions.clone())),
    );

    let mut text_data = Map::new();
    copy_summarized_field(&mut text_data, data, "analysis_id");
    copy_summarized_field(&mut text_data, data, "summary");
    copy_summarized_field(&mut text_data, data, "readiness");
    copy_summarized_field(&mut text_data, data, "readiness_score");
    copy_summarized_field(&mut text_data, data, "overlapping_claims");
    copy_summarized_field(&mut text_data, data, "risk_level");
    copy_summarized_field(&mut text_data, data, "blocker_count");
    copy_summarized_field(&mut text_data, data, "reused");
    copy_summarized_field(&mut text_data, data, "summary_resource");
    copy_summarized_field(&mut text_data, data, "section_handles");
    copy_summarized_field(&mut text_data, data, "next_actions");
    if !text_data.is_empty() {
        payload.insert("data".to_owned(), Value::Object(text_data));
    }

    Some(Value::Object(payload))
}

fn insert_if_present(target: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        target.insert(key.to_owned(), value);
    }
}

fn copy_summarized_field(target: &mut Map<String, Value>, source: &Map<String, Value>, key: &str) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_owned(), summarize_structured_content(value, 0));
    }
}
