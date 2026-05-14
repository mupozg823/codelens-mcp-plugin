use serde_json::{Value, json};

use super::payload_compact::summarize_structured_content;

/// Adaptive compression based on OpenDev 5-stage strategy (arxiv:2603.05344).
/// Thresholds are adjusted by effort level offset (Low=-10, Medium=0, High=+10).
/// Stage 1: <75% budget → pass through
/// Stage 2: 75-85% → summarize structured content (depth=1)
/// Stage 3: 85-95% → aggressive summarize (depth=0)
/// Stage 4: 95-100% → drop structured content entirely
/// Stage 5: >100% → hard truncation to error payload
///
/// Returns `(text, structured_content, truncation_info)`. When the payload
/// passes through (stage 1), `truncation_info` is `None`. Stages 2–5 emit
/// a `TruncationInfo` carrying the stage, original payload size estimate,
/// effective budget, and a human-readable recovery hint.
pub(crate) fn bounded_result_payload(
    mut text: String,
    mut structured_content: Option<Value>,
    payload_estimate: usize,
    effective_budget: usize,
    effort_offset: i32,
) -> (String, Option<Value>, Option<TruncationInfo>) {
    let usage_pct = payload_estimate
        .checked_mul(100)
        .and_then(|v| v.checked_div(effective_budget))
        .unwrap_or(100);
    // Apply effort offset to thresholds (High effort delays compression)
    let t1 = (75i32 + effort_offset).clamp(50, 90) as usize;
    let t2 = (85i32 + effort_offset).clamp(60, 95) as usize;
    let t3 = (95i32 + effort_offset).clamp(70, 100) as usize;
    let t4 = (100i32 + effort_offset).clamp(80, 110) as usize;

    let max_chars = effective_budget * 8;
    let stage: u8;

    if usage_pct <= t1 {
        // Stage 1: pass through
        stage = 1;
    } else if usage_pct <= t2 {
        // Stage 2: light summarization
        stage = 2;
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 1));
        }
    } else if usage_pct <= t3 {
        // Stage 3: aggressive summarization
        stage = 3;
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 0));
        }
    } else if usage_pct <= t4 {
        // Stage 4: aggressive summarize structured + truncate text if needed
        stage = 4;
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 0));
        }
        if text.len() > max_chars {
            let byte_idx = text
                .char_indices()
                .nth(max_chars)
                .map(|(i, _)| i)
                .unwrap_or(text.len());
            text.truncate(byte_idx);
            text.push_str("...[truncated]");
        }
    } else {
        // Stage 5: hard truncation — summarize structured to minimal skeleton
        stage = 5;
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 0));
        }
        text = serde_json::to_string(&json!({
            "success": true,
            "truncated": true,
            "compression_stage": 5,
            "error": format!(
                "Response too large ({} tokens, budget {}). Narrow with path, max_tokens, or depth.",
                payload_estimate, effective_budget
            ),
            "token_estimate": payload_estimate,
        }))
        .unwrap_or_else(|_| "{\"success\":false,\"truncated\":true}".to_owned());
    }

    let info = if stage >= 2 {
        Some(TruncationInfo {
            stage,
            original_payload_estimate: payload_estimate,
            effective_budget,
            recovery_hint: recovery_hint_for_stage(stage, payload_estimate, effective_budget),
        })
    } else {
        None
    };
    (text, structured_content, info)
}

/// Top-level metadata describing what the adaptive compressor did.
///
/// This is surfaced into the structured response so that an agent can
/// detect "I asked for X but only got a summarized view" without having
/// to reach into the data envelope. Stage 1 (pass-through) returns
/// `None` from `bounded_result_payload`; stages 2–5 emit one of these.
#[derive(Debug, Clone)]
pub(crate) struct TruncationInfo {
    pub stage: u8,
    pub original_payload_estimate: usize,
    pub effective_budget: usize,
    pub recovery_hint: String,
}

impl TruncationInfo {
    pub fn to_json(&self) -> Value {
        json!({
            "compression_stage": self.stage,
            "original_payload_estimate": self.original_payload_estimate,
            "effective_budget": self.effective_budget,
            "recovery_hint": self.recovery_hint,
        })
    }
}

/// When the call-graph extractor produced only `unresolved` edges and the
/// response was clipped, the default `recovery_hint` ("narrow with file_path
/// or smaller max_results") is misleading — the issue is extractor coverage,
/// not budget. Append a tool-aware grep cue that names the actual symbol so
/// an agent can fall back to direct text search instead of retrying the same
/// low-confidence call-graph query with smaller bounds.
///
/// No-op for stages < 4, missing structured content, or non-`unresolved_only`
/// confidence bases — the existing hint is correct in those cases.
pub(crate) fn enrich_recovery_hint_for_signals(
    info: TruncationInfo,
    structured_content: Option<&Value>,
) -> TruncationInfo {
    if info.stage < 4 {
        return info;
    }
    let Some(content) = structured_content.and_then(|v| v.as_object()) else {
        return info;
    };
    let basis = content
        .get("confidence_basis")
        .and_then(Value::as_str)
        .unwrap_or("");
    if basis != "unresolved_only" {
        return info;
    }
    let symbol = content
        .get("function")
        .or_else(|| content.get("symbol_name"))
        .or_else(|| content.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("the symbol");
    let mut recovery_hint = info.recovery_hint;
    recovery_hint.push_str(&format!(
        " Call graph returned only `unresolved` edges and the result was clipped — `Grep '{symbol}'` directly may surface raw matches the tree-sitter call query missed."
    ));
    TruncationInfo {
        recovery_hint,
        ..info
    }
}

pub(super) fn recovery_hint_for_stage(stage: u8, estimate: usize, budget: usize) -> String {
    match stage {
        2 => format!(
            "Light summarization applied ({} of {} budget). Drill into a specific file or symbol for full detail.",
            estimate, budget
        ),
        3 => format!(
            "Aggressive summarization applied ({} of {} budget). Arrays clipped to 3 items each — use file_path / max_results to narrow scope.",
            estimate, budget
        ),
        4 => format!(
            "Near-budget summarization applied ({} of {} budget). Result arrays clipped — narrow scope with file_path or smaller max_results to recover the full set.",
            estimate, budget
        ),
        5 => format!(
            "Response over budget ({} tokens vs {}); structured arrays clipped to 3 items each. Use file_path / smaller max_results / get_analysis_section to recover items.",
            estimate, budget
        ),
        _ => "Compression applied".to_owned(),
    }
}

#[cfg(test)]
mod routing_hint_tier_tests {
    use super::super::routing_hint::routing_hint_for_payload;
    use crate::protocol::{
        AnalysisSource, BackendKind, Freshness, RoutingHint, ToolCallResponse, ToolResponseMeta,
    };
    use serde_json::json;

    fn resp_with_data(data: serde_json::Value) -> ToolCallResponse {
        let meta = ToolResponseMeta {
            backend_used: BackendKind::Hybrid.to_string(),
            confidence: 1.0,
            degraded_reason: None,
            source: AnalysisSource::Native,
            partial: false,
            freshness: Freshness::Live,
            staleness_ms: None,
        };
        ToolCallResponse::success(data, meta)
    }

    #[test]
    fn returns_cached_exact_when_tier_is_exact() {
        let resp = resp_with_data(json!({"reused": true, "cache_hit_tier": "exact"}));
        assert!(matches!(
            routing_hint_for_payload(&resp),
            RoutingHint::CachedExact
        ));
    }

    #[test]
    fn returns_cached_warm_when_tier_is_warm() {
        let resp = resp_with_data(json!({"reused": true, "cache_hit_tier": "warm"}));
        assert!(matches!(
            routing_hint_for_payload(&resp),
            RoutingHint::CachedWarm
        ));
    }

    #[test]
    fn returns_legacy_cached_when_reused_without_tier() {
        let resp = resp_with_data(json!({"reused": true}));
        assert!(matches!(
            routing_hint_for_payload(&resp),
            RoutingHint::Cached
        ));
    }

    #[test]
    fn returns_async_when_job_id_present() {
        let resp = resp_with_data(json!({"job_id": "j-1"}));
        assert!(matches!(
            routing_hint_for_payload(&resp),
            RoutingHint::Async
        ));
    }

    #[test]
    fn returns_sync_when_neither_cache_nor_async() {
        let resp = resp_with_data(json!({"foo": "bar"}));
        assert!(matches!(routing_hint_for_payload(&resp), RoutingHint::Sync));
    }
}
