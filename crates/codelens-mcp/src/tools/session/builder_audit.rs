use super::audit_common::{
    CHECK_FAIL, CHECK_NA, CHECK_PASS, CHECK_WARN, add_check, collect_seen_paths,
    is_builder_surface, missing_paths, push_unique, resolve_audit_session_view,
};
use crate::AppState;
use crate::error::CodeLensError;
use crate::mutation_gate::is_refactor_gated_mutation_tool;
use crate::protocol::BackendKind;
use crate::session_context::SessionRequestContext;
use crate::telemetry::ToolInvocation;
use crate::tool_runtime::{ToolResult, success_meta};
use serde_json::{Value, json};

fn is_builder_preflight_tool(name: &str) -> bool {
    matches!(
        name,
        "prepare_harness_session"
            | "get_symbols_overview"
            | "get_file_diagnostics"
            | "verify_change_readiness"
            | "safe_rename_report"
            | "unresolved_reference_check"
    )
}

fn is_builder_session_tool(name: &str) -> bool {
    is_builder_preflight_tool(name)
        || matches!(
            name,
            "register_agent_work" | "claim_files" | "release_files"
        )
        || is_refactor_gated_mutation_tool(name)
}

fn collect_touched_files(
    timeline: &[ToolInvocation],
    active_claim_paths: &[String],
) -> Vec<String> {
    let mut touched = Vec::new();
    for entry in timeline {
        if is_refactor_gated_mutation_tool(&entry.tool)
            || matches!(
                entry.tool.as_str(),
                "verify_change_readiness"
                    | "safe_rename_report"
                    | "unresolved_reference_check"
                    | "claim_files"
            )
        {
            for path in &entry.target_paths {
                push_unique(&mut touched, path.clone());
            }
        }
    }
    for path in active_claim_paths {
        push_unique(&mut touched, path.clone());
    }
    touched
}

fn missing_preflight_paths_for_mutations(timeline: &[ToolInvocation]) -> Vec<String> {
    let mut verifier_paths = Vec::new();
    let mut missing = Vec::new();
    for entry in timeline {
        if matches!(
            entry.tool.as_str(),
            "verify_change_readiness" | "safe_rename_report" | "unresolved_reference_check"
        ) {
            for path in &entry.target_paths {
                push_unique(&mut verifier_paths, path.clone());
            }
            continue;
        }
        if is_refactor_gated_mutation_tool(&entry.tool) {
            for path in &entry.target_paths {
                if !verifier_paths.iter().any(|seen| seen == path) {
                    push_unique(&mut missing, path.clone());
                }
            }
        }
    }
    missing
}

fn missing_symbol_preflight_paths_for_renames(timeline: &[ToolInvocation]) -> Vec<String> {
    let mut symbol_preflight_paths = Vec::new();
    let mut missing = Vec::new();
    for entry in timeline {
        if matches!(
            entry.tool.as_str(),
            "safe_rename_report" | "unresolved_reference_check"
        ) {
            for path in &entry.target_paths {
                push_unique(&mut symbol_preflight_paths, path.clone());
            }
            continue;
        }
        if entry.tool == "rename_symbol" {
            for path in &entry.target_paths {
                if !symbol_preflight_paths.iter().any(|seen| seen == path) {
                    push_unique(&mut missing, path.clone());
                }
            }
        }
    }
    missing
}

pub(crate) fn build_builder_session_audit(
    state: &AppState,
    arguments: &Value,
) -> Result<Value, CodeLensError> {
    let request_session = SessionRequestContext::from_json(arguments);
    let requested_session_id = arguments.get("session_id").and_then(|value| value.as_str());
    let detail = arguments
        .get("detail")
        .and_then(|value| value.as_str())
        .unwrap_or("compact");

    let target_session_id = requested_session_id.unwrap_or(request_session.session_id.as_str());
    let metrics = state.metrics().session_snapshot_for(target_session_id);
    let view = resolve_audit_session_view(state, &request_session, requested_session_id, &metrics)?;
    let coordination = state.coordination_snapshot_for_scope(&view.scope);
    let active_registration = coordination
        .agents
        .iter()
        .find(|entry| entry.session_id == view.session_id);
    let active_claim = coordination
        .claims
        .iter()
        .find(|entry| entry.session_id == view.session_id);
    let active_claim_paths = active_claim
        .map(|claim| claim.paths.clone())
        .unwrap_or_default();

    let has_builder_preflight = metrics
        .timeline
        .iter()
        .any(|entry| is_builder_preflight_tool(&entry.tool));
    let has_mutation = metrics
        .timeline
        .iter()
        .any(|entry| is_refactor_gated_mutation_tool(&entry.tool));
    let has_coordination = metrics.timeline.iter().any(|entry| {
        matches!(
            entry.tool.as_str(),
            "register_agent_work" | "claim_files" | "release_files"
        )
    });
    let has_builder_surface = is_builder_surface(&view.current_surface)
        || metrics
            .timeline
            .iter()
            .any(|entry| is_builder_surface(&entry.surface));
    let candidate_session = has_builder_surface || has_mutation || has_coordination;

    if !candidate_session || (!has_mutation && !has_builder_preflight) {
        return Ok(json!({
            "status": CHECK_NA,
            "score": 0.0,
            "checks": [{
                "code": "applicability",
                "status": CHECK_NA,
                "summary": "No builder/refactor mutation or builder preflight flow was recorded for this session.",
                "evidence": {
                    "session_id": view.session_id,
                    "current_surface": view.current_surface,
                    "recent_tools": view.recent_tools,
                }
            }],
            "findings": [],
            "recommended_next_tools": [],
            "session_summary": {
                "session_id": view.session_id,
                "scope": view.scope,
                "transport": view.transport,
                "current_surface": view.current_surface,
                "requested_profile": view.requested_profile,
                "client_name": view.client_name,
                "recent_tools": view.recent_tools,
                "recent_files": view.recent_files,
                "mutation_calls": 0,
                "claim_state": {
                    "active_registration": false,
                    "active_claim": false,
                    "claimed_paths": [],
                }
            }
        }));
    }

    let touched_files = collect_touched_files(&metrics.timeline, &active_claim_paths);
    let first_mutation_idx = metrics
        .timeline
        .iter()
        .position(|entry| is_refactor_gated_mutation_tool(&entry.tool));
    let last_mutation_idx = metrics
        .timeline
        .iter()
        .rposition(|entry| is_refactor_gated_mutation_tool(&entry.tool));
    let first_builder_event_idx = metrics.timeline.iter().position(|entry| {
        is_builder_session_tool(&entry.tool) && entry.tool != "prepare_harness_session"
    });
    let prepared_before_first_event = first_builder_event_idx.map(|idx| {
        metrics.timeline[..idx]
            .iter()
            .any(|entry| entry.tool == "prepare_harness_session")
    });
    let has_registration_call = metrics
        .timeline
        .iter()
        .any(|entry| entry.tool == "register_agent_work");
    let has_claim_call = metrics
        .timeline
        .iter()
        .any(|entry| entry.tool == "claim_files");
    let has_release_call = metrics
        .timeline
        .iter()
        .any(|entry| entry.tool == "release_files");

    let symbols_seen = collect_seen_paths(
        &metrics.timeline,
        "get_symbols_overview",
        0..metrics.timeline.len(),
    );
    let missing_symbol_paths = missing_paths(&touched_files, &symbols_seen);
    let preflight_missing_paths = missing_preflight_paths_for_mutations(&metrics.timeline);
    let rename_preflight_missing_paths =
        missing_symbol_preflight_paths_for_renames(&metrics.timeline);
    let pre_diag_missing = first_mutation_idx
        .map(|_| {
            let mut diagnostics_seen = Vec::new();
            let mut missing = Vec::new();
            for entry in &metrics.timeline {
                if entry.tool == "get_file_diagnostics" {
                    for path in &entry.target_paths {
                        push_unique(&mut diagnostics_seen, path.clone());
                    }
                    continue;
                }
                if is_refactor_gated_mutation_tool(&entry.tool) {
                    for path in &entry.target_paths {
                        if !diagnostics_seen.iter().any(|seen| seen == path) {
                            push_unique(&mut missing, path.clone());
                        }
                    }
                }
            }
            missing
        })
        .unwrap_or_default();
    let post_diag_missing = last_mutation_idx
        .map(|_| {
            let mut diagnostics_seen = Vec::new();
            let mut missing = Vec::new();
            for entry in metrics.timeline.iter().rev() {
                if entry.tool == "get_file_diagnostics" {
                    for path in &entry.target_paths {
                        push_unique(&mut diagnostics_seen, path.clone());
                    }
                    continue;
                }
                if is_refactor_gated_mutation_tool(&entry.tool) {
                    for path in &entry.target_paths {
                        if !diagnostics_seen.iter().any(|seen| seen == path) {
                            push_unique(&mut missing, path.clone());
                        }
                    }
                }
            }
            missing
        })
        .unwrap_or_default();

    let mut checks = Vec::new();
    let mut findings = Vec::new();

    add_check(
        &mut checks,
        &mut findings,
        CHECK_PASS,
        "applicability",
        "Builder/refactor session telemetry found.",
        json!({
            "session_id": view.session_id,
            "current_surface": view.current_surface,
            "has_builder_preflight": has_builder_preflight,
            "has_mutation": has_mutation,
        }),
    );

    match prepared_before_first_event {
        Some(true) | None => add_check(
            &mut checks,
            &mut findings,
            CHECK_PASS,
            "bootstrap_order",
            "prepare_harness_session ran before the first builder/refactor action.",
            json!({
                "first_builder_event_index": first_builder_event_idx,
            }),
        ),
        Some(false) => add_check(
            &mut checks,
            &mut findings,
            CHECK_WARN,
            "bootstrap_order",
            "prepare_harness_session did not run before the first builder/refactor action.",
            json!({
                "first_builder_event_index": first_builder_event_idx,
                "recent_tools": view.recent_tools,
            }),
        ),
    }

    if has_mutation {
        let status = if metrics.mutation_without_preflight_count > 0
            || metrics.rename_without_symbol_preflight_count > 0
            || metrics.mutation_preflight_gate_denied_count > 0
            || !preflight_missing_paths.is_empty()
            || !rename_preflight_missing_paths.is_empty()
        {
            CHECK_FAIL
        } else {
            CHECK_PASS
        };
        add_check(
            &mut checks,
            &mut findings,
            status,
            "mutation_gate",
            if status == CHECK_FAIL {
                "Mutation attempts violated preflight or mutation-gate rules."
            } else {
                "Mutation attempts had verifier evidence and did not trip the mutation gate."
            },
            json!({
                "mutation_calls": metrics.timeline.iter().filter(|entry| is_refactor_gated_mutation_tool(&entry.tool)).count(),
                "mutation_preflight_checked_count": metrics.mutation_preflight_checked_count,
                "mutation_without_preflight_count": metrics.mutation_without_preflight_count,
                "rename_without_symbol_preflight_count": metrics.rename_without_symbol_preflight_count,
                "mutation_preflight_gate_denied_count": metrics.mutation_preflight_gate_denied_count,
                "stale_preflight_reject_count": metrics.stale_preflight_reject_count,
                "missing_preflight_paths": preflight_missing_paths,
                "missing_symbol_preflight_paths": rename_preflight_missing_paths,
            }),
        );
    } else {
        add_check(
            &mut checks,
            &mut findings,
            CHECK_NA,
            "mutation_gate",
            "No gated mutation call was recorded for this session.",
            json!({}),
        );
    }

    if touched_files.is_empty() {
        add_check(
            &mut checks,
            &mut findings,
            CHECK_NA,
            "structure_evidence",
            "No touched files were derived from preflight, mutation, or claim evidence.",
            json!({}),
        );
    } else if missing_symbol_paths.is_empty() {
        add_check(
            &mut checks,
            &mut findings,
            CHECK_PASS,
            "structure_evidence",
            "get_symbols_overview covered all touched files.",
            json!({
                "touched_files": touched_files,
            }),
        );
    } else {
        add_check(
            &mut checks,
            &mut findings,
            CHECK_WARN,
            "structure_evidence",
            "Some touched files have no get_symbols_overview evidence.",
            json!({
                "touched_files": touched_files,
                "missing_paths": missing_symbol_paths,
            }),
        );
    }

    if has_mutation && !touched_files.is_empty() {
        add_check(
            &mut checks,
            &mut findings,
            if pre_diag_missing.is_empty() {
                CHECK_PASS
            } else {
                CHECK_WARN
            },
            "diagnostics_before_mutation",
            if pre_diag_missing.is_empty() {
                "get_file_diagnostics ran on touched files before mutation."
            } else {
                "Some touched files have no pre-mutation diagnostics evidence."
            },
            json!({
                "touched_files": touched_files,
                "missing_paths": pre_diag_missing,
            }),
        );
        add_check(
            &mut checks,
            &mut findings,
            if post_diag_missing.is_empty() {
                CHECK_PASS
            } else {
                CHECK_WARN
            },
            "diagnostics_after_mutation",
            if post_diag_missing.is_empty() {
                "get_file_diagnostics ran on touched files after mutation."
            } else {
                "Some touched files have no post-mutation diagnostics evidence."
            },
            json!({
                "touched_files": touched_files,
                "missing_paths": post_diag_missing,
            }),
        );
    } else {
        add_check(
            &mut checks,
            &mut findings,
            CHECK_NA,
            "diagnostics_before_mutation",
            "No mutation happened, so pre-mutation diagnostics are not required.",
            json!({}),
        );
        add_check(
            &mut checks,
            &mut findings,
            CHECK_NA,
            "diagnostics_after_mutation",
            "No mutation happened, so post-mutation diagnostics are not required.",
            json!({}),
        );
    }

    let requires_coordination = has_mutation && view.non_local_http;
    if requires_coordination {
        add_check(
            &mut checks,
            &mut findings,
            if has_registration_call || active_registration.is_some() {
                CHECK_PASS
            } else {
                CHECK_WARN
            },
            "coordination_registration",
            if has_registration_call || active_registration.is_some() {
                "register_agent_work evidence exists for the HTTP builder session."
            } else {
                "HTTP builder session mutated files without register_agent_work evidence."
            },
            json!({
                "timeline_registration_call": has_registration_call,
                "active_registration": active_registration.is_some(),
            }),
        );
        add_check(
            &mut checks,
            &mut findings,
            if has_claim_call || active_claim.is_some() {
                CHECK_PASS
            } else {
                CHECK_WARN
            },
            "coordination_claim",
            if has_claim_call || active_claim.is_some() {
                "claim_files evidence exists for the HTTP builder session."
            } else {
                "HTTP builder session mutated files without claim_files evidence."
            },
            json!({
                "timeline_claim_call": has_claim_call,
                "active_claim": active_claim.is_some(),
                "claimed_paths": active_claim_paths,
            }),
        );
        add_check(
            &mut checks,
            &mut findings,
            if active_claim.is_none() {
                CHECK_PASS
            } else {
                CHECK_WARN
            },
            "coordination_release",
            if active_claim.is_none() {
                "No active file claim remains for the audited session."
            } else if has_release_call {
                "release_files was called but an active claim still remains."
            } else {
                "Active claim remains and no release_files evidence was found."
            },
            json!({
                "timeline_release_call": has_release_call,
                "active_claim": active_claim,
            }),
        );
    } else {
        add_check(
            &mut checks,
            &mut findings,
            CHECK_NA,
            "coordination_registration",
            "Coordination registration is only required for non-local HTTP mutation sessions.",
            json!({}),
        );
        add_check(
            &mut checks,
            &mut findings,
            CHECK_NA,
            "coordination_claim",
            "Coordination claims are only required for non-local HTTP mutation sessions.",
            json!({}),
        );
        add_check(
            &mut checks,
            &mut findings,
            CHECK_NA,
            "coordination_release",
            "Claim release is only required when a non-local HTTP mutation session holds claims.",
            json!({}),
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
        .any(|finding| finding["code"] == "mutation_gate")
    {
        if metrics.rename_without_symbol_preflight_count > 0
            || checks.iter().any(|check| {
                check["code"] == "mutation_gate"
                    && check["evidence"]["missing_symbol_preflight_paths"]
                        .as_array()
                        .is_some_and(|items| !items.is_empty())
            })
        {
            push_unique(&mut recommended_next_tools, "safe_rename_report");
        } else {
            push_unique(&mut recommended_next_tools, "verify_change_readiness");
        }
    }
    if findings
        .iter()
        .any(|finding| finding["code"] == "structure_evidence")
    {
        push_unique(&mut recommended_next_tools, "get_symbols_overview");
    }
    if findings.iter().any(|finding| {
        finding["code"] == "diagnostics_before_mutation"
            || finding["code"] == "diagnostics_after_mutation"
    }) {
        push_unique(&mut recommended_next_tools, "get_file_diagnostics");
    }
    if findings
        .iter()
        .any(|finding| finding["code"] == "coordination_registration")
    {
        push_unique(&mut recommended_next_tools, "register_agent_work");
    }
    if findings
        .iter()
        .any(|finding| finding["code"] == "coordination_claim")
    {
        push_unique(&mut recommended_next_tools, "claim_files");
    }
    if findings
        .iter()
        .any(|finding| finding["code"] == "coordination_release")
    {
        push_unique(&mut recommended_next_tools, "release_files");
    }

    let session_summary = json!({
        "session_id": view.session_id,
        "scope": view.scope,
        "transport": view.transport,
        "current_surface": view.current_surface,
        "requested_profile": view.requested_profile,
        "client_name": view.client_name,
        "recent_tools": view.recent_tools,
        "recent_files": view.recent_files,
        "mutation_calls": metrics.timeline.iter().filter(|entry| is_refactor_gated_mutation_tool(&entry.tool)).count(),
        "preflight_calls": metrics.timeline.iter().filter(|entry| matches!(entry.tool.as_str(), "verify_change_readiness" | "safe_rename_report" | "unresolved_reference_check")).count(),
        "touched_files": touched_files,
        "claim_state": {
            "active_registration": active_registration.is_some(),
            "active_claim": active_claim.is_some(),
            "claimed_paths": active_claim_paths,
        }
    });

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
        payload["coordination_snapshot"] =
            serde_json::to_value(&coordination).unwrap_or_else(|_| json!({}));
    }

    Ok(payload)
}

pub fn audit_builder_session(state: &AppState, arguments: &Value) -> ToolResult {
    Ok((
        build_builder_session_audit(state, arguments)?,
        success_meta(BackendKind::Session, 0.96),
    ))
}
