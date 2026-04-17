use crate::AppState;
use crate::protocol::BackendKind;
use crate::session_context::SessionRequestContext;
use crate::telemetry::ToolInvocation;
use crate::tool_defs::is_content_mutation_tool;
use crate::tool_runtime::{ToolResult, success_meta};
use serde_json::{Value, json};

use super::audit_common::{
    CHECK_FAIL, CHECK_NA, CHECK_PASS, CHECK_WARN, add_check, collect_seen_paths,
    is_planner_surface, missing_paths, push_unique, resolve_audit_session_view,
};

const CHANGE_EVIDENCE_WORKFLOWS: &[&str] = &[
    "review_changes",
    "impact_report",
    "semantic_code_review",
    "diff_aware_references",
    "verify_change_readiness",
    "refactor_safety_report",
];

fn is_planner_workflow(name: &str) -> bool {
    matches!(
        name,
        "explore_codebase"
            | "review_architecture"
            | "cleanup_duplicate_logic"
            | "review_changes"
            | "diagnose_issues"
            | "analyze_change_request"
            | "verify_change_readiness"
            | "find_minimal_context_for_change"
            | "impact_report"
            | "refactor_safety_report"
            | "diff_aware_references"
            | "semantic_code_review"
            | "module_boundary_report"
            | "summarize_symbol_impact"
            | "mermaid_module_graph"
            | "onboard_project"
    )
}

fn planner_workflow_target_paths(timeline: &[ToolInvocation]) -> Vec<String> {
    let mut targets = Vec::new();
    for entry in timeline {
        if entry.tool == "get_changed_files"
            || (is_planner_workflow(&entry.tool) && is_planner_surface(&entry.surface))
        {
            for path in &entry.target_paths {
                push_unique(&mut targets, path.clone());
            }
        }
    }
    targets
}

fn missing_change_evidence_workflows(timeline: &[ToolInvocation]) -> Vec<String> {
    let has_changed_files_evidence = timeline
        .iter()
        .any(|entry| entry.tool == "get_changed_files" && !entry.target_paths.is_empty());
    let mut missing = Vec::new();
    for entry in timeline {
        if !is_planner_surface(&entry.surface) {
            continue;
        }
        if CHANGE_EVIDENCE_WORKFLOWS.contains(&entry.tool.as_str())
            && entry.target_paths.is_empty()
            && !has_changed_files_evidence
        {
            push_unique(&mut missing, entry.tool.clone());
        }
    }
    missing
}

fn role_audit_session_summary(
    view: &super::audit_common::AuditSessionView,
    workflow_targets: &[String],
    mutation_attempt_count: usize,
    read_side_workflow_count: usize,
) -> Value {
    json!({
        "session_id": view.session_id,
        "scope": view.scope,
        "transport": view.transport,
        "current_surface": view.current_surface,
        "requested_profile": view.requested_profile,
        "client_name": view.client_name,
        "recent_tools": view.recent_tools,
        "recent_files": view.recent_files,
        "mutation_calls": mutation_attempt_count,
        "read_side_workflow_calls": read_side_workflow_count,
        "target_files": workflow_targets,
        "claim_state": {
            "active_registration": false,
            "active_claim": false,
            "claimed_paths": [],
        }
    })
}

pub(super) fn build_planner_session_audit(
    state: &AppState,
    arguments: &Value,
) -> Result<Value, crate::error::CodeLensError> {
    let request_session = SessionRequestContext::from_json(arguments);
    let requested_session_id = arguments.get("session_id").and_then(|value| value.as_str());
    let detail = arguments
        .get("detail")
        .and_then(|value| value.as_str())
        .unwrap_or("compact");

    let target_session_id = requested_session_id.unwrap_or(request_session.session_id.as_str());
    let metrics = state.metrics().session_snapshot_for(target_session_id);
    let view = resolve_audit_session_view(state, &request_session, requested_session_id, &metrics)?;

    let has_read_side_surface = is_planner_surface(&view.current_surface)
        || metrics
            .timeline
            .iter()
            .any(|entry| is_planner_surface(&entry.surface));
    let mutation_attempt_count = metrics
        .timeline
        .iter()
        .filter(|entry| is_content_mutation_tool(&entry.tool))
        .count();
    let read_side_workflow_count = metrics
        .timeline
        .iter()
        .filter(|entry| is_planner_surface(&entry.surface) && is_planner_workflow(&entry.tool))
        .count();

    if !has_read_side_surface
        && read_side_workflow_count == 0
        && mutation_attempt_count == 0
        && metrics.mutation_preflight_gate_denied_count == 0
    {
        return Ok(json!({
            "status": CHECK_NA,
            "score": 0.0,
            "checks": [{
                "code": "applicability",
                "status": CHECK_NA,
                "summary": "No planner/reviewer read-side workflow activity was recorded for this session.",
                "evidence": {
                    "session_id": view.session_id,
                    "current_surface": view.current_surface,
                    "recent_tools": view.recent_tools,
                }
            }],
            "findings": [],
            "recommended_next_tools": [],
            "session_summary": role_audit_session_summary(&view, &[], 0, 0),
        }));
    }

    let first_workflow_idx = metrics
        .timeline
        .iter()
        .position(|entry| is_planner_surface(&entry.surface) && is_planner_workflow(&entry.tool));
    let prepared_before_first_workflow = first_workflow_idx.map(|idx| {
        metrics.timeline[..idx]
            .iter()
            .any(|entry| entry.tool == "prepare_harness_session")
    });

    let workflow_targets = planner_workflow_target_paths(&metrics.timeline);
    let symbols_seen = collect_seen_paths(
        &metrics.timeline,
        "get_symbols_overview",
        0..metrics.timeline.len(),
    );
    let diagnostics_seen = collect_seen_paths(
        &metrics.timeline,
        "get_file_diagnostics",
        0..metrics.timeline.len(),
    );
    let symbol_search_seen =
        collect_seen_paths(&metrics.timeline, "find_symbol", 0..metrics.timeline.len());
    let mut evidence_seen = symbols_seen;
    for path in diagnostics_seen {
        push_unique(&mut evidence_seen, path);
    }
    for path in symbol_search_seen {
        push_unique(&mut evidence_seen, path);
    }

    let missing_change_evidence = missing_change_evidence_workflows(&metrics.timeline);
    let missing_workflow_targets = missing_paths(&workflow_targets, &evidence_seen);

    let mut checks = Vec::new();
    let mut findings = Vec::new();

    add_check(
        &mut checks,
        &mut findings,
        CHECK_PASS,
        "applicability",
        "Planner/reviewer session telemetry found.",
        json!({
            "session_id": view.session_id,
            "current_surface": view.current_surface,
            "read_side_workflow_calls": read_side_workflow_count,
        }),
    );

    match prepared_before_first_workflow {
        Some(true) | None => add_check(
            &mut checks,
            &mut findings,
            CHECK_PASS,
            "bootstrap_order",
            "prepare_harness_session ran before the first planner/reviewer workflow.",
            json!({
                "first_workflow_index": first_workflow_idx,
            }),
        ),
        Some(false) => add_check(
            &mut checks,
            &mut findings,
            CHECK_WARN,
            "bootstrap_order",
            "prepare_harness_session did not run before the first planner/reviewer workflow.",
            json!({
                "first_workflow_index": first_workflow_idx,
                "recent_tools": view.recent_tools,
            }),
        ),
    }

    let mutation_status =
        if mutation_attempt_count > 0 || metrics.mutation_preflight_gate_denied_count > 0 {
            CHECK_FAIL
        } else {
            CHECK_PASS
        };
    add_check(
        &mut checks,
        &mut findings,
        mutation_status,
        "read_side_mutation_attempt",
        if mutation_status == CHECK_FAIL {
            "Planner/reviewer session attempted a content mutation or tripped the mutation gate."
        } else {
            "Planner/reviewer session remained read-side only."
        },
        json!({
            "mutation_calls": mutation_attempt_count,
            "mutation_preflight_gate_denied_count": metrics.mutation_preflight_gate_denied_count,
            "mutation_without_preflight_count": metrics.mutation_without_preflight_count,
        }),
    );

    add_check(
        &mut checks,
        &mut findings,
        if missing_change_evidence.is_empty() {
            CHECK_PASS
        } else {
            CHECK_WARN
        },
        "change_evidence",
        if missing_change_evidence.is_empty() {
            "Read-side workflows had changed-files or explicit target-path evidence."
        } else {
            "Read-side workflows ran without get_changed_files or explicit target-path evidence."
        },
        json!({
            "missing_workflows": missing_change_evidence,
        }),
    );

    add_check(
        &mut checks,
        &mut findings,
        if metrics.repeated_low_level_chain_count == 0
            && metrics.composite_guidance_missed_count == 0
        {
            CHECK_PASS
        } else {
            CHECK_WARN
        },
        "workflow_first",
        if metrics.repeated_low_level_chain_count == 0
            && metrics.composite_guidance_missed_count == 0
        {
            "Planner/reviewer session stayed on composite workflow entrypoints."
        } else {
            "Planner/reviewer session fell back to low-level chains instead of composite guidance."
        },
        json!({
            "repeated_low_level_chain_count": metrics.repeated_low_level_chain_count,
            "composite_guidance_missed_count": metrics.composite_guidance_missed_count,
            "composite_guidance_missed_by_origin": metrics.composite_guidance_missed_by_origin,
        }),
    );

    if workflow_targets.is_empty() {
        add_check(
            &mut checks,
            &mut findings,
            CHECK_NA,
            "read_side_evidence",
            "No workflow target files were derived for read-side evidence checks.",
            json!({}),
        );
    } else {
        add_check(
            &mut checks,
            &mut findings,
            if missing_workflow_targets.is_empty() {
                CHECK_PASS
            } else {
                CHECK_WARN
            },
            "read_side_evidence",
            if missing_workflow_targets.is_empty() {
                "Target files have symbol/diagnostic evidence from read-side exploration tools."
            } else {
                "Some target files have no get_symbols_overview, find_symbol, or get_file_diagnostics evidence."
            },
            json!({
                "target_files": workflow_targets,
                "missing_paths": missing_workflow_targets,
            }),
        );
    }

    let fail_count = findings
        .iter()
        .filter(|finding| finding["severity"] == CHECK_FAIL)
        .count();
    let warn_count = findings
        .iter()
        .filter(|finding| finding["severity"] == CHECK_WARN)
        .count();
    let status = if fail_count > 0 {
        CHECK_FAIL
    } else if warn_count > 0 {
        CHECK_WARN
    } else {
        CHECK_PASS
    };
    let score = (1.0 - (fail_count as f64 * 0.5) - (warn_count as f64 * 0.15)).clamp(0.0, 1.0);

    let mut recommended_next_tools = Vec::new();
    if findings
        .iter()
        .any(|finding| finding["code"] == "bootstrap_order")
    {
        push_unique(&mut recommended_next_tools, "prepare_harness_session");
    }
    if findings
        .iter()
        .any(|finding| finding["code"] == "change_evidence")
    {
        push_unique(&mut recommended_next_tools, "get_changed_files");
    }
    if findings
        .iter()
        .any(|finding| finding["code"] == "workflow_first")
    {
        push_unique(&mut recommended_next_tools, "review_changes");
    }
    if findings
        .iter()
        .any(|finding| finding["code"] == "read_side_evidence")
    {
        push_unique(&mut recommended_next_tools, "get_symbols_overview");
        push_unique(&mut recommended_next_tools, "get_file_diagnostics");
    }

    let session_summary = role_audit_session_summary(
        &view,
        &workflow_targets,
        mutation_attempt_count,
        read_side_workflow_count,
    );

    let mut payload = json!({
        "status": status,
        "score": score,
        "checks": checks,
        "findings": findings,
        "recommended_next_tools": recommended_next_tools,
        "session_summary": session_summary,
    });
    if detail == "full" {
        payload["session_metrics"] = serde_json::to_value(&metrics).unwrap_or_else(|_| json!({}));
    }

    Ok(payload)
}

pub fn audit_planner_session(state: &AppState, arguments: &Value) -> ToolResult {
    Ok((
        build_planner_session_audit(state, arguments)?,
        success_meta(BackendKind::Session, 0.96),
    ))
}
