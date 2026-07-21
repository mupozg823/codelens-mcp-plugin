use crate::AppState;
use crate::mutation_gate::MutationGateAllowance;
use crate::protocol::{JsonRpcResponse, ToolCallResponse, ToolResponseMeta};
use crate::telemetry::{CallTelemetryHints, ToolCallEvent};
use crate::tool_defs::{ToolSurface, tool_definition};
use crate::tools;
use serde_json::Value;

use crate::dispatch::response_support::{
    apply_contextual_guidance, attach_index_freshness, bounded_result_payload, budget_hint,
    build_suggested_next_calls, compact_response_payload, delegate_hint_telemetry_fields,
    effective_budget_for_tool, inject_delegate_to_codex_builder_hint,
    max_result_size_chars_for_tool, record_verifier_preflight, routing_hint_for_payload,
    success_jsonrpc_response, text_payload_for_response, trim_scaffold_for_lean,
};

pub(crate) struct SuccessResponseInput<'a> {
    pub name: &'a str,
    pub payload: serde_json::Value,
    pub meta: ToolResponseMeta,
    pub state: &'a AppState,
    pub surface: ToolSurface,
    pub active_surface: &'a str,
    pub arguments: &'a serde_json::Value,
    pub logical_session_id: &'a str,
    pub recent_tools: Vec<String>,
    pub gate_allowance: Option<&'a MutationGateAllowance>,
    pub compact: bool,
    /// Lean response contract (scaffold-only thrift) — independent of the
    /// legacy `compact` data pruning.
    pub lean: bool,
    pub harness_phase: Option<&'a str>,
    pub request_budget: usize,
    pub start: std::time::Instant,
    pub id: Option<serde_json::Value>,
    /// Consecutive same-tool+args call count for doom-loop detection.
    pub doom_loop_count: usize,
    /// True when 3+ identical calls happen within 10 seconds (agent retry loop).
    pub doom_loop_rapid: bool,
}

pub(crate) fn build_success_response(input: SuccessResponseInput<'_>) -> JsonRpcResponse {
    let SuccessResponseInput {
        name,
        payload,
        meta,
        state,
        surface,
        active_surface,
        arguments,
        logical_session_id,
        recent_tools,
        gate_allowance,
        compact,
        lean,
        harness_phase,
        request_budget,
        start,
        id,
        doom_loop_count,
        doom_loop_rapid,
    } = input;

    let elapsed_ms = start.elapsed().as_millis();

    // Apply per-tool hard cap if defined (stricter than global budget)
    let effective_budget = effective_budget_for_tool(name, request_budget);
    let mut payload = payload;

    let refresh_recommended_from_freshness =
        attach_index_freshness(name, state, &mut payload, lean);
    record_verifier_preflight(
        name,
        active_surface,
        logical_session_id,
        arguments,
        state,
        &mut payload,
    );

    let had_caution = gate_allowance.map(|a| a.caution) == Some(true);
    if had_caution {
        state
            .metrics()
            .record_mutation_with_caution_for_session(Some(logical_session_id));
    }

    // Mutation allowed with caution = no fresh preflight was found
    let missing_preflight = had_caution;

    let has_output_schema = tool_definition(name)
        .and_then(|tool| tool.output_schema.as_ref())
        .is_some();
    let structured_content = has_output_schema.then(|| payload.clone());

    let mut resp = ToolCallResponse::success(payload, meta);

    let payload_estimate = serde_json::to_string(&resp.data)
        .map(|s| tools::estimate_tokens(&s))
        .unwrap_or(0);
    let mut hint = budget_hint(name, payload_estimate, effective_budget);
    if missing_preflight {
        hint = format!("{hint} Tip: run verify_change_readiness before mutations for safer edits.");
    }
    if doom_loop_count >= 3 {
        if doom_loop_rapid {
            hint = format!(
                "{hint} Rapid retry burst detected ({doom_loop_count}x in <10s). \
                 Use start_analysis_job for heavy analysis, or narrow scope with path/max_tokens."
            );
        } else {
            hint = format!(
                "{hint} Repeated low-level chain detected. Prefer verify_change_readiness, \
                 analyze_change_request, impact_report for compressed context."
            );
        }
    }
    resp.token_estimate = Some(payload_estimate);
    resp.budget_hint = Some(hint);
    resp.elapsed_ms = Some(elapsed_ms as u64);
    resp.routing_hint = Some(routing_hint_for_payload(&resp));

    let emitted_composite_guidance =
        apply_contextual_guidance(&mut resp, name, &recent_tools, harness_phase, surface);

    // Self-evolution: when doom-loop detected, override suggestions with alternative tools
    if doom_loop_count >= 3 {
        if doom_loop_rapid {
            // Rapid burst: suggest async path to break the retry loop
            resp.suggested_next_tools = Some(vec![
                "start_analysis_job".to_owned(),
                "analyze_change_request".to_owned(),
                "get_ranked_context".to_owned(),
            ]);
        } else {
            resp.suggested_next_tools = Some(vec![
                "verify_change_readiness".to_owned(),
                "analyze_change_request".to_owned(),
                "impact_report".to_owned(),
            ]);
        }
    }

    // PR (this commit, C): when the index is stale, surface
    // `refresh_symbol_index` at the top of suggested_next_tools so
    // the agent doesn't have to know to call it before retrying.
    // Idempotent: skip if it's already in the list.
    if refresh_recommended_from_freshness {
        match resp.suggested_next_tools.as_mut() {
            Some(tools) => {
                if !tools.iter().any(|t| t == "refresh_symbol_index") {
                    tools.insert(0, "refresh_symbol_index".to_owned());
                }
            }
            None => {
                resp.suggested_next_tools = Some(vec!["refresh_symbol_index".to_owned()]);
            }
        }
    }

    if let Some(ref next_tools) = resp.suggested_next_tools {
        resp.suggestion_reasons = Some(tools::suggestion_reasons_for(next_tools, name));
        let mut calls = build_suggested_next_calls(name, arguments, next_tools, resp.data.as_ref());
        let mut next_tools = next_tools.clone();
        inject_delegate_to_codex_builder_hint(
            name,
            arguments,
            resp.data.as_ref(),
            &mut next_tools,
            &mut calls,
            doom_loop_count,
            doom_loop_rapid,
        );
        resp.suggested_next_tools = Some(next_tools);
        if !calls.is_empty() {
            resp.suggested_next_calls = Some(calls);
        }
        resp.suggestion_reasons = resp
            .suggested_next_tools
            .as_ref()
            .map(|tools| tools::suggestion_reasons_for(tools, name));
    }

    if compact {
        compact_response_payload(&mut resp);
    }
    if lean {
        // Lean scaffold thrift: drop low-signal envelope fields (prose reasons,
        // telemetry, sync routing_hint, under-budget hints). Quality-neutral —
        // no code/symbol data is touched. `suggested_next_tools`/`_calls` and
        // any actionable budget_hint survive. Deliberately independent of the
        // legacy `compact` data pruning above.
        let budget_pct = if effective_budget == 0 {
            100
        } else {
            (payload_estimate as u64).saturating_mul(100) / effective_budget as u64
        };
        trim_scaffold_for_lean(&mut resp, budget_pct, doom_loop_count, missing_preflight);
    }

    let effort_offset = state.effort_level().compression_threshold_offset();
    let text = text_payload_for_response(&resp, structured_content.as_ref(), lean);
    // `bounded_result_payload` enriches the recovery hint internally (T5
    // artifact-over-promise retarget + S2 unresolved-only grep cue) so the
    // stage-5 TEXT stub is finalized AFTER enrichment and carries the FINAL
    // hint — hosts that ignore `structuredContent` (Claude Code, #4427) see it.
    // `resp.data` is passed so a no-schema tool (structured_content == None)
    // can still derive its `data_preview` + enrichment signals from the payload.
    let (text, mut structured_content, truncation_info) = bounded_result_payload(
        text,
        structured_content,
        resp.data.as_ref(),
        payload_estimate,
        effective_budget,
        effort_offset,
    );
    let truncated = truncation_info.is_some();
    // Surface the truncation envelope at the top level of structured_content
    // so an agent does not have to reach into the data envelope to discover
    // arrays were clipped. Pre-PR101 dogfood case (Flask `route` callers
    // 287→3) was a recall regression hidden by stage-5 compression.
    if let (Some(info), Some(Value::Object(map))) =
        (truncation_info.as_ref(), structured_content.as_mut())
    {
        map.insert("truncation_warning".to_owned(), info.to_json());
    }
    let suggested_next_tools = resp.suggested_next_tools.as_deref().unwrap_or(&[]);
    let handoff_id = arguments.get("handoff_id").and_then(|value| value.as_str());
    let (delegate_hint_trigger, delegate_target_tool, delegate_handoff_id) =
        delegate_hint_telemetry_fields(&resp);

    let target_paths = state.extract_target_paths(arguments);
    state.metrics().record_event(ToolCallEvent {
        tool: name,
        elapsed_ms: elapsed_ms as u64,
        tokens: payload_estimate,
        success: true,
        surface: active_surface,
        truncated,
        phase: harness_phase,
        logical_session_id: Some(logical_session_id),
        client_name: arguments
            .get("_session_client_name")
            .and_then(|value| value.as_str()),
        target_paths: &target_paths,
        hints: CallTelemetryHints {
            suggested_next_tools,
            delegate_hint_trigger,
            delegate_target_tool,
            delegate_handoff_id,
            handoff_id,
        },
    });
    if emitted_composite_guidance
        && !matches!(name, "get_tool_metrics" | "set_profile" | "set_preset")
    {
        state
            .metrics()
            .record_composite_guidance_emitted_for_session(name, Some(logical_session_id));
    }

    let max_result_size = max_result_size_chars_for_tool(name, truncated);
    success_jsonrpc_response(id, name, text, structured_content, Some(max_result_size))
}

#[cfg(test)]
mod no_schema_stage5_dispatch_tests {
    use super::*;
    use crate::protocol::BackendKind;
    use crate::tool_defs::{ToolPreset, ToolSurface};
    use crate::tool_runtime::success_meta;
    use codelens_engine::ProjectRoot;
    use serde_json::json;

    fn temp_state() -> crate::AppState {
        let dir = std::env::temp_dir().join(format!(
            "codelens-success-stage5-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".codelens")).unwrap();
        std::fs::write(
            dir.join(".codelens").join("principals.toml"),
            "[default]\nrole = \"Refactor\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("hello.txt"), "hello world\n").unwrap();
        let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
        crate::AppState::new_minimal(project, ToolPreset::Full)
    }

    #[test]
    fn no_schema_tool_stage5_text_stub_embeds_payload_preview_end_to_end() {
        // End-to-end through `build_success_response`: `search` has NO
        // output_schema, so `structured_content` is None. A large-array payload
        // over a tiny budget must (a) still emit NO structuredContent (the
        // gating at success.rs:95 is unchanged) and (b) put a payload-derived
        // `data_preview` + enriched `recovery_hint` into the TEXT stub — not a
        // bare error stub = total data loss.
        let state = temp_state();
        let payload = json!({
            "matches": (0..400)
                .map(|n| json!({"name": format!("sym_{n}"), "file_path": format!("src/m_{n}.rs")}))
                .collect::<Vec<_>>(),
            "count": 400,
        });
        let args = json!({});
        let response = build_success_response(SuccessResponseInput {
            name: "search",
            payload,
            meta: success_meta(BackendKind::TreeSitter, 0.9),
            state: &state,
            surface: ToolSurface::Preset(ToolPreset::Balanced),
            active_surface: "balanced",
            arguments: &args,
            logical_session_id: "test-session",
            recent_tools: Vec::new(),
            gate_allowance: None,
            compact: false,
            lean: false,
            harness_phase: None,
            request_budget: 400,
            start: std::time::Instant::now(),
            id: Some(json!(1)),
            doom_loop_count: 0,
            doom_loop_rapid: false,
        });

        let value = serde_json::to_value(&response).expect("serialize response");
        // No-schema tool: the structuredContent channel must stay absent.
        assert!(
            value["result"].get("structuredContent").is_none()
                || value["result"]["structuredContent"].is_null(),
            "no-schema tool must not emit structuredContent"
        );
        let text = value["result"]["content"][0]["text"]
            .as_str()
            .expect("text content present");
        let parsed: serde_json::Value =
            serde_json::from_str(text).expect("stage-5 text stub is valid JSON");
        assert_eq!(parsed["compression_stage"], json!(5), "stub: {text}");
        assert!(
            parsed["data_preview"].is_object(),
            "no-schema stage-5 stub must carry a payload-derived preview: {text}"
        );
        assert!(
            parsed["data_preview"]["matches"]
                .as_array()
                .expect("preview matches array")
                .len()
                <= 3
        );
        let hint = parsed["recovery_hint"]
            .as_str()
            .expect("enriched recovery_hint embedded in stub");
        assert!(
            hint.contains("matches"),
            "hint names the clipped field: {hint}"
        );
        assert!(
            !hint.contains("get_analysis_section"),
            "primitive payload drops the artifact over-promise: {hint}"
        );
        assert!(
            parsed.get("error").is_none(),
            "degrade-to-summary: no error key when a preview exists: {text}"
        );
    }
}
