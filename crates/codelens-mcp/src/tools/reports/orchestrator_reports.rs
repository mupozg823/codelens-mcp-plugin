use crate::AppState;
use crate::error::CodeLensError;
use crate::resources::{analysis_section_handles, analysis_summary_resource};
use crate::tool_runtime::{ToolResult, required_string};
use crate::tools::report_contract::make_handle_response;
use crate::tools::report_utils::strings_from_array;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const RUN_SCHEMA_VERSION: &str = "codelens-orchestration-run-v1";

#[derive(Clone, Copy, PartialEq, Eq)]
enum ApprovalDecision {
    Requested,
    Granted,
    Denied,
}

impl ApprovalDecision {
    fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Granted => "granted",
            Self::Denied => "denied",
        }
    }
}

fn normalize_mode(arguments: &Value) -> Result<&str, CodeLensError> {
    match arguments
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("solo")
    {
        "solo" => Ok("solo"),
        "planner_builder" | "planner-builder" => Ok("planner_builder"),
        "ci_audit" | "ci-audit" => Ok("ci_audit"),
        other => Err(CodeLensError::Validation(format!(
            "unsupported orchestrate_change mode `{other}`; expected solo, planner_builder, or ci_audit"
        ))),
    }
}

fn approval_value(arguments: &Value) -> Option<&Value> {
    arguments.get("approval").filter(|value| value.is_object())
}

fn approval_decision(arguments: &Value) -> Result<Option<ApprovalDecision>, CodeLensError> {
    let decision = approval_value(arguments)
        .and_then(|value| value.get("decision"))
        .or_else(|| arguments.get("approval_decision"))
        .and_then(Value::as_str);
    match decision {
        None => Ok(None),
        Some("requested") | Some("request") => Ok(Some(ApprovalDecision::Requested)),
        Some("granted") | Some("approved") | Some("approve") => Ok(Some(ApprovalDecision::Granted)),
        Some("denied") | Some("rejected") | Some("deny") => Ok(Some(ApprovalDecision::Denied)),
        Some(other) => Err(CodeLensError::Validation(format!(
            "unsupported approval decision `{other}`; expected requested, granted, or denied"
        ))),
    }
}

fn approval_actor(arguments: &Value) -> String {
    approval_value(arguments)
        .and_then(|value| value.get("actor"))
        .or_else(|| arguments.get("approved_by"))
        .or_else(|| arguments.get("requester"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned()
}

fn approval_reason(arguments: &Value) -> Option<String> {
    approval_value(arguments)
        .and_then(|value| value.get("reason"))
        .or_else(|| arguments.get("approval_reason"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn approved_actions(arguments: &Value) -> Vec<String> {
    let actions = approval_value(arguments)
        .and_then(|value| value.get("approved_actions"))
        .or_else(|| arguments.get("approved_actions"))
        .and_then(Value::as_array);
    let mut values = strings_from_array(actions, "action", 16);
    if values.is_empty() {
        values.push("mutation".to_owned());
    }
    values
}

fn target_paths(arguments: &Value) -> Vec<String> {
    let mut paths = strings_from_array(
        arguments
            .get("target_paths")
            .and_then(|value| value.as_array()),
        "path",
        16,
    );
    for path in strings_from_array(
        arguments
            .get("changed_files")
            .and_then(|value| value.as_array()),
        "file",
        16,
    ) {
        crate::util::push_unique_string(&mut paths, path);
    }
    paths
}

fn acceptance_items(arguments: &Value) -> Vec<String> {
    strings_from_array(
        arguments
            .get("acceptance")
            .and_then(|value| value.as_array()),
        "item",
        12,
    )
}

fn analysis_args(task: &str, profile_hint: Option<&str>, paths: &[String]) -> Value {
    let mut args = json!({"task": task});
    if let Some(map) = args.as_object_mut() {
        if let Some(profile_hint) = profile_hint {
            map.insert("profile_hint".to_owned(), json!(profile_hint));
        }
        if !paths.is_empty() {
            map.insert("changed_files".to_owned(), json!(paths));
        }
    }
    args
}

fn evidence_handle(payload: &Value) -> Value {
    json!({
        "analysis_id": payload.get("analysis_id").cloned().unwrap_or(Value::Null),
        "summary": payload.get("summary").cloned().unwrap_or(Value::Null),
        "summary_resource": payload.get("summary_resource").cloned().unwrap_or(Value::Null),
        "section_handles": payload.get("section_handles").cloned().unwrap_or_else(|| json!([])),
        "readiness": payload.get("readiness").cloned().unwrap_or(Value::Null),
        "blocker_count": payload.get("blocker_count").cloned().unwrap_or_else(|| json!(0)),
    })
}

fn make_run_id(
    task: &str,
    mode: &str,
    paths: &[String],
    acceptance: &[String],
    requester: &str,
    worktree: &str,
    created_at_ms: u64,
) -> String {
    let digest = crate::util::canonical_sha256_hex(&json!({
        "task": task,
        "mode": mode,
        "target_paths": paths,
        "acceptance": acceptance,
        "requester": requester,
        "worktree": worktree,
        "created_at_ms": created_at_ms,
    }));
    format!("orun-{}", &digest[..16])
}

fn role_bindings() -> Value {
    json!([
        {
            "role": "viewer",
            "surface": "run summary, artifacts, audit",
            "allowed_actions": ["inspect"],
            "mutations": "never"
        },
        {
            "role": "planner",
            "surface": "plan, preflight, claims",
            "allowed_actions": ["create_plan", "run_preflight", "claim_files"],
            "tools": [
                "prepare_harness_session",
                "get_symbols_overview",
                "get_file_diagnostics",
                "verify_change_readiness",
                "register_agent_work",
                "claim_files"
            ],
            "mutations": "never"
        },
        {
            "role": "builder",
            "surface": "execution lane",
            "allowed_actions": ["apply_approved_changes", "run_focused_tests"],
            "mutations": "only_after_approval_and_preflight"
        },
        {
            "role": "reviewer",
            "surface": "verification lane",
            "allowed_actions": ["review_diff", "audit_sessions", "verify_evidence"],
            "mutations": "never"
        },
        {
            "role": "admin",
            "surface": "operation lane",
            "allowed_actions": ["cancel_run", "override_stale_claims", "configure_policy"],
            "mutations": "policy_controlled"
        }
    ])
}

fn audit_event(
    run_id: &str,
    event: &str,
    from: Option<&str>,
    to: &str,
    created_at_ms: u64,
    offset: u64,
) -> Value {
    json!({
        "run_id": run_id,
        "event": event,
        "from": from,
        "to": to,
        "timestamp_ms": created_at_ms.saturating_add(offset),
        "audit_required": true,
    })
}

fn orchestration_sections() -> Vec<String> {
    [
        "orchestration_run",
        "plan",
        "preflight",
        "audit_events",
        "evidence_handles",
        "approval_policy",
        "dispatch_plan",
    ]
    .iter()
    .map(|section| (*section).to_owned())
    .collect()
}

fn run_state(run: &Value) -> &str {
    run.get("state")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
}

fn run_id_value(run: &Value) -> Option<&str> {
    run.get("run_id").and_then(Value::as_str)
}

fn terminal_state(state: &str) -> bool {
    matches!(state, "completed" | "failed" | "cancelled")
}

fn next_required_event(state: &str) -> Option<&'static str> {
    match state {
        "drafted" => Some("plan_proposed"),
        "planned" => Some("preflight_passed"),
        "preflighted" => Some("approval_requested"),
        "approval_required" => Some("approval_granted"),
        "executing" => Some("mutation_applied"),
        "verifying" => Some("verification_passed"),
        _ => None,
    }
}

fn recommended_next_tools(state: &str) -> Vec<&'static str> {
    match state {
        "drafted" | "planned" | "preflighted" | "approval_required" => {
            vec!["orchestrate_change", "verify_change_readiness"]
        }
        "executing" => vec!["replace", "replace_content", "verify_change_readiness"],
        "verifying" => vec![
            "verify_change_readiness",
            "review_changes",
            "audit_builder_session",
        ],
        _ => vec!["get_orchestration_run", "list_orchestration_runs"],
    }
}

fn audit_events_for_analysis(state: &AppState, analysis_id: &str) -> Value {
    state
        .get_analysis_section(analysis_id, "audit_events")
        .unwrap_or_else(|_| json!({"events": []}))
}

fn audit_event_count(state: &AppState, analysis_id: &str) -> usize {
    audit_events_for_analysis(state, analysis_id)
        .get("events")
        .and_then(Value::as_array)
        .map(|events| events.len())
        .unwrap_or_default()
}

fn run_summary_item(state: &AppState, analysis_id: &str, run: &Value, created_at_ms: u64) -> Value {
    let state_name = run_state(run);
    let sections = orchestration_sections();
    json!({
        "analysis_id": analysis_id,
        "run_id": run_id_value(run).unwrap_or(""),
        "state": state_name,
        "terminal": terminal_state(state_name),
        "objective": run.get("objective").cloned().unwrap_or(Value::Null),
        "mode": run.get("mode").cloned().unwrap_or(Value::Null),
        "target_paths": run.get("target_paths").cloned().unwrap_or_else(|| json!([])),
        "created_at_ms": run.get("created_at_ms").and_then(Value::as_u64).unwrap_or(created_at_ms),
        "last_event": run.get("last_event").cloned().unwrap_or(Value::Null),
        "last_event_timestamp_ms": run.get("last_event_timestamp_ms").cloned().unwrap_or(Value::Null),
        "event_count": audit_event_count(state, analysis_id),
        "summary_resource": analysis_summary_resource(analysis_id),
        "section_handles": analysis_section_handles(analysis_id, &sections),
        "resume": {
            "resumable": !terminal_state(state_name),
            "next_required_event": next_required_event(state_name),
            "recommended_next_tools": recommended_next_tools(state_name),
        }
    })
}

fn find_orchestration_run(
    state: &AppState,
    scope: &str,
    arguments: &Value,
) -> Result<(String, crate::runtime_types::AnalysisArtifact, Value), CodeLensError> {
    let requested_analysis_id = arguments.get("analysis_id").and_then(Value::as_str);
    let requested_run_id = arguments.get("run_id").and_then(Value::as_str);

    if requested_analysis_id.is_none() && requested_run_id.is_none() {
        return Err(CodeLensError::Validation(
            "orchestration run lookup requires run_id or analysis_id".to_owned(),
        ));
    }

    if let Some(analysis_id) = requested_analysis_id {
        let artifact = state
            .get_analysis_for_scope(scope, analysis_id)
            .ok_or_else(|| {
                CodeLensError::NotFound(format!("unknown analysis_id `{analysis_id}`"))
            })?;
        if artifact.tool_name != "orchestrate_change" {
            return Err(CodeLensError::Validation(format!(
                "analysis_id `{analysis_id}` was produced by `{}` not orchestrate_change",
                artifact.tool_name
            )));
        }
        let run = state.get_analysis_section(analysis_id, "orchestration_run")?;
        if let Some(run_id) = requested_run_id
            && run_id_value(&run) != Some(run_id)
        {
            return Err(CodeLensError::Validation(format!(
                "analysis_id `{analysis_id}` does not contain run_id `{run_id}`"
            )));
        }
        return Ok((analysis_id.to_owned(), artifact, run));
    }

    let run_id = requested_run_id.expect("checked above");
    for summary in state.list_analysis_summaries_for_scope(scope) {
        if summary.tool_name != "orchestrate_change" {
            continue;
        }
        let Ok(run) = state.get_analysis_section(&summary.id, "orchestration_run") else {
            continue;
        };
        if run_id_value(&run) == Some(run_id)
            && let Some(artifact) = state.get_analysis_for_scope(scope, &summary.id)
        {
            return Ok((summary.id, artifact, run));
        }
    }

    Err(CodeLensError::NotFound(format!(
        "unknown orchestration run `{run_id}`"
    )))
}

pub fn list_orchestration_runs(state: &AppState, arguments: &Value) -> ToolResult {
    let scope = state.project_scope_for_arguments(arguments);
    let state_filter = arguments.get("state").and_then(Value::as_str);
    let mode_filter = arguments.get("mode").and_then(Value::as_str);
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value.clamp(1, 100) as usize)
        .unwrap_or(25);

    let mut status_counts = BTreeMap::new();
    let mut runs = Vec::new();
    for summary in state.list_analysis_summaries_for_scope(&scope) {
        if summary.tool_name != "orchestrate_change" {
            continue;
        }
        let Ok(run) = state.get_analysis_section(&summary.id, "orchestration_run") else {
            continue;
        };
        let current_state = run_state(&run);
        *status_counts
            .entry(current_state.to_owned())
            .or_insert(0usize) += 1;
        if state_filter.is_some_and(|filter| filter != current_state) {
            continue;
        }
        if mode_filter.is_some_and(|filter| run.get("mode").and_then(Value::as_str) != Some(filter))
        {
            continue;
        }
        runs.push(run_summary_item(
            state,
            &summary.id,
            &run,
            summary.created_at_ms,
        ));
        if runs.len() >= limit {
            break;
        }
    }

    Ok((
        json!({
            "runs": runs,
            "count": runs.len(),
            "limit": limit,
            "status_counts": status_counts,
            "scope": scope,
        }),
        crate::tools::success_meta(crate::protocol::BackendKind::Memory, 1.0),
    ))
}

pub fn get_orchestration_run(state: &AppState, arguments: &Value) -> ToolResult {
    let scope = state.project_scope_for_arguments(arguments);
    let include_events = arguments
        .get("include_events")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let include_sections = arguments
        .get("include_sections")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let (analysis_id, artifact, run) = find_orchestration_run(state, &scope, arguments)?;
    let run_id = run_id_value(&run)
        .ok_or_else(|| {
            CodeLensError::Internal(anyhow::anyhow!("orchestration run missing run_id"))
        })?
        .to_owned();
    let state_name = run_state(&run);
    let sections = orchestration_sections();
    let audit_events = audit_events_for_analysis(state, &analysis_id);
    let event_count = audit_events
        .get("events")
        .and_then(Value::as_array)
        .map(|events| events.len())
        .unwrap_or_default();

    let mut payload = json!({
        "analysis_id": analysis_id,
        "run_id": run_id,
        "run": run,
        "state": state_name,
        "terminal": terminal_state(state_name),
        "event_count": event_count,
        "events": if include_events { audit_events.get("events").cloned().unwrap_or_else(|| json!([])) } else { json!([]) },
        "event_replay": {
            "current_state": state_name,
            "last_event": run.get("last_event").cloned().unwrap_or(Value::Null),
            "terminal": terminal_state(state_name),
        },
        "resume": {
            "resumable": !terminal_state(state_name),
            "next_required_event": next_required_event(state_name),
            "recommended_next_tools": recommended_next_tools(state_name),
        },
        "summary_resource": analysis_summary_resource(&analysis_id),
        "section_handles": analysis_section_handles(&analysis_id, &sections),
    });

    if include_sections && let Some(obj) = payload.as_object_mut() {
        let mut expanded = serde_json::Map::new();
        for section in &sections {
            if let Ok(content) = state.get_analysis_section(&analysis_id, section) {
                expanded.insert(section.clone(), content);
            }
        }
        obj.insert("sections".to_owned(), Value::Object(expanded));
    }

    Ok((
        payload,
        crate::tools::success_meta(crate::protocol::BackendKind::Memory, artifact.confidence),
    ))
}

pub fn cancel_orchestration_run(state: &AppState, arguments: &Value) -> ToolResult {
    let scope = state.project_scope_for_arguments(arguments);
    let actor = arguments
        .get("actor")
        .or_else(|| arguments.get("requester"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let reason = arguments
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("cancelled by requester");
    let (analysis_id, artifact, mut run) = find_orchestration_run(state, &scope, arguments)?;
    let run_id = run_id_value(&run)
        .ok_or_else(|| {
            CodeLensError::Internal(anyhow::anyhow!("orchestration run missing run_id"))
        })?
        .to_owned();
    let current_state = run_state(&run).to_owned();

    if current_state == "cancelled" {
        return Ok((
            json!({
                "analysis_id": analysis_id,
                "run_id": run_id,
                "state": "cancelled",
                "cancelled": false,
                "reason": "already_cancelled",
                "revoked_approvals": 0,
                "run": run,
            }),
            crate::tools::success_meta(crate::protocol::BackendKind::Memory, artifact.confidence),
        ));
    }
    if terminal_state(&current_state) {
        return Err(CodeLensError::Validation(format!(
            "orchestration run `{run_id}` is already terminal in state `{current_state}`"
        )));
    }

    let event = audit_event(
        &run_id,
        "run_cancelled",
        Some(&current_state),
        "cancelled",
        crate::util::now_ms(),
        0,
    );
    let mut event = event.as_object().cloned().unwrap_or_default();
    event.insert("actor".to_owned(), json!(actor));
    event.insert("reason".to_owned(), json!(reason));
    let event = Value::Object(event);

    let mut audit_events = audit_events_for_analysis(state, &analysis_id);
    if let Some(events) = audit_events
        .get_mut("events")
        .and_then(|value| value.as_array_mut())
    {
        events.push(event.clone());
    } else {
        audit_events = json!({"events": [event.clone()]});
    }
    state.upsert_analysis_section_for_scope(&scope, &analysis_id, "audit_events", &audit_events)?;

    if let Some(obj) = run.as_object_mut() {
        obj.insert("state".to_owned(), json!("cancelled"));
        obj.insert("last_event".to_owned(), json!("run_cancelled"));
        obj.insert(
            "last_event_timestamp_ms".to_owned(),
            event
                .get("timestamp_ms")
                .cloned()
                .unwrap_or_else(|| json!(crate::util::now_ms())),
        );
        obj.insert("cancelled_by".to_owned(), json!(actor));
        obj.insert("cancel_reason".to_owned(), json!(reason));
    }
    state.upsert_analysis_section_for_scope(&scope, &analysis_id, "orchestration_run", &run)?;
    let revoked_approvals = state.revoke_orchestration_approvals_for_scope(&scope, &run_id);

    Ok((
        json!({
            "analysis_id": analysis_id,
            "run_id": run_id,
            "state": "cancelled",
            "cancelled": true,
            "event": event,
            "revoked_approvals": revoked_approvals,
            "run": run,
            "summary_resource": analysis_summary_resource(&analysis_id),
            "section_handles": analysis_section_handles(&analysis_id, &orchestration_sections()),
        }),
        crate::tools::success_meta(crate::protocol::BackendKind::Memory, artifact.confidence),
    ))
}

pub fn orchestrate_change(state: &AppState, arguments: &Value) -> ToolResult {
    let task = required_string(arguments, "task")?;
    let mode = normalize_mode(arguments)?;
    let paths = target_paths(arguments);
    let acceptance = acceptance_items(arguments);
    let profile_hint = arguments.get("profile_hint").and_then(Value::as_str);
    let approval_decision = approval_decision(arguments)?;
    let approval_actor = approval_actor(arguments);
    let approval_reason = approval_reason(arguments);
    let approved_actions = approved_actions(arguments);
    let project_root = state.current_project_scope();
    let worktree = arguments
        .get("worktree")
        .and_then(Value::as_str)
        .unwrap_or(project_root.as_str());
    let requester = arguments
        .get("requester")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let created_at_ms = crate::util::now_ms();
    let run_id = make_run_id(
        task,
        mode,
        &paths,
        &acceptance,
        requester,
        worktree,
        created_at_ms,
    );
    let evidence_args = analysis_args(task, profile_hint, &paths);

    let (change_payload, _) = super::analyze_change_request(state, &evidence_args)?;
    let (readiness_payload, _) = super::verify_change_readiness(state, &evidence_args)?;

    let readiness = readiness_payload
        .get("readiness")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mutation_ready = readiness
        .get("mutation_ready")
        .and_then(Value::as_str)
        .unwrap_or("caution");
    let blocker_count = readiness_payload
        .get("blocker_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let blocked = mutation_ready == "blocked" || blocker_count > 0;
    let final_state = match (blocked, approval_decision) {
        (true, _) => "failed",
        (false, Some(ApprovalDecision::Granted)) => "executing",
        (false, Some(ApprovalDecision::Denied)) => "cancelled",
        (false, _) => "approval_required",
    };
    let preflight_event = if blocked {
        "preflight_blocked"
    } else {
        "preflight_passed"
    };
    let preflight_to_state = if blocked { final_state } else { "preflighted" };

    let mut events = vec![
        audit_event(&run_id, "run_created", None, "drafted", created_at_ms, 0),
        audit_event(
            &run_id,
            "plan_proposed",
            Some("drafted"),
            "planned",
            created_at_ms,
            1,
        ),
        audit_event(
            &run_id,
            preflight_event,
            Some("planned"),
            preflight_to_state,
            created_at_ms,
            2,
        ),
    ];
    if !blocked {
        events.push(audit_event(
            &run_id,
            "approval_requested",
            Some("preflighted"),
            "approval_required",
            created_at_ms,
            3,
        ));
        match approval_decision {
            Some(ApprovalDecision::Granted) => events.push(audit_event(
                &run_id,
                "approval_granted",
                Some("approval_required"),
                "executing",
                created_at_ms,
                4,
            )),
            Some(ApprovalDecision::Denied) => events.push(audit_event(
                &run_id,
                "approval_denied",
                Some("approval_required"),
                "cancelled",
                created_at_ms,
                4,
            )),
            _ => {}
        }
    }

    let next_actions = if blocked {
        vec![
            "Expand the readiness evidence before dispatching a builder.".to_owned(),
            "Resolve blockers, then rerun orchestrate_change.".to_owned(),
        ]
    } else if approval_decision == Some(ApprovalDecision::Granted) {
        vec![
            "Dispatch the approved bounded mutation with orchestration_run_id.".to_owned(),
            "Append mutation_applied after the wrapped mutation returns.".to_owned(),
            "Run verification before marking the orchestration run complete.".to_owned(),
        ]
    } else if approval_decision == Some(ApprovalDecision::Denied) {
        vec![
            "Do not dispatch mutation for this run.".to_owned(),
            "Create a new orchestration run if the objective changes.".to_owned(),
        ]
    } else {
        vec![
            "Record approval.decision=granted before any mutation.".to_owned(),
            "Dispatch one builder/refactor lane only after approval is recorded.".to_owned(),
        ]
    };
    let top_findings = vec![
        format!("Run `{run_id}` dry-run state: `{final_state}`."),
        format!("{} target path(s) supplied for preflight.", paths.len()),
        format!("Verifier mutation readiness: `{mutation_ready}`."),
    ];

    let mut sections = BTreeMap::new();
    sections.insert(
        "orchestration_run".to_owned(),
        json!({
            "schema_version": RUN_SCHEMA_VERSION,
            "run_id": run_id.clone(),
            "dry_run": true,
            "execution_performed": false,
            "created_at_ms": created_at_ms,
            "state": final_state,
            "project_root": project_root,
            "worktree": worktree,
            "requester": requester,
            "objective": task,
            "mode": mode,
            "target_paths": paths.clone(),
            "acceptance": acceptance.clone(),
        }),
    );
    sections.insert(
        "plan".to_owned(),
        json!({
            "objective": task,
            "mode": mode,
            "target_paths": paths.clone(),
            "acceptance": acceptance.clone(),
            "recommended_sequence": [
                "prepare_harness_session",
                "orchestrate_change",
                "approval_granted",
                "task_dispatched",
                "mutation_applied",
                "verification_passed"
            ],
            "mutation_policy": "phase-2 approval can authorize dispatch; this tool still does not mutate files"
        }),
    );
    sections.insert(
        "preflight".to_owned(),
        json!({
            "state": final_state,
            "mutation_ready": mutation_ready,
            "blocker_count": blocker_count,
            "readiness": readiness,
            "blockers": readiness_payload.get("blockers").cloned().unwrap_or_else(|| json!([])),
            "verifier_checks": readiness_payload.get("verifier_checks").cloned().unwrap_or_else(|| json!([])),
        }),
    );
    sections.insert(
        "evidence_handles".to_owned(),
        json!({
            "change_request": evidence_handle(&change_payload),
            "readiness": evidence_handle(&readiness_payload),
        }),
    );
    sections.insert("audit_events".to_owned(), json!({ "events": events }));
    sections.insert("role_bindings".to_owned(), role_bindings());
    sections.insert(
        "approval_policy".to_owned(),
        json!({
            "mutation_requires_approval": true,
            "current_approval_state": if blocked {
                "not_applicable_blocked".to_owned()
            } else {
                approval_decision.map(ApprovalDecision::as_str).unwrap_or("requested").to_owned()
            },
            "approval_event_required_before": "task_dispatched",
            "approved_by": if approval_decision == Some(ApprovalDecision::Granted) { Some(approval_actor.as_str()) } else { None },
            "approval_reason": approval_reason.clone(),
            "approved_actions": approved_actions.clone(),
        }),
    );
    sections.insert(
        "data_ownership".to_owned(),
        json!({
            "project_root": "owns index data, project memories, project-local audit, artifacts, and run event logs",
            "worktree": "owns mutable file state and test execution side effects",
            "orchestration_run": "owns plan, target paths, approvals, claims, artifact handles, state, and event timeline",
            "host": "owns user chat, model choice, credentials, UI presentation, and broad workflow policy"
        }),
    );
    sections.insert(
        "dispatch_plan".to_owned(),
        json!({
            "dispatch_allowed": !blocked && approval_decision == Some(ApprovalDecision::Granted),
            "reason": if blocked {
                "preflight blocked; dispatch is not allowed"
            } else if approval_decision == Some(ApprovalDecision::Granted) {
                "approval recorded; a wrapped mutation may be dispatched with orchestration_run_id"
            } else if approval_decision == Some(ApprovalDecision::Denied) {
                "approval denied; dispatch is not allowed"
            } else {
                "approval is required before dispatch"
            },
            "next_required_event": if blocked {
                "plan_proposed"
            } else if approval_decision == Some(ApprovalDecision::Granted) {
                "task_dispatched"
            } else {
                "approval_granted"
            },
        }),
    );

    let result = make_handle_response(
        state,
        "orchestrate_change",
        None,
        format!("Dry-run orchestration plan for `{task}` reached `{final_state}`."),
        top_findings,
        0.89,
        next_actions,
        sections,
        paths.clone(),
        None,
        Some(arguments),
    )?;

    let logical_session = crate::session_context::SessionRequestContext::from_json(arguments);
    let active_surface = state.surface().as_label();
    state.record_recent_preflight_from_payload(
        "orchestrate_change",
        active_surface,
        logical_session.session_id.as_str(),
        arguments,
        &result.0,
    );

    if !blocked && approval_decision == Some(ApprovalDecision::Granted) {
        let analysis_id = result
            .0
            .get("analysis_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        state.record_orchestration_approval(
            logical_session.session_id.as_str(),
            run_id,
            approval_actor,
            paths,
            approved_actions,
            analysis_id,
        );
    }

    Ok(result)
}
