use crate::protocol::ToolCallResponse;
use serde_json::{Map, Value};

use super::payload_compact::{
    copy_summarized_field, insert_if_present, summarize_text_data_for_response,
};

#[cfg(test)]
use super::payload_compact::{
    TEXT_CHANNEL_MAX_ARRAY_ITEMS, summarize_structured_content, summarize_text_object,
};

#[cfg(test)]
use super::truncation::{
    TruncationInfo, bounded_result_payload, enrich_recovery_hint_for_signals,
    recovery_hint_for_stage,
};

pub(crate) fn text_payload_for_response(
    resp: &ToolCallResponse,
    structured_content: Option<&Value>,
    lean: bool,
) -> String {
    if let Some(slim) = slim_text_payload_for_async_job(resp, structured_content) {
        return serde_json::to_string(&slim).unwrap_or_else(|_| {
            "{\"success\":false,\"error\":\"serialization failed\"}".to_owned()
        });
    }
    // Async handles get a slim payload that cherry-picks essential fields.
    if let Some(slim) = slim_text_payload_for_async_handle(resp, structured_content) {
        return serde_json::to_string(&slim).unwrap_or_else(|_| {
            "{\"success\":false,\"error\":\"serialization failed\"}".to_owned()
        });
    }
    // Pretty-print the full response as structured JSON with readable formatting.
    // Agents get valid JSON (parseability preserved) but with indentation and
    // newlines instead of a single flat line.
    format_structured_response(resp, lean)
}

/// Format a ToolCallResponse as pretty-printed JSON.
/// Preserves JSON validity for parsing while being much more readable than
/// a single-line blob. Strips redundant metadata fields that waste tokens.
///
/// When `lean` is set (lean response contract), the constant `schema_version`
/// marker is omitted — it repeats identically on every call and carries no
/// per-response signal.
fn format_structured_response(resp: &ToolCallResponse, lean: bool) -> String {
    // Build a clean output object with only the fields agents need.
    let mut out = serde_json::Map::new();

    out.insert("success".to_owned(), Value::Bool(resp.success));
    if !lean {
        out.insert("schema_version".to_owned(), Value::String("1.0".to_owned()));
    }

    // Error message (if present)
    if let Some(ref err) = resp.error {
        out.insert("error".to_owned(), Value::String(err.clone()));
    }

    // Structured recovery hint — actionable state, kept in both the full
    // and lean contracts (agents parse it instead of the error string).
    if let Some(ref hint) = resp.recovery_hint
        && let Ok(value) = serde_json::to_value(hint)
    {
        out.insert("recovery_hint".to_owned(), value);
    }

    // Header: compact metadata on key fields only
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
    if let Some(truncated) = resp
        .data
        .as_ref()
        .and_then(|data| data.get("truncated"))
        .and_then(|value| value.as_bool())
    {
        out.insert("truncated".to_owned(), Value::Bool(truncated));
    }

    // Data: summarized mirror of the structured payload for text-only clients.
    // The full machine-readable body remains in `structuredContent`.
    // Opt-in: CODELENS_VERBOSE_TEXT=1 copies the full data (no summarization),
    // so human UIs that only render `content[0].text` see the same payload
    // the agent sees in `structuredContent`.
    if let Some(ref data) = resp.data {
        let data_value =
            if crate::env_compat::env_var_bool("CODELENS_VERBOSE_TEXT").unwrap_or(false) {
                data.clone()
            } else {
                summarize_text_data_for_response(data)
            };
        out.insert("data".to_owned(), data_value);
    }

    // Suggested next tools (preserve original key for compatibility)
    if let Some(ref tools) = resp.suggested_next_tools {
        out.insert(
            "suggested_next_tools".to_owned(),
            serde_json::to_value(tools).unwrap_or(Value::Array(vec![])),
        );
        // Reasons as separate map (agents can read both)
        if let Some(ref reasons) = resp.suggestion_reasons
            && let Ok(v) = serde_json::to_value(reasons)
        {
            out.insert("suggestion_reasons".to_owned(), v);
        }
        // Additive concrete-args companion — skipped when empty.
        if let Some(ref calls) = resp.suggested_next_calls
            && !calls.is_empty()
            && let Ok(v) = serde_json::to_value(calls)
        {
            out.insert("suggested_next_calls".to_owned(), v);
        }
    }

    // CI/batch mode: check if routing hint is Async (analysis handles)
    // or if the response is very small — use compact JSON for efficiency.
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

fn slim_text_payload_for_async_job(
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

fn slim_text_payload_for_async_handle(
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

    let mut text_data = Map::new();
    copy_summarized_field(&mut text_data, data, "analysis_id");
    copy_summarized_field(&mut text_data, data, "summary");
    copy_summarized_field(&mut text_data, data, "readiness");
    copy_summarized_field(&mut text_data, data, "readiness_score");
    copy_summarized_field(&mut text_data, data, "overlapping_claims");
    copy_summarized_field(&mut text_data, data, "risk_level");
    copy_summarized_field(&mut text_data, data, "blocker_count");
    copy_summarized_field(&mut text_data, data, "reused");
    copy_summarized_field(&mut text_data, data, "cache_hit_tier");
    copy_summarized_field(&mut text_data, data, "summary_resource");
    copy_summarized_field(&mut text_data, data, "section_handles");
    copy_summarized_field(&mut text_data, data, "next_actions");
    if !text_data.is_empty() {
        payload.insert("data".to_owned(), Value::Object(text_data));
    }

    Some(Value::Object(payload))
}

#[cfg(test)]
mod text_channel_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lean_text_channel_omits_constant_schema_version() {
        use crate::protocol::BackendKind;
        use crate::tool_runtime::success_meta;
        let resp = ToolCallResponse::success(
            json!({"symbol": "x"}),
            success_meta(BackendKind::TreeSitter, 0.9),
        );

        let full = text_payload_for_response(&resp, None, false);
        let full_json: Value = serde_json::from_str(&full).expect("valid json");
        assert!(
            full_json.get("schema_version").is_some(),
            "full contract keeps schema_version"
        );

        let lean = text_payload_for_response(&resp, None, true);
        let lean_json: Value = serde_json::from_str(&lean).expect("valid json");
        assert!(
            lean_json.get("schema_version").is_none(),
            "lean contract omits the constant schema_version marker"
        );
        // The actual answer channel is untouched by the lean flag.
        assert!(
            lean_json.get("data").is_some(),
            "data survives lean text render"
        );
        assert!(lean.len() < full.len(), "lean render is smaller");
    }

    #[test]
    fn summarize_text_object_records_omitted_keys_when_capped() {
        // #211: a 12-key payload with the cap at 8 used to silently lose
        // the trailing 4 keys with only `truncated: true` as a signal.
        // The new contract surfaces each dropped key by name so the
        // caller can either widen the request (detail=full) or request
        // the missing fields directly.
        let mut map = Map::new();
        for i in 0..12u32 {
            map.insert(format!("key{i:02}"), json!(i));
        }
        let result = summarize_text_object(&map, 0);
        let obj = result.as_object().expect("object");
        assert_eq!(
            obj.get("truncated").and_then(Value::as_bool),
            Some(true),
            "cap-driven drop must mark truncated=true"
        );
        let omitted = obj
            .get("_omitted_keys")
            .and_then(Value::as_array)
            .expect("_omitted_keys array present after cap drop");
        assert_eq!(
            omitted.len(),
            4,
            "12 keys - 8 cap = 4 dropped, got {omitted:?}"
        );
        let omitted_names: Vec<&str> = omitted.iter().filter_map(Value::as_str).collect();
        assert_eq!(
            omitted_names,
            vec!["key08", "key09", "key10", "key11"],
            "omitted_keys must list the trailing keys in iteration order"
        );
    }

    #[test]
    fn summarize_text_object_no_omitted_keys_when_under_cap() {
        // Stage 1 / under-cap behavior must stay clean — no `truncated`
        // flag and no `_omitted_keys` entry when nothing was dropped.
        let mut map = Map::new();
        for i in 0..5u32 {
            map.insert(format!("key{i}"), json!(i));
        }
        let result = summarize_text_object(&map, 0);
        let obj = result.as_object().expect("object");
        assert!(
            obj.get("_omitted_keys").is_none(),
            "no _omitted_keys when under cap, got {obj:?}"
        );
        assert!(
            obj.get("truncated").is_none(),
            "no truncated when under cap"
        );
    }

    #[test]
    fn shrinking_array_child_flags_parent_truncated() {
        let payload = json!({
            "references": [1, 2, 3, 4, 5],
            "count": 5,
            "returned_count": 5,
            "sampled": false,
        });
        let summarized = summarize_text_data_for_response(&payload);
        let obj = summarized.as_object().expect("object");
        assert_eq!(
            obj.get("truncated").and_then(Value::as_bool),
            Some(true),
            "parent must be flagged when an array child was shrunk"
        );
        assert_eq!(
            obj.get("references")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(TEXT_CHANNEL_MAX_ARRAY_ITEMS)
        );
        assert_eq!(obj.get("count").and_then(Value::as_i64), Some(5));
        assert_eq!(obj.get("returned_count").and_then(Value::as_i64), Some(5));
    }

    #[test]
    fn short_array_leaves_parent_untruncated() {
        let payload = json!({
            "references": [1, 2],
            "count": 2,
            "returned_count": 2,
        });
        let summarized = summarize_text_data_for_response(&payload);
        let obj = summarized.as_object().expect("object");
        assert!(obj.get("truncated").is_none());
        assert_eq!(
            obj.get("references")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );
    }

    #[test]
    fn nested_object_array_shrink_flags_inner_parent() {
        let payload = json!({
            "outer": {
                "items": [1, 2, 3, 4],
                "total": 4,
            }
        });
        let summarized = summarize_text_data_for_response(&payload);
        let inner = summarized
            .get("outer")
            .and_then(Value::as_object)
            .expect("outer object");
        assert_eq!(inner.get("truncated").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn bounded_payload_passthrough_returns_no_truncation_info() {
        // Stage 1 passthrough: payload well below budget should report
        // no compression so callers can keep the fast path.
        let (text, structured, info) = bounded_result_payload(
            r#"{"ok":true}"#.to_owned(),
            Some(json!({"ok": true})),
            None, // payload (unused off stage 5)
            10,   // payload tokens
            1000, // budget
            0,    // medium effort
        );
        assert_eq!(text, r#"{"ok":true}"#);
        assert!(info.is_none(), "stage 1 must not surface truncation_info");
        assert!(structured.is_some());
    }

    #[test]
    fn bounded_payload_stage_5_emits_truncation_info_with_recovery_hint() {
        // Stage 5 (over-budget) is the dogfood case: Flask `route` extracted
        // 287 callers but the response showed 3, with `truncated: true`
        // buried in the data envelope. The new contract surfaces a
        // top-level TruncationInfo with stage + estimate + budget +
        // human-readable recovery_hint.
        let (_text, _structured, info) = bounded_result_payload(
            "x".repeat(50_000),
            Some(json!({
                "callers": (0..300).map(|n| json!({"name": n})).collect::<Vec<_>>(),
                "count": 300,
            })),
            None,   // payload (schema tool — signal comes from structured_content)
            12_000, // payload tokens (over budget)
            4_000,  // budget
            10,     // high effort
        );
        let info = info.expect("stage 5 must emit truncation info");
        assert_eq!(info.stage, 5);
        assert_eq!(info.original_payload_estimate, 12_000);
        assert_eq!(info.effective_budget, 4_000);
        assert!(
            info.recovery_hint.contains("12000")
                && info.recovery_hint.contains("4000")
                && (info.recovery_hint.contains("file_path")
                    || info.recovery_hint.contains("max_results")),
            "recovery_hint should include both estimate, budget, and a recovery cue: {}",
            info.recovery_hint
        );
        let info_json = info.to_json();
        assert_eq!(info_json["compression_stage"], json!(5));
        assert_eq!(info_json["original_payload_estimate"], json!(12_000));
        assert_eq!(info_json["effective_budget"], json!(4_000));
        assert!(info_json["recovery_hint"].is_string());
    }

    #[test]
    fn bounded_payload_preserves_bootstrap_routing_recovery_arrays() {
        let structured = json!({
            "routing": {
                "preferred_entrypoints": [
                    "prepare_harness_session",
                    "review_changes",
                    "get_current_config",
                    "plan_safe_refactor",
                    "trace_request_path",
                ],
                "preferred_entrypoints_omitted": [
                    {
                        "tool": "plan_safe_refactor",
                        "reason": "not_in_active_surface",
                        "recommended_action": "switch_tool_surface",
                        "execution_policy": {
                            "execution_class": "analyze",
                            "risk": "low",
                            "cost_hint": "medium",
                            "concurrency_safe": true,
                        },
                        "tool_tier": "workflow",
                        "included_in": [
                            "preset:balanced",
                            "preset:full",
                            "planner-readonly",
                            "builder-minimal",
                        ],
                        "recommended_profile": "planner-readonly",
                    },
                    {
                        "tool": "trace_request_path",
                        "reason": "not_in_active_surface",
                        "recommended_action": "switch_tool_surface",
                        "execution_policy": {
                            "execution_class": "analyze",
                            "risk": "low",
                            "cost_hint": "medium",
                            "concurrency_safe": true,
                        },
                        "tool_tier": "workflow",
                        "included_in": ["preset:balanced", "preset:full", "builder-minimal"],
                        "recommended_profile": "builder-minimal",
                    },
                    {
                        "tool": "refresh_symbol_index",
                        "reason": "not_in_active_surface",
                        "recommended_action": "switch_tool_surface",
                        "execution_policy": {
                            "execution_class": "read",
                            "risk": "low",
                            "cost_hint": "low",
                            "concurrency_safe": true,
                        },
                        "tool_tier": "workflow",
                        "included_in": [
                            "preset:minimal",
                            "preset:balanced",
                            "preset:full",
                            "builder-minimal",
                        ],
                        "recommended_profile": "builder-minimal",
                    },
                    {
                        "tool": "this_tool_does_not_exist_xyz",
                        "reason": "unknown_tool",
                        "recommended_action": "fix_preferred_entrypoint",
                    },
                ],
            },
        });

        let (_text, summarized, info) = bounded_result_payload(
            "{\"success\":true}".to_owned(),
            Some(structured),
            None,
            9_100,
            10_000,
            0,
        );

        assert_eq!(info.expect("stage 3 compression").stage, 3);
        let routing = &summarized.expect("structured content")["routing"];
        assert_eq!(
            routing["preferred_entrypoints"]
                .as_array()
                .expect("preferred_entrypoints")
                .len(),
            5,
            "routing recovery must retain the full requested entrypoint list"
        );
        let omitted = routing["preferred_entrypoints_omitted"]
            .as_array()
            .expect("preferred_entrypoints_omitted");
        assert_eq!(
            omitted.len(),
            4,
            "routing recovery must retain every omitted entrypoint"
        );
        assert_eq!(
            omitted[0]["execution_policy"]["execution_class"],
            json!("analyze"),
            "known omitted entrypoints must keep execution metadata under compression"
        );
        assert_eq!(
            omitted[0]["tool_tier"],
            json!("workflow"),
            "known omitted entrypoints must keep tier metadata under compression"
        );
        assert_eq!(
            omitted[0]["included_in"]
                .as_array()
                .expect("included_in")
                .len(),
            4,
            "surface recovery list must not be clipped for routing metadata"
        );
        assert_eq!(
            omitted[3]["recommended_action"],
            json!("fix_preferred_entrypoint"),
            "unknown entrypoint recovery must survive compression"
        );
        assert!(
            routing
                .get("preferred_entrypoints_omitted_omitted_count")
                .is_none(),
            "preserved routing recovery arrays must not advertise synthetic omissions"
        );
    }

    #[test]
    fn array_clipping_records_omitted_count_marker() {
        // When an array of length N gets clipped to MAX_ARRAY_ITEMS=3,
        // the parent object should carry `<key>_omitted_count` so the
        // call site (e.g. `data.callers`) shows how much was dropped.
        let payload = json!({
            "callers": (0..287).map(|n| json!({"name": n})).collect::<Vec<_>>(),
            "count": 287,
        });
        let summarized = summarize_structured_content(&payload, 0);
        let obj = summarized.as_object().expect("object");
        let arr = obj
            .get("callers")
            .and_then(Value::as_array)
            .expect("callers array");
        assert_eq!(arr.len(), TEXT_CHANNEL_MAX_ARRAY_ITEMS);
        assert_eq!(
            obj.get("callers_omitted_count").and_then(Value::as_i64),
            Some((287 - TEXT_CHANNEL_MAX_ARRAY_ITEMS) as i64),
            "expected callers_omitted_count to record dropped items"
        );
    }

    #[test]
    fn unresolved_only_truncation_appends_grep_fallback_hint() {
        // S2: when call-graph extractor reports `confidence_basis:
        // "unresolved_only"` and the response was clipped at stage 4-5,
        // the recovery hint should name the symbol and suggest a direct
        // grep rather than telling the agent to retry with smaller bounds
        // (which won't help — the extractor missed the edges).
        let info = TruncationInfo {
            stage: 5,
            original_payload_estimate: 12_000,
            effective_budget: 4_000,
            recovery_hint: recovery_hint_for_stage(5, 12_000, 4_000),
        };
        let structured = json!({
            "function": "register_route",
            "callers": [{"name": "a"}, {"name": "b"}, {"name": "c"}],
            "confidence_basis": "unresolved_only",
        });
        let enriched = enrich_recovery_hint_for_signals(info, Some(&structured));
        assert!(
            enriched.recovery_hint.contains("Grep")
                && enriched.recovery_hint.contains("register_route"),
            "expected grep-fallback cue with symbol name, got: {}",
            enriched.recovery_hint
        );
        assert!(
            enriched.recovery_hint.contains("file_path")
                || enriched.recovery_hint.contains("max_results"),
            "base recovery_hint must remain (got: {})",
            enriched.recovery_hint
        );
    }

    #[test]
    fn mixed_resolution_truncation_keeps_default_hint() {
        // Negative case: when the call graph has real evidence
        // (import_evidence / mixed), the budget-narrowing hint is the
        // correct guidance — do not append the grep-fallback cue.
        let base = recovery_hint_for_stage(5, 12_000, 4_000);
        let info = TruncationInfo {
            stage: 5,
            original_payload_estimate: 12_000,
            effective_budget: 4_000,
            recovery_hint: base.clone(),
        };
        let structured = json!({
            "function": "register_route",
            "callers": [{"name": "a"}],
            "confidence_basis": "import_evidence",
        });
        let enriched = enrich_recovery_hint_for_signals(info, Some(&structured));
        assert_eq!(enriched.recovery_hint, base);
    }

    #[test]
    fn enrichment_skipped_for_stage_below_four() {
        // Stage 2-3 already get tool-agnostic guidance and leave the
        // structured payload mostly intact, so the unresolved_only label
        // here is informational, not a wall to recovery. Don't enrich.
        let base = recovery_hint_for_stage(3, 9_000, 10_000);
        let info = TruncationInfo {
            stage: 3,
            original_payload_estimate: 9_000,
            effective_budget: 10_000,
            recovery_hint: base.clone(),
        };
        let structured = json!({
            "function": "register_route",
            "confidence_basis": "unresolved_only",
        });
        let enriched = enrich_recovery_hint_for_signals(info, Some(&structured));
        assert_eq!(enriched.recovery_hint, base);
    }

    #[test]
    fn short_arrays_get_no_omitted_marker() {
        // Backward-compat: arrays at or below MAX_ARRAY_ITEMS keep the
        // current shape — no extra `_omitted_count` marker is added.
        let payload = json!({
            "callers": [{"name": "a"}, {"name": "b"}],
            "count": 2,
        });
        let summarized = summarize_structured_content(&payload, 0);
        let obj = summarized.as_object().expect("object");
        assert!(obj.get("callers_omitted_count").is_none());
    }
}
