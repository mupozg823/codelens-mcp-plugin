use serde_json::{Map, Value, json};

use super::budget::TRUNCATED_RESULT_SIZE_CHARS;
use super::payload_compact::{TEXT_CHANNEL_MAX_ARRAY_ITEMS, summarize_structured_content};

/// Headroom the stage-5 stub scaffold + enriched recovery_hint occupy on top
/// of the embedded `data_preview`. The preview cap must leave this much room
/// under the host's truncated-result cap, or the host clips the stub mid-JSON
/// and the agent loses compression_stage, recovery_hint AND preview at once —
/// strictly worse than a bare stub.
const STAGE5_STUB_HEADROOM_CHARS: usize = 5_000;

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
///
/// `payload` is the raw response envelope (`resp.data`). It is used ONLY at
/// stage 5 and ONLY when `structured_content` is `None` (a no-schema tool):
/// the depth-0 payload summary then feeds both the `data_preview` and the
/// hint-enrichment signals, so hosts that ignore `structuredContent` still
/// degrade to a usable SUMMARY instead of a bare error stub = total data loss.
///
/// Stage-5 text finalization is DEFERRED to the end of this function: the stub
/// must embed the ENRICHED recovery_hint, so `enrich_recovery_hint_for_signals`
/// runs here (before `finalize_stage5_text_stub`) rather than in the caller.
pub(crate) fn bounded_result_payload(
    mut text: String,
    mut structured_content: Option<Value>,
    payload: Option<&Value>,
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

    // Stage-5 deferred pieces. The text stub is finalized after hint
    // enrichment (below), so stage 5 records its `data_preview` candidate and
    // the signal source it enriched from instead of emitting the final text
    // inline. `None` for stages 1–4.
    let mut stage5_preview: Option<Value> = None;
    let mut stage5_signal: Option<Value> = None;

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
        // Stage 5: hard truncation — summarize structured to minimal skeleton.
        stage = 5;
        // Summarize structured content for the `structuredContent` channel
        // exactly as today (schema tools only). No-schema tools keep `None`
        // here — the structuredContent gating at the caller is unchanged.
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 0));
        }
        // Signal source feeding BOTH the enriched hint and `data_preview`: the
        // summarized structured content when a schema exists; otherwise a
        // depth-0 summary of the raw payload. Hosts that ignore
        // `structuredContent` (Claude Code, issue #4427) receive ONLY the text
        // channel — a bare stub here means total data loss even though the
        // depth-0 summary (arrays clipped to 3) usually fits well under budget.
        let signal = structured_content.clone().or_else(|| {
            payload
                .filter(|value| !value.is_null())
                .map(|value| summarize_structured_content(value, 0))
        });
        if let Some(signal) = signal.as_ref() {
            // Two caps, both mandatory: the token-budget-derived cap, AND the
            // host's fixed truncated-result cap minus stub headroom. The
            // budget cap alone is unsafe — at effective_budget > ~8.3K tokens
            // budget*3 exceeds the 25K-char host cap, and a preview that fits
            // the former but not the latter gets clipped mid-JSON by the host
            // (unparseable, total loss on the text-only channel).
            let preview_cap_chars = effective_budget
                .saturating_mul(3)
                .min(TRUNCATED_RESULT_SIZE_CHARS - STAGE5_STUB_HEADROOM_CHARS);
            let fits = serde_json::to_string(signal)
                .map(|serialized| serialized.len() <= preview_cap_chars)
                .unwrap_or(false);
            if fits {
                stage5_preview = Some(signal.clone());
            }
        }
        stage5_signal = signal;
        // text stub finalized below, after hint enrichment.
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

    // Enrich the recovery hint from structured signals BEFORE the stage-5 text
    // stub is finalized, so the stub carries the FINAL hint. Stages < 4
    // early-return inside the enricher. Stages 2–4 use the summarized
    // structured content (unchanged); stage 5 uses `stage5_signal`, which falls
    // back to the payload-derived summary so no-schema tools get the same
    // T5/S2 retargeting.
    let info = info.map(|info| {
        let signal_source = if stage == 5 {
            stage5_signal.as_ref()
        } else {
            structured_content.as_ref()
        };
        enrich_recovery_hint_for_signals(info, signal_source)
    });

    if stage == 5 {
        text = finalize_stage5_text_stub(
            info.as_ref(),
            stage5_preview.as_ref(),
            payload_estimate,
            effective_budget,
        );
    }

    (text, structured_content, info)
}

/// Finalize the stage-5 text stub — the SINGLE source of truth for its JSON.
///
/// Built AFTER `enrich_recovery_hint_for_signals` so it always carries the
/// FINAL recovery_hint. Hosts that ignore `structuredContent` (Claude Code,
/// issue #4427) see ONLY this text channel, so it degrades to a SUMMARY
/// (`data_preview` + enriched `recovery_hint`, no `error`) whenever a preview
/// fits, and reserves the `error` framing for genuine total loss (no preview).
fn finalize_stage5_text_stub(
    info: Option<&TruncationInfo>,
    preview: Option<&Value>,
    payload_estimate: usize,
    effective_budget: usize,
) -> String {
    let mut stub = json!({
        "success": true,
        "truncated": true,
        "compression_stage": 5,
        "token_estimate": payload_estimate,
        "effective_budget": effective_budget,
    });
    if let Some(info) = info {
        stub["recovery_hint"] = Value::String(info.recovery_hint.clone());
    }
    match preview {
        Some(preview) => {
            // A usable preview exists → this is a summary, not an error. The
            // narrowing guidance already lives in `recovery_hint`.
            stub["data_preview"] = preview.clone();
        }
        None => {
            // Genuine total loss: even the depth-0 skeleton exceeded the cap.
            // Keep the explicit error framing so the summary case stays
            // distinguishable from real data loss.
            stub["error"] = Value::String(format!(
                "Response too large ({payload_estimate} tokens, budget {effective_budget}). Narrow with path, max_tokens, or depth."
            ));
        }
    }
    serde_json::to_string(&stub)
        .unwrap_or_else(|_| "{\"success\":false,\"truncated\":true}".to_owned())
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

/// Rewrite the clipping `recovery_hint` when structured signals show the
/// generic budget-narrowing guidance is misleading. Two tool-aware passes,
/// applied in order to the same `TruncationInfo`:
///
/// 1. **Stage-5 artifact over-promise (T5).** The stage-5 hint advertises
///    `get_analysis_section`, but that handle only resolves results persisted
///    in `artifact_store` — the report / analysis-job family, which all route
///    through `build_handle_payload` and therefore carry an `analysis_id`
///    (with `available_sections` / `section_handles`). Primitive symbol tools
///    (`find_symbol`, `find_referencing_symbols`, `get_callers`, …) never write
///    an artifact, so pointing them at `get_analysis_section` is a dead path.
///    When a clipped payload carries no artifact handle, retarget the hint to
///    an executable narrowing action naming the concrete omitted count.
/// 2. **`unresolved_only` extractor gap (S2).** When the call-graph extractor
///    produced only `unresolved` edges, retrying with smaller bounds cannot
///    recover edges it never discovered — append a grep cue naming the symbol.
///
/// No-op for stages < 4 or missing structured content — the existing hint is
/// correct there.
pub(crate) fn enrich_recovery_hint_for_signals(
    mut info: TruncationInfo,
    structured_content: Option<&Value>,
) -> TruncationInfo {
    if info.stage < 4 {
        return info;
    }
    let Some(content) = structured_content.and_then(|v| v.as_object()) else {
        return info;
    };

    // Pass 1 (T5): the stage-5 hint over-promises `get_analysis_section`. Only
    // retarget when the payload is *not* an artifact AND actually clipped an
    // array (a concrete `<field>_omitted_count` marker survives summarization);
    // if nothing was clipped there is nothing to recover and the generic hint
    // is harmless. The artifact-handle check is deliberately data-driven — this
    // seam only receives the payload, not the tool name, and the handle's
    // presence is the exact precondition `get_analysis_section` needs, so it
    // needs no hardcoded tool list and stays correct for future artifact tools.
    if info.stage == 5
        && !payload_produces_analysis_artifact(content)
        && let Some((field, omitted)) = largest_omitted_marker(content)
    {
        info.recovery_hint = clipped_primitive_recovery_hint(
            info.original_payload_estimate,
            info.effective_budget,
            &field,
            omitted,
        );
    }

    // Pass 2 (S2): call-graph extractor produced only `unresolved` edges — a
    // coverage gap, not a budget problem. Append a grep fallback naming the
    // symbol so a retry with smaller bounds isn't the only cue.
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
    info.recovery_hint.push_str(&format!(
        " Call graph returned only `unresolved` edges and the result was clipped — `Grep '{symbol}'` directly may surface raw matches the tree-sitter call query missed."
    ));
    info
}

/// Does this (already-summarized) payload carry an analysis-artifact handle?
///
/// Only tools whose result is persisted in `artifact_store` return an
/// `analysis_id` — plus the `available_sections` / `section_handles` that
/// `get_analysis_section` resolves. That handle is exactly the precondition for
/// the `get_analysis_section` recovery path, so its presence in the payload is
/// the artifact signal: no hardcoded tool list, and it stays correct for any
/// future report/analysis tool (they all shape their payload through
/// `build_handle_payload`). The handle survives stage-5 summarization because
/// `summarize_structured_content` keeps every top-level key at depth 0.
fn payload_produces_analysis_artifact(content: &Map<String, Value>) -> bool {
    content.contains_key("analysis_id")
        || content.contains_key("available_sections")
        || content.contains_key("section_handles")
}

/// Stage-5 clipping hint for a primitive (non-artifact) symbol result: name the
/// concrete omitted count and an *executable* narrowing action, without the
/// `get_analysis_section` artifact promise (no artifact exists) or a pagination
/// cursor (none is emitted). Parameter names cover the primitive symbol family
/// (`max_matches` for `find_symbol`, `max_results` for
/// `find_referencing_symbols` / `get_callers`).
fn clipped_primitive_recovery_hint(
    estimate: usize,
    budget: usize,
    field: &str,
    omitted: u64,
) -> String {
    let max_items = TEXT_CHANNEL_MAX_ARRAY_ITEMS;
    // The summarizer length-caps string VALUES but clones object KEYS verbatim,
    // and `field` is a key — clamp it so a pathological key cannot balloon the
    // hint past the budget the stub exists to honor.
    let field = truncate_chars(field, 80);
    format!(
        "Response over budget ({estimate} tokens vs {budget}); result arrays clipped to {max_items} items each ({field} dropped {omitted}). \
         Narrow scope to recover them — pass path / a more specific symbol name, or lower the result cap (max_matches / max_results). \
         Primitive symbol results are not stored as artifacts and expose no pagination cursor, so there is no section handle to page through."
    )
}

fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

/// Largest `<field>_omitted_count` marker in the summarized payload, returned as
/// `(field, count)` with the suffix stripped. `summarize_structured_content`
/// records one per clipped array so the caller sees the original size (e.g.
/// `callers` clipped 287 → 3 leaves `callers_omitted_count: 284`). Picking the
/// largest surfaces the array the agent most likely wants to recover.
fn largest_omitted_marker(content: &Map<String, Value>) -> Option<(String, u64)> {
    content
        .iter()
        .filter_map(|(key, value)| {
            let field = key.strip_suffix("_omitted_count")?;
            let count = value.as_u64()?;
            (count > 0).then(|| (field.to_owned(), count))
        })
        .max_by_key(|(_, count)| *count)
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
mod stage5_preview_tests {
    use super::bounded_result_payload;
    use serde_json::json;

    #[test]
    fn stage5_embeds_bounded_data_preview_in_text_channel() {
        // #P0: hosts that ignore structuredContent must still receive the
        // depth-0 summary through the text channel instead of a bare stub.
        let structured = json!({
            "query": "rename a symbol",
            "symbols": [
                {"name": "alpha", "file_path": "a.rs"},
                {"name": "beta", "file_path": "b.rs"},
                {"name": "gamma", "file_path": "c.rs"},
                {"name": "delta", "file_path": "d.rs"},
            ],
            "count": 4,
        });
        let (text, structured_out, info) =
            bounded_result_payload("x".repeat(60_000), Some(structured), None, 12_000, 4_000, 0);
        assert_eq!(info.as_ref().map(|i| i.stage), Some(5));
        assert!(structured_out.is_some());
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["compression_stage"], 5);
        let preview = &parsed["data_preview"];
        assert!(
            preview.is_object(),
            "stage-5 text must carry a data_preview, got: {text}"
        );
        // Arrays are clipped to 3 by the summarizer, but the top hits survive.
        assert_eq!(preview["symbols"][0]["name"], "alpha");
        assert!(preview["symbols"].as_array().unwrap().len() <= 3);
    }

    #[test]
    fn stage5_omits_preview_when_even_summary_exceeds_cap() {
        // A skeleton that is itself enormous (many top-level keys) must fall
        // back to the bare stub rather than blow the budget it exists to honor.
        let mut huge = serde_json::Map::new();
        for i in 0..200 {
            huge.insert(format!("field_{i}"), json!("y".repeat(240)));
        }
        let (text, _, info) = bounded_result_payload(
            "x".repeat(60_000),
            Some(serde_json::Value::Object(huge)),
            None,
            12_000,
            4_000,
            0,
        );
        assert_eq!(info.as_ref().map(|i| i.stage), Some(5));
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(parsed.get("data_preview").is_none());
        assert!(
            text.len() <= 4_000,
            "bare stub must stay tiny, got {}",
            text.len()
        );
    }

    #[test]
    fn stage5_no_schema_tool_embeds_payload_preview() {
        // #4427 no-schema gap: `structured_content` is None (the tool has no
        // output_schema), but the raw payload carries a large array. The text
        // stub must derive `data_preview` from the payload — NOT a bare stub —
        // so hosts that ignore structuredContent still receive data.
        let payload = json!({
            "matches": (0..200)
                .map(|n| json!({"name": format!("sym_{n}")}))
                .collect::<Vec<_>>(),
            "count": 200,
        });
        let (text, structured_out, info) =
            bounded_result_payload("x".repeat(60_000), None, Some(&payload), 12_000, 4_000, 0);
        assert_eq!(info.as_ref().map(|i| i.stage), Some(5));
        assert!(
            structured_out.is_none(),
            "no-schema tools must NOT emit structuredContent (gating unchanged)"
        );
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["compression_stage"], 5);
        let preview = &parsed["data_preview"];
        assert!(
            preview.is_object(),
            "no-schema stage-5 text must carry a payload-derived data_preview, got: {text}"
        );
        assert!(preview["matches"].as_array().unwrap().len() <= 3);
        assert_eq!(
            preview["matches_omitted_count"],
            json!(197),
            "payload-derived preview must record the clipped count"
        );
    }

    #[test]
    fn stage5_text_stub_carries_enriched_hint() {
        // Primitive clip (matches_omitted_count, no artifact handle): the TEXT
        // stub must carry the ENRICHED recovery_hint — naming the omitted count
        // and dropping the get_analysis_section over-promise that only
        // artifact-backed tools support.
        let payload = json!({
            "function": "resolve_target",
            "matches": (0..130).map(|n| json!({"name": n})).collect::<Vec<_>>(),
            "count": 130,
        });
        let (text, _structured, _info) =
            bounded_result_payload("x".repeat(60_000), None, Some(&payload), 12_000, 4_000, 0);
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        let hint = parsed["recovery_hint"]
            .as_str()
            .expect("stage-5 text stub must carry a recovery_hint");
        assert!(
            hint.contains("127"),
            "hint must name the omitted count, got: {hint}"
        );
        assert!(
            hint.contains("matches"),
            "hint must name the clipped field, got: {hint}"
        );
        assert!(
            !hint.contains("get_analysis_section"),
            "primitive clip must drop the artifact over-promise, got: {hint}"
        );
    }

    #[test]
    fn stage5_text_stub_keeps_artifact_hint() {
        // Artifact payload (analysis_id + section list): the TEXT stub's
        // recovery_hint must still advertise get_analysis_section — a real
        // recovery path for artifact-backed reports.
        let payload = json!({
            "analysis_id": "analysis-abc123",
            "available_sections": ["overview", "findings"],
            "top_findings": (0..80)
                .map(|n| json!(format!("finding {n}")))
                .collect::<Vec<_>>(),
        });
        let (text, _structured, _info) =
            bounded_result_payload("x".repeat(60_000), None, Some(&payload), 12_000, 4_000, 0);
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        let hint = parsed["recovery_hint"]
            .as_str()
            .expect("stage-5 text stub must carry a recovery_hint");
        assert!(
            hint.contains("get_analysis_section"),
            "artifact payload must keep the get_analysis_section hint, got: {hint}"
        );
    }

    #[test]
    fn stage5_with_preview_has_no_error_key() {
        // Degrade-to-summary: when a preview fits, the stub is a summary, not an
        // error — success/truncated stay true and the `error` key is dropped.
        let payload = json!({
            "matches": (0..50).map(|n| json!({"name": n})).collect::<Vec<_>>(),
            "count": 50,
        });
        let (text, _s, _i) =
            bounded_result_payload("x".repeat(60_000), None, Some(&payload), 12_000, 4_000, 0);
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(
            parsed.get("data_preview").is_some(),
            "a preview must be present for this payload: {text}"
        );
        assert!(
            parsed.get("error").is_none(),
            "degrade-to-summary: no error key when a preview exists, got: {text}"
        );
        assert_eq!(parsed["success"], json!(true));
        assert_eq!(parsed["truncated"], json!(true));
    }

    #[test]
    fn stage5_preview_cap_respects_truncated_host_result_cap() {
        // The host clips truncated responses at TRUNCATED_RESULT_SIZE_CHARS
        // regardless of token budget. At effective_budget=16_000 the
        // budget-derived cap (48_000 chars) exceeds the host cap — a ~30KB
        // depth-0 summary "fits" the budget cap but would be cut mid-JSON by
        // the host (unparseable, total loss). The preview must be refused and
        // the final stub stay parseable and under the host cap.
        let mut wide = serde_json::Map::new();
        for i in 0..160 {
            wide.insert(format!("section_{i}"), json!("y".repeat(180)));
        }
        let payload = serde_json::Value::Object(wide);
        let (text, _s, info) =
            bounded_result_payload("x".repeat(400_000), None, Some(&payload), 60_000, 16_000, 0);
        assert_eq!(info.as_ref().map(|i| i.stage), Some(5));
        let parsed: serde_json::Value = serde_json::from_str(&text)
            .expect("stage-5 stub must stay parseable JSON under the host cap");
        assert!(
            parsed.get("data_preview").is_none(),
            "a summary larger than the host cap headroom must not be embedded"
        );
        assert!(
            text.len() <= super::super::budget::TRUNCATED_RESULT_SIZE_CHARS,
            "final stub ({} chars) must stay within the truncated host cap",
            text.len()
        );
    }

    #[test]
    fn stage5_final_stub_stays_within_truncated_host_cap_with_preview() {
        // A preview just under the cap plus scaffold + enriched hint must still
        // serialize under the host's truncated-result cap — the headroom
        // constant exists exactly for that scaffold.
        let payload = json!({
            "function": "resolve_target",
            "matches": (0..400)
                .map(|n| json!({"name": format!("symbol_number_{n}"), "file": format!("src/module_{n}.rs")}))
                .collect::<Vec<_>>(),
            "count": 400,
            "confidence_basis": "unresolved_only",
        });
        let (text, _s, info) =
            bounded_result_payload("x".repeat(400_000), None, Some(&payload), 60_000, 16_000, 0);
        assert_eq!(info.as_ref().map(|i| i.stage), Some(5));
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(
            parsed.get("data_preview").is_some(),
            "this compact summary must fit the preview cap: {text}"
        );
        assert!(
            text.len() <= super::super::budget::TRUNCATED_RESULT_SIZE_CHARS,
            "stub with preview + enriched hint ({} chars) must stay within the host cap",
            text.len()
        );
    }

    #[test]
    fn stage5_bare_stub_keeps_error_key() {
        // No preview fits (the depth-0 skeleton itself blows the cap) → genuine
        // data loss keeps the explicit error framing.
        let mut huge = serde_json::Map::new();
        for i in 0..200 {
            huge.insert(format!("field_{i}"), json!("y".repeat(240)));
        }
        let (text, _s, _i) = bounded_result_payload(
            "x".repeat(60_000),
            None,
            Some(&serde_json::Value::Object(huge)),
            12_000,
            4_000,
            0,
        );
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(
            parsed.get("data_preview").is_none(),
            "no preview should fit for this payload: {text}"
        );
        assert!(
            parsed.get("error").is_some(),
            "bare stub (total loss) must keep the error key, got: {text}"
        );
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

#[cfg(test)]
mod tool_aware_recovery_hint_tests {
    use super::{
        TruncationInfo, enrich_recovery_hint_for_signals, largest_omitted_marker,
        payload_produces_analysis_artifact, recovery_hint_for_stage,
    };
    use serde_json::json;

    fn stage5_info() -> TruncationInfo {
        TruncationInfo {
            stage: 5,
            original_payload_estimate: 12_000,
            effective_budget: 4_000,
            recovery_hint: recovery_hint_for_stage(5, 12_000, 4_000),
        }
    }

    #[test]
    fn stage5_default_hint_promises_analysis_section() {
        // Guard the premise this track fixes: the tool-agnostic stage-5 hint
        // advertises `get_analysis_section`, which only artifact tools support.
        assert!(recovery_hint_for_stage(5, 12_000, 4_000).contains("get_analysis_section"));
    }

    #[test]
    fn pathological_field_key_is_clamped_in_hint() {
        // The summarizer caps string VALUES but clones object KEYS verbatim,
        // and the hint embeds a key as `field` — a pathological key must not
        // balloon the hint past the budget the stub exists to honor.
        let long_key = "k".repeat(50_000);
        let structured = json!({
            format!("{long_key}_omitted_count"): 42,
            long_key.clone(): [1, 2, 3],
        });
        let enriched = enrich_recovery_hint_for_signals(stage5_info(), Some(&structured));
        assert!(
            enriched.recovery_hint.len() < 1_000,
            "hint must stay bounded for pathological keys, got {} chars",
            enriched.recovery_hint.len()
        );
        assert!(enriched.recovery_hint.contains("42"));
    }

    #[test]
    fn primitive_clip_retargets_hint_without_analysis_section() {
        // find_symbol-style clip: a symbol result whose `matches` array was
        // clipped (concrete `matches_omitted_count`) and carries no artifact
        // handle. The over-promise must go and an executable action take over.
        let structured = json!({
            "function": "resolve_target",
            "matches": [{"name": "a"}, {"name": "b"}, {"name": "c"}],
            "matches_omitted_count": 120,
        });
        let enriched = enrich_recovery_hint_for_signals(stage5_info(), Some(&structured));
        let hint = &enriched.recovery_hint;
        assert!(
            !hint.contains("get_analysis_section"),
            "primitive clip must drop the artifact over-promise, got: {hint}"
        );
        assert!(
            hint.contains("max_matches") || hint.contains("max_results"),
            "must offer an executable result-cap narrowing action, got: {hint}"
        );
        assert!(
            hint.contains("path"),
            "must offer scope narrowing, got: {hint}"
        );
        assert!(
            hint.contains("120") && hint.contains("matches"),
            "must name the concrete omitted count and field, got: {hint}"
        );
    }

    #[test]
    fn report_clip_keeps_analysis_section_hint() {
        // report-style clip: an artifact payload (analysis_id + sections) whose
        // `top_findings` array was clipped. `get_analysis_section` is a real
        // recovery path here, so the existing hint must survive verbatim.
        let base = recovery_hint_for_stage(5, 12_000, 4_000);
        let structured = json!({
            "analysis_id": "analysis-abc123",
            "available_sections": ["overview", "findings", "evidence"],
            "top_findings": ["a", "b", "c"],
            "top_findings_omitted_count": 40,
        });
        let enriched = enrich_recovery_hint_for_signals(stage5_info(), Some(&structured));
        assert_eq!(
            enriched.recovery_hint, base,
            "artifact-producing tool must keep the get_analysis_section hint unchanged"
        );
        assert!(enriched.recovery_hint.contains("get_analysis_section"));
    }

    #[test]
    fn section_handles_alone_marks_payload_as_artifact() {
        // Handle detection must not hinge on a single key name.
        let structured = json!({
            "section_handles": [{"section": "overview"}],
            "rows": ["a", "b", "c"],
            "rows_omitted_count": 9,
        });
        let enriched = enrich_recovery_hint_for_signals(stage5_info(), Some(&structured));
        assert!(
            enriched.recovery_hint.contains("get_analysis_section"),
            "a `section_handles` payload is artifact-backed — keep the hint"
        );
    }

    #[test]
    fn primitive_clip_combines_retarget_and_grep_cue() {
        // get_callers-style clip that is *also* `unresolved_only`: both passes
        // fire — no artifact over-promise, plus the grep fallback naming symbol.
        let structured = json!({
            "function": "register_route",
            "callers": [{"name": "a"}, {"name": "b"}, {"name": "c"}],
            "callers_omitted_count": 284,
            "confidence_basis": "unresolved_only",
        });
        let enriched = enrich_recovery_hint_for_signals(stage5_info(), Some(&structured));
        let hint = &enriched.recovery_hint;
        assert!(
            !hint.contains("get_analysis_section"),
            "primitive clip must drop the artifact over-promise, got: {hint}"
        );
        assert!(
            hint.contains("Grep") && hint.contains("register_route"),
            "unresolved_only grep cue must still append, got: {hint}"
        );
        assert!(
            hint.contains("284"),
            "must carry the omitted count, got: {hint}"
        );
    }

    #[test]
    fn primitive_clip_without_omitted_marker_leaves_hint() {
        // No array was actually clipped (no `_omitted_count`) → nothing to
        // recover, so the generic hint is harmless and left intact. Documents
        // the intentional gate on a concrete omitted count.
        let base = recovery_hint_for_stage(5, 12_000, 4_000);
        let structured = json!({
            "function": "resolve_target",
            "matches": [{"name": "a"}],
        });
        let enriched = enrich_recovery_hint_for_signals(stage5_info(), Some(&structured));
        assert_eq!(enriched.recovery_hint, base);
    }

    #[test]
    fn artifact_detection_helper_matches_handle_keys() {
        let artifact = json!({"analysis_id": "x"}).as_object().unwrap().clone();
        let primitive = json!({"callers": []}).as_object().unwrap().clone();
        assert!(payload_produces_analysis_artifact(&artifact));
        assert!(!payload_produces_analysis_artifact(&primitive));
    }

    #[test]
    fn largest_omitted_marker_picks_the_biggest_and_strips_suffix() {
        let content = json!({
            "callers_omitted_count": 284,
            "imports_omitted_count": 12,
            "zero_omitted_count": 0,
        })
        .as_object()
        .unwrap()
        .clone();
        assert_eq!(
            largest_omitted_marker(&content),
            Some(("callers".to_owned(), 284))
        );
    }
}
