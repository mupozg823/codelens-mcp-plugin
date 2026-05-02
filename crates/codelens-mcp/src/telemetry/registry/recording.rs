//! Low-level call, surface, and session recording helpers.
#![allow(clippy::collapsible_if)]

use super::{MAX_TIMELINE, has_low_level_chain, is_workflow_tool, push_latency_sample};
use crate::telemetry::{SessionMetrics, SurfaceMetrics, ToolInvocation, ToolMetrics};
use std::collections::HashMap;

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

#[allow(clippy::too_many_arguments)]
pub(super) fn record_session_call(
    session: &mut SessionMetrics,
    name: &str,
    elapsed_ms: u64,
    success: bool,
    tokens: usize,
    surface: &str,
    truncated: bool,
    phase: Option<&str>,
    target_paths: &[String],
) {
    let previous = session.timeline.last().cloned();
    session.core.total_calls += 1;
    if success {
        session.core.success_count += 1;
    }
    session.core.total_ms += elapsed_ms;
    session.core.total_tokens += tokens;
    if name == "tools/list" {
        session.token.tools_list_tokens += tokens;
    }
    if is_workflow_tool(name) {
        session.call_type.composite_calls += 1;
    } else {
        session.call_type.low_level_calls += 1;
    }
    if !success {
        session.core.error_count += 1;
    }
    if let Some(origin_tool) = session.guidance.pending_composite_guidance_from.clone() {
        if !matches!(name, "get_tool_metrics" | "set_profile" | "set_preset") {
            if is_workflow_tool(name) || name == "get_analysis_section" {
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
        && (name == "get_analysis_section"
            || name == "get_file_diagnostics"
            || name == "find_tests")
    {
        session.guidance.recommended_check_followthrough_count += 1;
        session.guidance.pending_quality_contract = false;
    }
    if name != "get_tool_metrics"
        && session.guidance.pending_verifier_contract
        && (name == "get_analysis_section"
            || name == "get_file_diagnostics"
            || name == "find_tests"
            || name == "safe_rename_report"
            || name == "verify_change_readiness"
            || name == "unresolved_reference_check"
            || crate::tool_defs::is_content_mutation_tool(name))
    {
        session.guidance.verifier_followthrough_count += 1;
        session.guidance.pending_verifier_contract = false;
    }
    if let Some(prev) = previous {
        if prev.tool == name && !prev.success {
            session.core.retry_count += 1;
        }
    }
    if name != "get_tool_metrics" {
        if let Some(prev_tool) = session.truncation.pending_truncation_tool.take() {
            session.truncation.truncation_followup_count += 1;
            if prev_tool == name {
                session.truncation.truncation_same_tool_retry_count += 1;
            }
        }
    }
    push_latency_sample(&mut session.core.latency_samples, elapsed_ms);
    let invocation = ToolInvocation {
        tool: name.to_owned(),
        surface: surface.to_owned(),
        elapsed_ms,
        tokens,
        success,
        truncated,
        phase: phase.map(ToOwned::to_owned),
        target_paths: target_paths.to_vec(),
    };
    if session.timeline.len() < MAX_TIMELINE {
        session.timeline.push(invocation);
    } else {
        session.timeline.remove(0);
        session.timeline.push(invocation);
    }
    if truncated {
        session.truncation.truncated_response_count += 1;
        session.truncation.pending_truncation_tool = Some(name.to_owned());
    }
    if has_low_level_chain(&session.timeline) {
        session.guidance.repeated_low_level_chain_count += 1;
    }
}
