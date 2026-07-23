//! Tool recommendation engine — static `suggest_next` tables, contextual
//! overrides (doom-loop / mutation / review / exploration), phase filters,
//! composite guidance for low-level chains, and harness-phase inference.
//!
//! Extracted from `tools/mod.rs` as of v1.9.32. The public API is re-exported
//! through `crate::tools::*` so existing consumers compile unchanged.

mod chain_guidance;
#[cfg(test)]
mod phase_tests;
mod reasons;
mod static_next;

pub use chain_guidance::composite_guidance_for_chain;
pub use reasons::suggestion_reasons_for;
#[cfg(test)]
pub(crate) use static_next::SUGGEST_NEXT_TABLE;
pub use static_next::suggest_next;

/// Tools relevant during harness PLAN phase
pub(crate) const PLAN_PHASE_TOOLS: &[&str] = &[
    "explore_codebase",
    "review_architecture",
    "review_changes",
    "orchestrate_change",
    "analyze_change_request",
    "verify_change_readiness",
    "onboard_project",
    "get_ranked_context",
    "get_symbols_overview",
    "find_symbol",
    "impact_report",
    "module_boundary_report",
    "get_changed_files",
    "find_referencing_symbols",
    "get_type_hierarchy",
];

/// Tools relevant during harness BUILD phase.
///
/// Mutation entries are the pending-D3 dispatch-only editors (`replace_symbol_body`,
/// `rename_symbol`); the former line-edit family (`insert_content`, `replace`,
/// `create_text_file`, `add_import`) is tombstoned (#346, delegated to the
/// host-native Edit/Write tools) and `analyze_missing_imports` was removed in
/// v2.0, so all five were dropped here — a phantom in a phase filter can never
/// match a live `suggest_next` value (guarded by
/// `phase_and_signal_constants_only_reference_live_tools`).
pub(crate) const BUILD_PHASE_TOOLS: &[&str] = &[
    "explore_codebase",
    "trace_request_path",
    "plan_safe_refactor",
    "find_symbol",
    "get_symbols_overview",
    "get_ranked_context",
    "find_referencing_symbols",
    "get_file_diagnostics",
    "replace_symbol_body",
    "rename_symbol",
    "find_tests",
    "refresh_symbol_index",
    "verify_change_readiness",
];

/// Tools relevant during harness REVIEW phase.
///
/// `semantic_code_review` is an analysis *kind* (never a callable tool),
/// `find_dead_code` was superseded by `dead_code_report`, and
/// `find_circular_dependencies` is an engine helper reached only through
/// `module_boundary_report` — none are live tools, so they were dropped as inert
/// filter entries (guarded by `phase_and_signal_constants_only_reference_live_tools`).
pub(crate) const REVIEW_PHASE_TOOLS: &[&str] = &[
    "review_architecture",
    "cleanup_duplicate_logic",
    "review_changes",
    "diagnose_issues",
    "verify_change_readiness",
    "get_file_diagnostics",
    "find_scoped_references",
    "impact_report",
    "refactor_safety_report",
    "diff_aware_references",
    "dead_code_report",
    "get_changed_files",
    "find_tests",
    "unresolved_reference_check",
    "audit_builder_session",
    "audit_planner_session",
    "export_session_markdown",
];

/// Tools relevant during harness EVAL phase
pub(crate) const EVAL_PHASE_TOOLS: &[&str] = &[
    "review_changes",
    "diagnose_issues",
    "verify_change_readiness",
    "get_file_diagnostics",
    "get_changed_files",
    "find_tests",
    "get_symbols_overview",
    "find_symbol",
    "read_file",
    "get_analysis_section",
];

/// Tools whose presence in the recent-call trail marks a mutation just
/// happened (so `get_file_diagnostics` is hoisted first).
///
/// Only the pending-D3 dispatch-only editors remain: the line-edit family
/// (`replace_content`, `replace_lines`, `delete_lines`, `insert_at_line`,
/// `insert_content`, `replace`, `create_text_file`, `add_import`) is tombstoned
/// (#346) and can never appear as a live recent-tool call, so keeping it here was
/// inert (guarded by `phase_and_signal_constants_only_reference_live_tools`).
pub(crate) const MUTATION_TOOLS: &[&str] = &[
    "rename_symbol",
    "replace_symbol_body",
    "insert_before_symbol",
    "insert_after_symbol",
    "refactor_extract_function",
    "refactor_inline_function",
    "refactor_move_to_file",
    "refactor_change_signature",
];

pub(crate) const REVIEW_TOOLS: &[&str] = &[
    "review_architecture",
    "review_changes",
    "diagnose_issues",
    "cleanup_duplicate_logic",
    "get_changed_files",
    "find_scoped_references",
];

/// Tools whose presence in the recent-call trail marks an exploration context
/// (so deeper-context tools like `get_ranked_context` get boosted).
///
/// `get_project_structure` was removed from the surface in v2.0 (handler retained
/// but unregistered), so it was dropped as an inert matcher entry — its live
/// stand-ins `explore_codebase` / `get_symbols_overview` remain.
pub(crate) const EXPLORATION_TOOLS: &[&str] = &[
    "explore_codebase",
    "trace_request_path",
    "get_symbols_overview",
    "onboard_project",
    "get_current_config",
];

// Distinctive phase signals — tools that strongly indicate a single phase.
// These are a subset of PLAN_/BUILD_/REVIEW_/EVAL_PHASE_TOOLS filtered down to
// entries that do not appear in multiple phase lists. Hoisted to module scope
// (like `SUGGEST_NEXT_TABLE`) so the drift gate can cross-check every member
// against the live registry. Each list matches against real recent-tool calls,
// so any phantom / tombstoned name would be a dead string that can never fire —
// guarded by `phase_and_signal_constants_only_reference_live_tools`.

/// Recent-call signals for the BUILD phase (pending-D3 dispatch-only editors +
/// refactor/plan tools). The tombstoned line-edit family was dropped: those
/// names are rejected at dispatch, so they never reach the recent-call trail.
pub(crate) const BUILD_SIGNAL: &[&str] = &[
    "rename_symbol",
    "replace_symbol_body",
    "refactor_extract_function",
    "refactor_inline_function",
    "refactor_move_to_file",
    "refactor_change_signature",
    "plan_safe_refactor",
    "trace_request_path",
];

/// Recent-call signals for the REVIEW phase. `semantic_code_review` (an analysis
/// kind, not a tool) and `find_circular_dependencies` (an engine helper) were
/// dropped as inert entries.
pub(crate) const REVIEW_SIGNAL: &[&str] = &[
    "diff_aware_references",
    "refactor_safety_report",
    "dead_code_report",
    "unresolved_reference_check",
    "find_misplaced_code",
    "find_code_duplicates",
    "review_changes",
    "review_architecture",
];

/// Recent-call signals for the PLAN phase. `analyze_change_impact` is the removed
/// v2.0 alias kept **deliberately** for historical-name matching: an agent still
/// emitting the legacy name is inferred into the plan phase. It is allow-listed
/// in the drift gate through `INTENTIONAL_ALIAS_KEYS`.
pub(crate) const PLAN_SIGNAL: &[&str] = &[
    "orchestrate_change",
    "analyze_change_request",
    "onboard_project",
    "explore_codebase",
    "analyze_change_impact",
];

/// Infer the harness phase from recent tool usage when the client has not
/// supplied `_harness_phase` explicitly.
///
/// Walks the most recent tools (newest first) and returns the first
/// distinctive phase signal it finds. Priority order is
/// `build` > `review` > `plan` because progression is one-way: once an
/// agent has reached a mutation, it is building; mutations followed by
/// review tools means the build is done and review is active.
pub(crate) fn infer_harness_phase(recent_tools: &[String]) -> Option<&'static str> {
    // Look at up to the 5 most recent tools. The most recent call is the
    // last entry when `push_recent_tool_for_session` appends in order.
    for tool in recent_tools.iter().rev().take(5) {
        let t = tool.as_str();
        if BUILD_SIGNAL.contains(&t) {
            return Some("build");
        }
        if REVIEW_SIGNAL.contains(&t) {
            return Some("review");
        }
        if PLAN_SIGNAL.contains(&t) {
            return Some("plan");
        }
    }
    None
}

fn phase_tools(phase: &str) -> Option<&'static [&'static str]> {
    match phase {
        "plan" => Some(PLAN_PHASE_TOOLS),
        "build" => Some(BUILD_PHASE_TOOLS),
        "review" => Some(REVIEW_PHASE_TOOLS),
        "eval" => Some(EVAL_PHASE_TOOLS),
        _ => None,
    }
}

pub(crate) fn retain_phase_compatible_suggestions(
    suggestions: &mut Vec<String>,
    harness_phase: Option<&str>,
) {
    let Some(allowed) = harness_phase.and_then(phase_tools) else {
        return;
    };
    suggestions.retain(|suggestion| allowed.contains(&suggestion.as_str()));
}

/// Context-aware tool suggestions: overrides static suggestions based on recent workflow.
pub fn suggest_next_contextual(
    tool_name: &str,
    recent_tools: &[String],
    harness_phase: Option<&str>,
) -> Option<Vec<String>> {
    let mut suggestions = suggest_next(tool_name)?;

    // After any mutation tool: always put get_file_diagnostics first
    let recent_has_mutation = recent_tools
        .iter()
        .any(|t| MUTATION_TOOLS.contains(&t.as_str()));
    if recent_has_mutation || MUTATION_TOOLS.contains(&tool_name) {
        suggestions.retain(|s| s != "get_file_diagnostics");
        suggestions.insert(0, "get_file_diagnostics".to_owned());
        suggestions.truncate(3);
    }

    // Before mutation: suggest verify_change_readiness after exploration/review
    // so agents run preflight before editing code
    if !MUTATION_TOOLS.contains(&tool_name)
        && !suggestions.contains(&"verify_change_readiness".to_owned())
    {
        let is_pre_mutation_context = REVIEW_TOOLS.contains(&tool_name)
            || EXPLORATION_TOOLS.contains(&tool_name)
            || tool_name == "get_ranked_context"
            || tool_name == "find_symbol"
            || tool_name == "find_referencing_symbols";
        if is_pre_mutation_context {
            suggestions.push("verify_change_readiness".to_owned());
            suggestions.truncate(4);
        }
    }

    // During review workflow: boost review-oriented tools
    let recent_has_review = recent_tools
        .iter()
        .any(|t| REVIEW_TOOLS.contains(&t.as_str()));
    if recent_has_review
        && !MUTATION_TOOLS.contains(&tool_name)
        && !suggestions.contains(&"impact_report".to_owned())
    {
        suggestions.push("impact_report".to_owned());
        suggestions.truncate(3);
    }

    // During exploration: boost deeper exploration tools
    let recent_has_exploration = recent_tools
        .iter()
        .any(|t| EXPLORATION_TOOLS.contains(&t.as_str()));
    if recent_has_exploration
        && !MUTATION_TOOLS.contains(&tool_name)
        && !REVIEW_TOOLS.contains(&tool_name)
        && !suggestions.contains(&"get_ranked_context".to_owned())
    {
        suggestions.push("get_ranked_context".to_owned());
        suggestions.truncate(3);
    }

    retain_phase_compatible_suggestions(&mut suggestions, harness_phase);

    (!suggestions.is_empty()).then_some(suggestions)
}
