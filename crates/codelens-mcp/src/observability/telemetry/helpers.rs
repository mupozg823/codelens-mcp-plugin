use super::types::{MAX_TIMELINE, has_low_level_chain, is_workflow_tool, push_latency_sample};
use super::{SessionMetrics, SurfaceMetrics, ToolInvocation, ToolMetrics};
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub(super) struct SessionCallRef<'a> {
    pub name: &'a str,
    pub elapsed_ms: u64,
    pub success: bool,
    pub tokens: usize,
    pub surface: &'a str,
    pub truncated: bool,
    pub phase: Option<&'a str>,
    pub target_paths: &'a [String],
}

pub(super) fn record_tool_call(
    map: &mut HashMap<String, ToolMetrics>,
    name: &str,
    elapsed_ms: u64,
    success: bool,
    tokens: usize,
    now: u64,
) {
    let entry = map.entry(name.to_owned()).or_default();
    entry.call_count += 1;
    if success {
        entry.success_count += 1;
    }
    entry.total_ms += elapsed_ms;
    entry.total_tokens += tokens;
    if elapsed_ms > entry.max_ms {
        entry.max_ms = elapsed_ms;
    }
    push_latency_sample(&mut entry.latency_samples, elapsed_ms);
    if !success {
        entry.error_count += 1;
    }
    entry.last_called_at = now;
}

pub(super) fn record_surface_call(
    surfaces: &mut HashMap<String, SurfaceMetrics>,
    surface: &str,
    elapsed_ms: u64,
    success: bool,
    tokens: usize,
    now: u64,
) {
    let entry = surfaces.entry(surface.to_owned()).or_default();
    entry.call_count += 1;
    if success {
        entry.success_count += 1;
    }
    entry.total_ms += elapsed_ms;
    entry.total_tokens += tokens;
    push_latency_sample(&mut entry.latency_samples, elapsed_ms);
    if !success {
        entry.error_count += 1;
    }
    entry.last_called_at = now;
}

pub(super) fn record_session_call(session: &mut SessionMetrics, call: SessionCallRef<'_>) {
    let previous = session.timeline.last().cloned();
    session.total_calls += 1;
    if call.success {
        session.success_count += 1;
    }
    session.total_ms += call.elapsed_ms;
    session.total_tokens += call.tokens;
    if call.name == "tools/list" {
        session.tools_list_tokens += call.tokens;
    }
    if is_workflow_tool(call.name) {
        session.composite_calls += 1;
    } else {
        session.low_level_calls += 1;
    }
    if !call.success {
        session.error_count += 1;
    }
    if let Some(origin_tool) = session.pending_composite_guidance_from.clone()
        && !matches!(call.name, "get_tool_metrics" | "set_profile" | "set_preset")
    {
        if is_workflow_tool(call.name) || call.name == "get_analysis_section" {
            session.composite_guidance_followed_count += 1;
        } else {
            session.composite_guidance_missed_count += 1;
            *session
                .composite_guidance_missed_by_origin
                .entry(origin_tool)
                .or_insert(0) += 1;
        }
        session.pending_composite_guidance_from = None;
    }
    if session.pending_quality_contract
        && (call.name == "get_analysis_section"
            || call.name == "get_file_diagnostics"
            || call.name == "find_tests")
    {
        session.recommended_check_followthrough_count += 1;
        session.pending_quality_contract = false;
    }
    if call.name != "get_tool_metrics"
        && session.pending_verifier_contract
        && (call.name == "get_analysis_section"
            || call.name == "get_file_diagnostics"
            || call.name == "find_tests"
            || call.name == "safe_rename_report"
            || call.name == "verify_change_readiness"
            || call.name == "unresolved_reference_check"
            || crate::tool_defs::is_content_mutation_tool(call.name))
    {
        session.verifier_followthrough_count += 1;
        session.pending_verifier_contract = false;
    }
    if let Some(prev) = previous
        && prev.tool == call.name
        && !prev.success
    {
        session.retry_count += 1;
    }
    if call.name != "get_tool_metrics"
        && let Some(prev_tool) = session.pending_truncation_tool.take()
    {
        session.truncation_followup_count += 1;
        if prev_tool == call.name {
            session.truncation_same_tool_retry_count += 1;
        }
    }
    push_latency_sample(&mut session.latency_samples, call.elapsed_ms);
    let invocation = ToolInvocation {
        tool: call.name.to_owned(),
        surface: call.surface.to_owned(),
        elapsed_ms: call.elapsed_ms,
        tokens: call.tokens,
        success: call.success,
        truncated: call.truncated,
        phase: call.phase.map(ToOwned::to_owned),
        target_paths: call.target_paths.to_vec(),
    };
    if session.timeline.len() < MAX_TIMELINE {
        session.timeline.push(invocation);
    } else {
        session.timeline.remove(0);
        session.timeline.push(invocation);
    }
    if call.truncated {
        session.truncated_response_count += 1;
        session.pending_truncation_tool = Some(call.name.to_owned());
    }
    if has_low_level_chain(&session.timeline) {
        session.repeated_low_level_chain_count += 1;
    }
}
