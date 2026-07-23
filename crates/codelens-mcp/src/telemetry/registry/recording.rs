//! Low-level call, surface, and session recording helpers.
#![allow(clippy::collapsible_if)]

use super::{MAX_TIMELINE, has_low_level_chain, push_latency_sample};
use crate::telemetry::{
    SessionMetrics, SurfaceMetrics, ToolCallEvent, ToolInvocation, ToolMetrics,
};
use std::collections::HashMap;

fn is_suggestion_observer(tool: &str) -> bool {
    matches!(tool, "get_tool_metrics" | "set_profile" | "set_preset")
}

fn resolve_pending_suggestion(session: &mut SessionMetrics, event: &ToolCallEvent<'_>) {
    if is_suggestion_observer(event.tool) || session.guidance.pending_suggested_tools.is_empty() {
        return;
    }

    let accepted = session
        .guidance
        .pending_suggested_tools
        .iter()
        .any(|suggested| {
            suggested == event.tool || event.operation.target == Some(suggested.as_str())
        });
    session.guidance.suggestion_unresolved_count = session
        .guidance
        .suggestion_unresolved_count
        .saturating_sub(1);
    if accepted {
        session.guidance.suggestion_accepted_count += 1;
        if event.success {
            session.guidance.suggestion_outcome_success_count += 1;
        } else {
            session.guidance.suggestion_outcome_error_count += 1;
        }
    } else {
        session.guidance.suggestion_diverted_count += 1;
    }
    session.guidance.pending_suggested_tools.clear();
}

fn record_pending_suggestions(session: &mut SessionMetrics, event: &ToolCallEvent<'_>) {
    if is_suggestion_observer(event.tool) || event.hints.suggested_next_tools.is_empty() {
        return;
    }
    session.guidance.pending_suggested_tools = event.hints.suggested_next_tools.to_vec();
    session.guidance.suggestion_unresolved_count += 1;
}

pub(super) fn record_tool_call(
    map: &mut HashMap<String, ToolMetrics>,
    event: &ToolCallEvent<'_>,
    now: u64,
) {
    let entry = map.entry(event.tool.to_owned()).or_default();
    entry.call_count += 1;
    if event.success {
        entry.success_count += 1;
    }
    entry.total_ms += event.elapsed_ms;
    entry.total_tokens += event.tokens;
    if event.elapsed_ms > entry.max_ms {
        entry.max_ms = event.elapsed_ms;
    }
    push_latency_sample(&mut entry.latency_samples, event.elapsed_ms);
    if !event.success {
        entry.error_count += 1;
    }
    entry.last_called_at = now;
}

pub(super) fn record_surface_call(
    surfaces: &mut HashMap<String, SurfaceMetrics>,
    event: &ToolCallEvent<'_>,
    now: u64,
) {
    let entry = surfaces.entry(event.surface.to_owned()).or_default();
    entry.call_count += 1;
    if event.success {
        entry.success_count += 1;
    }
    entry.total_ms += event.elapsed_ms;
    entry.total_tokens += event.tokens;
    push_latency_sample(&mut entry.latency_samples, event.elapsed_ms);
    if !event.success {
        entry.error_count += 1;
    }
    entry.last_called_at = now;
}

pub(super) fn record_session_call(session: &mut SessionMetrics, event: &ToolCallEvent<'_>) {
    let previous = session.timeline.last().cloned();
    resolve_pending_suggestion(session, event);
    session.core.total_calls += 1;
    if event.success {
        session.core.success_count += 1;
    }
    session.core.total_ms += event.elapsed_ms;
    session.core.total_tokens += event.tokens;
    if event.tool == "tools/list" {
        session.token.tools_list_tokens += event.tokens;
    }
    match event.operation.work_class {
        crate::operation::OperationWorkClass::Composite => {
            session.call_type.composite_calls += 1;
        }
        crate::operation::OperationWorkClass::Primitive => {
            session.call_type.low_level_calls += 1;
        }
        crate::operation::OperationWorkClass::Unresolved => {}
    }
    if !event.success {
        session.core.error_count += 1;
    }
    if let Some(origin_tool) = session.guidance.pending_composite_guidance_from.clone() {
        if !matches!(
            event.tool,
            "get_tool_metrics" | "set_profile" | "set_preset"
        ) {
            if event.operation.work_class.is_composite()
                || event.operation.target == Some("get_analysis_section")
            {
                session.guidance.composite_guidance_followed_count += 1;
            } else {
                session.guidance.composite_guidance_missed_count += 1;
                *session
                    .guidance
                    .composite_guidance_missed_by_origin
                    .entry(origin_tool)
                    .or_insert(0) += 1;
            }
            session.guidance.pending_composite_guidance_from = None;
        }
    }
    if session.guidance.pending_quality_contract
        && (event.tool == "get_analysis_section"
            || event.tool == "get_file_diagnostics"
            || event.tool == "find_tests")
    {
        session.guidance.recommended_check_followthrough_count += 1;
        session.guidance.pending_quality_contract = false;
    }
    if event.tool != "get_tool_metrics"
        && session.guidance.pending_verifier_contract
        && (event.tool == "get_analysis_section"
            || event.tool == "get_file_diagnostics"
            || event.tool == "find_tests"
            || event.tool == "safe_rename_report"
            || event.tool == "verify_change_readiness"
            || event.tool == "unresolved_reference_check"
            || crate::tool_defs::is_content_mutation_tool(event.tool))
    {
        session.guidance.verifier_followthrough_count += 1;
        session.guidance.pending_verifier_contract = false;
    }
    if let Some(prev) = previous {
        if prev.tool == event.tool && !prev.success {
            session.core.retry_count += 1;
        }
    }
    if event.tool != "get_tool_metrics" {
        if let Some(prev_tool) = session.truncation.pending_truncation_tool.take() {
            session.truncation.truncation_followup_count += 1;
            if prev_tool == event.tool {
                session.truncation.truncation_same_tool_retry_count += 1;
            }
        }
    }
    push_latency_sample(&mut session.core.latency_samples, event.elapsed_ms);
    let invocation = ToolInvocation {
        tool: event.tool.to_owned(),
        resolved_target: event.operation.target.map(ToOwned::to_owned),
        mode: event.operation.mode.map(ToOwned::to_owned),
        work_class: event.operation.work_class,
        downstream_call_count: event.operation.downstream_call_count,
        surface: event.surface.to_owned(),
        elapsed_ms: event.elapsed_ms,
        tokens: event.tokens,
        success: event.success,
        truncated: event.truncated,
        phase: event.phase.map(ToOwned::to_owned),
        target_paths: event.target_paths.to_vec(),
    };
    if session.timeline.len() < MAX_TIMELINE {
        session.timeline.push(invocation);
    } else {
        session.timeline.remove(0);
        session.timeline.push(invocation);
    }
    if event.truncated {
        session.truncation.truncated_response_count += 1;
        session.truncation.pending_truncation_tool = Some(event.tool.to_owned());
    }
    if has_low_level_chain(&session.timeline) {
        session.guidance.repeated_low_level_chain_count += 1;
    }
    record_pending_suggestions(session, event);
}
