//! Mutation gate policy — determines whether a mutation tool call is allowed
//! based on preflight analysis, staleness, surface context, and blocker checks.

use crate::state::AppState;
use crate::tool_defs::{ToolProfile, ToolSurface};

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
    /// Strict coordination mode requires an active claim for HTTP mutation sessions
    MissingClaim,
    /// Strict coordination mode claim coverage does not match the mutation target path
    ClaimPathMismatch,
    /// Strict coordination mode blocks when the latest preflight reported overlap
    OverlappingClaimsBlocked,
    /// rename_symbol without symbol-aware preflight
    SymbolPreflightRequired,
    /// Symbol hint doesn't match preflight evidence
    SymbolMismatch,
    /// Verifier explicitly blocked the mutation
    VerifierBlocked,
    /// No mutation target path provided
    NoTargetPath,
}

pub(crate) struct MutationGateFailure {
    pub(crate) message: String,
    pub(crate) kind: MutationFailureKind,
    pub(crate) analysis_id: Option<String>,
    pub(crate) suggested_next_tools: Vec<String>,
    pub(crate) budget_hint: String,
    pub(crate) stale: bool,
    pub(crate) rename_without_symbol_preflight: bool,
    pub(crate) missing_preflight: bool,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(crate) fn is_verifier_source_tool(name: &str) -> bool {
    matches!(
        name,
        "verify_change_readiness"
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
    stale: bool,
    rename_without_symbol_preflight: bool,
    missing_preflight: bool,
) -> MutationGateFailure {
    let suggested_next_tools = match kind {
        MutationFailureKind::MissingClaim | MutationFailureKind::ClaimPathMismatch => vec![
            "claim_files".to_owned(),
            "list_active_agents".to_owned(),
            "verify_change_readiness".to_owned(),
        ],
        MutationFailureKind::OverlappingClaimsBlocked => vec![
            "list_active_agents".to_owned(),
            "verify_change_readiness".to_owned(),
            "claim_files".to_owned(),
        ],
        _ if is_symbol_aware_mutation_tool(name) => vec![
            "safe_rename_report".to_owned(),
            "unresolved_reference_check".to_owned(),
            "get_analysis_section".to_owned(),
        ],
        _ => vec![
            "verify_change_readiness".to_owned(),
            "get_analysis_section".to_owned(),
            "get_file_diagnostics".to_owned(),
        ],
    };
    let budget_hint = match kind {
        MutationFailureKind::MissingClaim | MutationFailureKind::ClaimPathMismatch => {
            "Strict coordination is active for this HTTP builder session. Claim the intended files before mutating."
                .to_owned()
        }
        MutationFailureKind::OverlappingClaimsBlocked => {
            "Strict coordination blocks overlapping claims. Resolve the conflicting builder session before mutating."
                .to_owned()
        }
        _ if is_symbol_aware_mutation_tool(name) => {
            "Run symbol-aware preflight before rename, then expand evidence if the target is ambiguous."
                .to_owned()
        }
        _ => "Run preflight first, then expand verifier evidence before mutation.".to_owned(),
    };
    MutationGateFailure {
        message: reason.into(),
        kind,
        analysis_id,
        suggested_next_tools,
        budget_hint,
        stale,
        rename_without_symbol_preflight,
        missing_preflight,
    }
}

fn strict_coordination_applies(
    state: &AppState,
    session: &crate::session_context::SessionRequestContext,
    surface: ToolSurface,
) -> bool {
    matches!(state.coordination_mode(), crate::state::RuntimeCoordinationMode::Strict)
        && matches!(surface, ToolSurface::Profile(ToolProfile::RefactorFull))
        && matches!(
            state.transport_mode(),
            crate::state::RuntimeTransportMode::Http
        )
        && !session.is_local()
        && session.trusted_client
}

pub(crate) fn evaluate_mutation_gate(
    state: &AppState,
    name: &str,
    session: &crate::session_context::SessionRequestContext,
    surface: ToolSurface,
    arguments: &serde_json::Value,
) -> Result<Option<MutationGateAllowance>, MutationGateFailure> {
    if !matches!(surface, ToolSurface::Profile(ToolProfile::RefactorFull))
        || !is_refactor_gated_mutation_tool(name)
    {
        return Ok(None);
    }

    let _logical_session = session.session_id.as_str();
    let logical_session = session.session_id.as_str();
    let Some(preflight) = state.recent_preflight_for_session(session, logical_session) else {
        return Err(mutation_gate_failure(
            name,
            format!(
                "Tool `{name}` requires a fresh preflight in `refactor-full`. Run `verify_change_readiness` first."
            ),
            MutationFailureKind::MissingPreflight,
            None,
            false,
            false,
            true,
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
            true,
            false,
            false,
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
            false,
            is_symbol_aware_mutation_tool(name),
            false,
        ));
    }
    let strict_coordination = strict_coordination_applies(state, session, surface);
    let preflight_covers_any_path = mutation_paths
        .iter()
        .any(|path| preflight.target_paths.iter().any(|target| target == path));
    let preflight_covers_all_paths = mutation_paths
        .iter()
        .all(|path| preflight.target_paths.iter().any(|target| target == path));
    if (!strict_coordination && !preflight_covers_any_path)
        || (strict_coordination && !preflight_covers_all_paths)
    {
        return Err(mutation_gate_failure(
            name,
            if strict_coordination {
                format!(
                    "Tool `{name}` is blocked because strict coordination requires the recent preflight to cover every requested target path."
                )
            } else {
                format!(
                    "Tool `{name}` is blocked because the recent preflight does not cover the requested target paths."
                )
            },
            MutationFailureKind::PathMismatch,
            preflight.analysis_id.clone(),
            false,
            false,
            false,
        ));
    }

    if strict_coordination {
        let Some(active_claim) = state.active_claim_for_session(session) else {
            return Err(mutation_gate_failure(
                name,
                format!(
                    "Tool `{name}` is blocked because strict coordination requires an active `claim_files` reservation for this trusted HTTP builder session."
                ),
                MutationFailureKind::MissingClaim,
                preflight.analysis_id.clone(),
                false,
                false,
                false,
            ));
        };
        let claim_covers_all_paths = mutation_paths
            .iter()
            .all(|path| active_claim.paths.iter().any(|claimed| claimed == path));
        if !claim_covers_all_paths {
            return Err(mutation_gate_failure(
                name,
                format!(
                    "Tool `{name}` is blocked because the active file claim does not cover every requested mutation path."
                ),
                MutationFailureKind::ClaimPathMismatch,
                preflight.analysis_id.clone(),
                false,
                false,
                false,
            ));
        }
        if preflight.overlapping_claim_count > 0 {
            let conflicting_sessions = if preflight.overlapping_claim_session_ids.is_empty() {
                "unknown session".to_owned()
            } else {
                preflight.overlapping_claim_session_ids.join(", ")
            };
            let conflicting_paths = if preflight.overlapping_claim_paths.is_empty() {
                "unknown path".to_owned()
            } else {
                preflight.overlapping_claim_paths.join(", ")
            };
            return Err(mutation_gate_failure(
                name,
                format!(
                    "Tool `{name}` is blocked because the latest preflight reported overlapping claims from session(s) [{conflicting_sessions}] on path(s) [{conflicting_paths}]."
                ),
                MutationFailureKind::OverlappingClaimsBlocked,
                preflight.analysis_id.clone(),
                false,
                false,
                false,
            ));
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
                false,
                true,
                false,
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
                false,
                true,
                false,
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
                false,
                true,
                false,
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
            false,
            false,
            false,
        ));
    }

    Ok(Some(MutationGateAllowance {
        caution: preflight.readiness.mutation_ready == "caution",
    }))
}
