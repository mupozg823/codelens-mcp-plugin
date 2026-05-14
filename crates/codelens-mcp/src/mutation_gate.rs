//! Mutation gate policy — determines whether a mutation tool call is allowed
//! based on preflight analysis, staleness, surface context, and blocker checks.

use crate::state::AppState;
use crate::tool_defs::{ToolProfile, ToolSurface};
use crate::util::now_ms;

#[derive(Clone)]
pub(crate) struct MutationGateAllowance {
    pub(crate) caution: bool,
}

/// Named failure taxonomy (NLAH paper pattern) for structured recovery.
#[derive(Clone, Copy, Debug)]
pub(crate) enum MutationFailureKind {
    /// No preflight run at all — first attempt
    MissingPreflight,
    /// Preflight exists but expired (TTL exceeded)
    StalePreflight,
    /// Preflight doesn't cover the mutation target path
    PathMismatch,
    /// rename_symbol without symbol-aware preflight
    SymbolPreflightRequired,
    /// Symbol hint doesn't match preflight evidence
    SymbolMismatch,
    /// Verifier explicitly blocked the mutation
    VerifierBlocked,
    /// No mutation target path provided
    NoTargetPath,
    /// Orchestrated mutation named a run without a recorded approval
    MissingApproval,
    /// Recorded orchestration approval has expired
    StaleApproval,
    /// Recorded approval does not cover the mutation path
    ApprovalPathMismatch,
}

pub(crate) struct MutationGateFailure {
    pub(crate) message: String,
    pub(crate) kind: MutationFailureKind,
    pub(crate) analysis_id: Option<String>,
    pub(crate) suggested_next_tools: Vec<String>,
    pub(crate) budget_hint: String,
}

pub(crate) fn is_verifier_source_tool(name: &str) -> bool {
    matches!(
        name,
        "orchestrate_change"
            | "verify_change_readiness"
            | "safe_rename_report"
            | "unresolved_reference_check"
            | "refactor_safety_report"
    )
}

pub(crate) fn is_refactor_gated_mutation_tool(name: &str) -> bool {
    matches!(
        name,
        "rename_symbol"
            | "replace_symbol_body"
            | "delete_lines"
            | "insert_at_line"
            | "insert_before_symbol"
            | "insert_after_symbol"
            | "insert_content"
            | "replace_content"
            | "replace_lines"
            | "replace"
            | "create_text_file"
            | "add_import"
            | "refactor_extract_function"
            | "refactor_inline_function"
            | "refactor_move_to_file"
            | "refactor_change_signature"
    )
}

fn is_symbol_aware_mutation_tool(name: &str) -> bool {
    matches!(name, "rename_symbol")
}

fn mutation_gate_failure(
    name: &str,
    reason: impl Into<String>,
    kind: MutationFailureKind,
    analysis_id: Option<String>,
) -> MutationGateFailure {
    let suggested_next_tools = if is_symbol_aware_mutation_tool(name) {
        vec![
            "safe_rename_report".to_owned(),
            "unresolved_reference_check".to_owned(),
            "get_analysis_section".to_owned(),
        ]
    } else {
        vec![
            "verify_change_readiness".to_owned(),
            "get_analysis_section".to_owned(),
            "get_file_diagnostics".to_owned(),
        ]
    };
    let budget_hint = if is_symbol_aware_mutation_tool(name) {
        "Run symbol-aware preflight before rename, then expand evidence if the target is ambiguous."
            .to_owned()
    } else {
        "Run preflight first, then expand verifier evidence before mutation.".to_owned()
    };
    MutationGateFailure {
        message: reason.into(),
        kind,
        analysis_id,
        suggested_next_tools,
        budget_hint,
    }
}

pub(crate) fn evaluate_mutation_gate(
    state: &AppState,
    name: &str,
    session: &crate::session_context::SessionRequestContext,
    surface: ToolSurface,
    arguments: &serde_json::Value,
) -> Result<Option<MutationGateAllowance>, MutationGateFailure> {
    let is_gated_surface = matches!(
        surface,
        ToolSurface::Profile(ToolProfile::RefactorFull)
            | ToolSurface::Profile(ToolProfile::BuilderMinimal)
    );
    if !is_gated_surface || !is_refactor_gated_mutation_tool(name) {
        return Ok(None);
    }

    let _logical_session = session.session_id.as_str();
    let logical_session = session.session_id.as_str();
    let Some(preflight) = state.recent_preflight_for_session(session, logical_session) else {
        return Err(mutation_gate_failure(
            name,
            format!(
                "Tool `{name}` requires a fresh preflight in this surface. Run `verify_change_readiness` first."
            ),
            MutationFailureKind::MissingPreflight,
            None,
        ));
    };

    if now_ms().saturating_sub(preflight.timestamp_ms) > crate::state::preflight_ttl_ms() {
        return Err(mutation_gate_failure(
            name,
            format!(
                "Tool `{name}` is blocked because the last `{}` preflight from surface `{}` is stale. Re-run verifier tools within {} seconds before mutating.",
                preflight.tool_name,
                preflight.surface,
                state.preflight_ttl_seconds()
            ),
            MutationFailureKind::StalePreflight,
            preflight.analysis_id.clone(),
        ));
    }

    let mutation_paths = state.extract_target_paths(arguments);
    if mutation_paths.is_empty() {
        return Err(mutation_gate_failure(
            name,
            format!(
                "Tool `{name}` is blocked because no mutation target path was provided for preflight matching."
            ),
            MutationFailureKind::NoTargetPath,
            preflight.analysis_id.clone(),
        ));
    }
    let path_overlap = mutation_paths
        .iter()
        .any(|path| preflight.target_paths.iter().any(|target| target == path));
    if !path_overlap {
        return Err(mutation_gate_failure(
            name,
            format!(
                "Tool `{name}` is blocked because the recent preflight does not cover the requested target paths."
            ),
            MutationFailureKind::PathMismatch,
            preflight.analysis_id.clone(),
        ));
    }

    if let Some(run_id) = arguments
        .get("orchestration_run_id")
        .and_then(|value| value.as_str())
    {
        let Some(approval) =
            state.orchestration_approval_for_session(session, logical_session, run_id)
        else {
            return Err(MutationGateFailure {
                message: format!(
                    "Tool `{name}` is blocked because orchestration run `{run_id}` has no recorded approval. Re-run `orchestrate_change` with approval.decision=granted before dispatching mutation."
                ),
                kind: MutationFailureKind::MissingApproval,
                analysis_id: preflight.analysis_id.clone(),
                suggested_next_tools: vec![
                    "orchestrate_change".to_owned(),
                    "get_analysis_section".to_owned(),
                    "verify_change_readiness".to_owned(),
                ],
                budget_hint: "Record approval on the orchestration run, then retry the bounded mutation with the same orchestration_run_id.".to_owned(),
            });
        };

        if now_ms().saturating_sub(approval.timestamp_ms) > crate::state::preflight_ttl_ms() {
            return Err(MutationGateFailure {
                message: format!(
                    "Tool `{name}` is blocked because orchestration run `{}` approval is stale. Re-run `orchestrate_change` and record a fresh approval.",
                    approval.run_id
                ),
                kind: MutationFailureKind::StaleApproval,
                analysis_id: approval
                    .analysis_id
                    .clone()
                    .or(preflight.analysis_id.clone()),
                suggested_next_tools: vec![
                    "orchestrate_change".to_owned(),
                    "verify_change_readiness".to_owned(),
                    "get_analysis_section".to_owned(),
                ],
                budget_hint:
                    "Refresh the orchestration approval within the preflight TTL before mutating."
                        .to_owned(),
            });
        }

        let action_allowed = approval.approved_actions.is_empty()
            || approval.approved_actions.iter().any(|action| {
                action == "*"
                    || action == "mutation"
                    || action == name
                    || (action == "content_mutation" && !is_symbol_aware_mutation_tool(name))
            });
        if !action_allowed {
            return Err(MutationGateFailure {
                message: format!(
                    "Tool `{name}` is blocked because orchestration run `{}` approval by `{}` does not include this action.",
                    approval.run_id, approval.actor
                ),
                kind: MutationFailureKind::MissingApproval,
                analysis_id: approval
                    .analysis_id
                    .clone()
                    .or(preflight.analysis_id.clone()),
                suggested_next_tools: vec![
                    "orchestrate_change".to_owned(),
                    "get_analysis_section".to_owned(),
                    "verify_change_readiness".to_owned(),
                ],
                budget_hint:
                    "Record approval for the requested mutation action before dispatching."
                        .to_owned(),
            });
        }

        let approval_covers_path = !approval.target_paths.is_empty()
            && mutation_paths.iter().any(|path| {
                approval
                    .target_paths
                    .iter()
                    .any(|approved_path| approved_path == path)
            });
        if !approval_covers_path {
            return Err(MutationGateFailure {
                message: format!(
                    "Tool `{name}` is blocked because orchestration run `{}` approval by `{}` does not cover the requested target paths.",
                    approval.run_id, approval.actor
                ),
                kind: MutationFailureKind::ApprovalPathMismatch,
                analysis_id: approval
                    .analysis_id
                    .clone()
                    .or(preflight.analysis_id.clone()),
                suggested_next_tools: vec![
                    "orchestrate_change".to_owned(),
                    "get_analysis_section".to_owned(),
                    "verify_change_readiness".to_owned(),
                ],
                budget_hint:
                    "Record approval for the exact target path set before dispatching mutation."
                        .to_owned(),
            });
        }
    }

    if is_symbol_aware_mutation_tool(name) {
        if !matches!(
            preflight.tool_name.as_str(),
            "safe_rename_report" | "unresolved_reference_check"
        ) {
            return Err(mutation_gate_failure(
                name,
                format!(
                    "Tool `{name}` requires a symbol-aware preflight. Run `safe_rename_report` or `unresolved_reference_check` first."
                ),
                MutationFailureKind::SymbolPreflightRequired,
                preflight.analysis_id.clone(),
            ));
        }
        let Some(mutation_symbol) = crate::state::extract_symbol_hint(arguments) else {
            return Err(mutation_gate_failure(
                name,
                format!(
                    "Tool `{name}` requires an exact symbol hint plus symbol-aware preflight evidence."
                ),
                MutationFailureKind::SymbolPreflightRequired,
                preflight.analysis_id.clone(),
            ));
        };
        if preflight
            .symbol
            .as_deref()
            .map(|symbol| symbol != mutation_symbol)
            .unwrap_or(true)
        {
            return Err(mutation_gate_failure(
                name,
                format!(
                    "Tool `{name}` is blocked because the symbol-aware preflight does not match `{mutation_symbol}`."
                ),
                MutationFailureKind::SymbolMismatch,
                preflight.analysis_id.clone(),
            ));
        }
    }

    if preflight.readiness.mutation_ready == "blocked" {
        return Err(mutation_gate_failure(
            name,
            format!(
                "Tool `{name}` is blocked by verifier readiness. The last `{}` preflight reported {} blocker(s); resolve them before mutation.",
                preflight.tool_name, preflight.blocker_count
            ),
            MutationFailureKind::VerifierBlocked,
            preflight.analysis_id.clone(),
        ));
    }

    Ok(Some(MutationGateAllowance {
        caution: preflight.readiness.mutation_ready == "caution",
    }))
}
