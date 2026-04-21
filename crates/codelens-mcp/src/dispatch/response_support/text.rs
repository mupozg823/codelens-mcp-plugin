mod async_payload;
mod summarize;
#[cfg(test)]
mod tests;

use self::async_payload::{slim_text_payload_for_async_handle, slim_text_payload_for_async_job};
pub(super) use self::summarize::summarize_structured_content;
use self::summarize::summarize_text_data_for_response;
use crate::protocol::ToolCallResponse;
use serde_json::Value;

pub(crate) fn text_payload_for_response(
    resp: &ToolCallResponse,
    structured_content: Option<&Value>,
) -> String {
    text_payload_for_response_with_shape(resp, structured_content, false)
}

pub(crate) fn text_payload_for_response_with_shape(
    resp: &ToolCallResponse,
    structured_content: Option<&Value>,
    primitive: bool,
) -> String {
    if let Some(slim) = slim_text_payload_for_async_job(resp, structured_content) {
        return serde_json::to_string(&slim).unwrap_or_else(|_| {
            "{\"success\":false,\"error\":\"serialization failed\"}".to_owned()
        });
    }
    if let Some(slim) = slim_text_payload_for_async_handle(resp, structured_content) {
        return serde_json::to_string(&slim).unwrap_or_else(|_| {
            "{\"success\":false,\"error\":\"serialization failed\"}".to_owned()
        });
    }
    if primitive {
        return format_primitive_response(resp);
    }
    format_structured_response(resp)
}

fn format_primitive_response(resp: &ToolCallResponse) -> String {
    let mut out = serde_json::Map::new();
    out.insert("success".to_owned(), Value::Bool(resp.success));
    if let Some(ref err) = resp.error {
        out.insert("error".to_owned(), Value::String(err.clone()));
    }
    if let Some(ref data) = resp.data {
        out.insert("data".to_owned(), data.clone());
    }
    if let Some(ref tools) = resp.suggested_next_tools
        && !tools.is_empty()
    {
        out.insert(
            "suggested_next_tools".to_owned(),
            serde_json::to_value(tools).unwrap_or(Value::Array(vec![])),
        );
    }
    if !resp.decisions.is_empty() {
        out.insert("decisions".to_owned(), Value::Array(resp.decisions.clone()));
    }
    serde_json::to_string(&Value::Object(out))
        .unwrap_or_else(|_| "{\"success\":false,\"error\":\"serialization failed\"}".to_owned())
}

fn format_structured_response(resp: &ToolCallResponse) -> String {
    let mut out = serde_json::Map::new();

    out.insert("success".to_owned(), Value::Bool(resp.success));
    out.insert("schema_version".to_owned(), Value::String("1.0".to_owned()));

    if let Some(ref err) = resp.error {
        out.insert("error".to_owned(), Value::String(err.clone()));
    }
    if let Some(ref backend) = resp.backend_used {
        out.insert("backend_used".to_owned(), Value::String(backend.clone()));
    }
    if let Some(c) = resp.confidence {
        out.insert("confidence".to_owned(), Value::from(c));
    }
    if let Some(ms) = resp.elapsed_ms {
        out.insert("elapsed_ms".to_owned(), Value::from(ms));
    }
    if let Some(ref hint) = resp.budget_hint {
        out.insert("budget_hint".to_owned(), Value::String(hint.clone()));
    }
    if let Some(ref hint) = resp.routing_hint
        && let Ok(value) = serde_json::to_value(hint)
    {
        out.insert("routing_hint".to_owned(), value);
    }
    if let Some(token_estimate) = resp.token_estimate {
        out.insert("token_estimate".to_owned(), Value::from(token_estimate));
    }
    if let Some(partial) = resp.partial {
        out.insert("partial".to_owned(), Value::Bool(partial));
    }
    if let Some(ref degraded_reason) = resp.degraded_reason {
        out.insert(
            "degraded_reason".to_owned(),
            Value::String(degraded_reason.clone()),
        );
    }
    out.insert("decisions".to_owned(), Value::Array(resp.decisions.clone()));
    if let Some(truncated) = resp
        .data
        .as_ref()
        .and_then(|data| data.get("truncated"))
        .and_then(|value| value.as_bool())
    {
        out.insert("truncated".to_owned(), Value::Bool(truncated));
    }

    if let Some(ref data) = resp.data {
        let data_value = if std::env::var("CODELENS_VERBOSE_TEXT")
            .map(|v| matches!(v.as_str(), "1" | "true" | "on" | "yes"))
            .unwrap_or(false)
        {
            data.clone()
        } else {
            summarize_text_data_for_response(data)
        };
        out.insert("data".to_owned(), data_value);
    }

    if let Some(ref tools) = resp.suggested_next_tools {
        out.insert(
            "suggested_next_tools".to_owned(),
            serde_json::to_value(tools).unwrap_or(Value::Array(vec![])),
        );
        if let Some(ref reasons) = resp.suggestion_reasons
            && let Ok(v) = serde_json::to_value(reasons)
        {
            out.insert("suggestion_reasons".to_owned(), v);
        }
        if let Some(ref calls) = resp.suggested_next_calls
            && !calls.is_empty()
            && let Ok(v) = serde_json::to_value(calls)
        {
            out.insert("suggested_next_calls".to_owned(), v);
        }
    }

    let use_compact = out
        .get("data")
        .and_then(|d| d.as_object())
        .is_some_and(|obj| obj.contains_key("job_id") || obj.contains_key("analysis_id"));

    if use_compact {
        serde_json::to_string(&Value::Object(out))
            .unwrap_or_else(|_| "{\"error\":\"serialization failed\"}".to_owned())
    } else {
        serde_json::to_string_pretty(&Value::Object(out))
            .unwrap_or_else(|_| "{\"error\":\"serialization failed\"}".to_owned())
    }
}
