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
        stale,
        rename_without_symbol_preflight,
        missing_preflight,
    }
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

    let logical_session = session.session_id.as_str();
    let logical_session = session.session_id.as_str();
    let Some(preflight) = state.recent_preflight(logical_session) else {
        return Err(mutation_gate_failure(
            name,
            format!("Tool `{name}` requires a fresh preflight in `refactor-full`. Run `verify_change_readiness` first."),
            MutationFailureKind::MissingPreflight,
            None, false, false, true,
        ));
    };

    if now_ms().saturating_sub(preflight.timestamp_ms) > crate::state::preflight_ttl_ms() {
        return Err(mutation_gate_failure(
            name,
            format!("Tool `{name}` is blocked because the last `{}` preflight from surface `{}` is stale. Re-run verifier tools within {} seconds before mutating.",
                preflight.tool_name, preflight.surface, state.preflight_ttl_seconds()),
            MutationFailureKind::StalePreflight,
            preflight.analysis_id.clone(), true, false, false,
        ));
    }

    let mutation_paths = state.extract_target_paths(arguments);
    if mutation_paths.is_empty() {
        return Err(mutation_gate_failure(
            name,
            format!("Tool `{name}` is blocked because no mutation target path was provided for preflight matching."),
            MutationFailureKind::NoTargetPath,
            preflight.analysis_id.clone(), false, is_symbol_aware_mutation_tool(name), false,
        ));
    }
    let path_overlap = mutation_paths
        .iter()
        .any(|path| preflight.target_paths.iter().any(|target| target == path));
    if !path_overlap {
        return Err(mutation_gate_failure(
            name,
            format!("Tool `{name}` is blocked because the recent preflight does not cover the requested target paths."),
            MutationFailureKind::PathMismatch,
            preflight.analysis_id.clone(), false, false, false,
        ));
    }

    if is_symbol_aware_mutation_tool(name) {
        if !matches!(
            preflight.tool_name.as_str(),
            "safe_rename_report" | "unresolved_reference_check"
        ) {
            return Err(mutation_gate_failure(
                name,
                format!("Tool `{name}` requires a symbol-aware preflight. Run `safe_rename_report` or `unresolved_reference_check` first."),
                MutationFailureKind::SymbolPreflightRequired,
                preflight.analysis_id.clone(), false, true, false,
            ));
        }
        let Some(mutation_symbol) = state.extract_symbol_hint(arguments) else {
            return Err(mutation_gate_failure(
                name,
                format!("Tool `{name}` requires an exact symbol hint plus symbol-aware preflight evidence."),
                MutationFailureKind::SymbolPreflightRequired,
                preflight.analysis_id.clone(), false, true, false,
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
                format!("Tool `{name}` is blocked because the symbol-aware preflight does not match `{mutation_symbol}`."),
                MutationFailureKind::SymbolMismatch,
                preflight.analysis_id.clone(), false, true, false,
            ));
        }
    }

    if preflight.readiness.mutation_ready == "blocked" {
        return Err(mutation_gate_failure(
            name,
            format!("Tool `{name}` is blocked by verifier readiness. The last `{}` preflight reported {} blocker(s); resolve them before mutation.",
                preflight.tool_name, preflight.blocker_count),
            MutationFailureKind::VerifierBlocked,
            preflight.analysis_id.clone(), false, false, false,
        ));
    }

    Ok(Some(MutationGateAllowance {
        caution: preflight.readiness.mutation_ready == "caution",
    }))
}
