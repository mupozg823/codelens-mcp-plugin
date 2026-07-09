//! Tool recommendation engine — static `suggest_next` tables, contextual
//! overrides (doom-loop / mutation / review / exploration), phase filters,
//! composite guidance for low-level chains, and harness-phase inference.
//!
//! Extracted from `tools/mod.rs` as of v1.9.32. The public API is re-exported
//! through `crate::tools::*` so existing consumers compile unchanged.

use crate::tool_defs::{ToolProfile, ToolSurface};

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

    // Filter suggestions by harness phase if specified
    if let Some(phase) = harness_phase {
        let phase_tools: &[&str] = match phase {
            "plan" => PLAN_PHASE_TOOLS,
            "build" => BUILD_PHASE_TOOLS,
            "review" => REVIEW_PHASE_TOOLS,
            "eval" => EVAL_PHASE_TOOLS,
            _ => return Some(suggestions), // unknown phase, no filtering
        };
        suggestions.retain(|s| phase_tools.contains(&s.as_str()));
        // Ensure we always have at least 1 suggestion
        if suggestions.is_empty() {
            suggestions = suggest_next(tool_name).unwrap_or_default();
        }
    }

    Some(suggestions)
}

fn is_workflow_tool_name(name: &str) -> bool {
    // `analyze_change_impact` is the removed v2.0 alias retained for
    // historical-name matching (an agent still emitting the legacy name is
    // classified as a workflow tool, matching the `suggest_next` key carve-out).
    // The other removed aliases (`audit_security_context`,
    // `assess_change_readiness`) were dropped — they are neither live tools nor
    // intentional aliases, so classifying them was inert.
    matches!(
        name,
        "explore_codebase"
            | "trace_request_path"
            | "review_architecture"
            | "plan_safe_refactor"
            | "analyze_change_impact"
            | "cleanup_duplicate_logic"
            | "review_changes"
            | "diagnose_issues"
            | "orchestrate_change"
            | "analyze_change_request"
            | "verify_change_readiness"
            | "module_boundary_report"
            | "safe_rename_report"
            | "unresolved_reference_check"
            | "dead_code_report"
            | "impact_report"
            | "refactor_safety_report"
            | "diff_aware_references"
            | "start_analysis_job"
            | "get_analysis_job"
            | "cancel_analysis_job"
            | "get_analysis_section"
    )
}

fn has_recent_low_level_chain(recent_tools: &[String]) -> bool {
    if recent_tools.len() < 3 {
        return false;
    }
    recent_tools[recent_tools.len() - 3..]
        .iter()
        .all(|tool| !is_workflow_tool_name(tool))
}

fn composite_suggestions_for_surface(surface: ToolSurface) -> &'static [&'static str] {
    // Normalize deprecated profiles to their canonical core equivalent.
    let surface = match surface {
        ToolSurface::Profile(p) if p.is_deprecated() => ToolSurface::Profile(p.canonical()),
        other => other,
    };
    // Deprecated profiles resolve to their canonical core equivalent,
    // so all routing is unified through the core trio.
    match surface {
        ToolSurface::Profile(ToolProfile::PlannerReadonly) => &[
            "explore_codebase",
            "review_architecture",
            "review_changes",
            "plan_safe_refactor",
        ],
        ToolSurface::Profile(ToolProfile::ReviewerGraph) => &[
            "review_architecture",
            "review_changes",
            "cleanup_duplicate_logic",
            "diagnose_issues",
        ],
        ToolSurface::Profile(ToolProfile::BuilderMinimal) | ToolSurface::Preset(_) => &[
            "explore_codebase",
            "trace_request_path",
            "plan_safe_refactor",
            "review_changes",
        ],
        // Fallback for any remaining surface variants (should be unreachable
        // after canonical normalization above).
        _ => &[
            "explore_codebase",
            "review_architecture",
            "review_changes",
            "plan_safe_refactor",
        ],
    }
}

pub fn composite_guidance_for_chain(
    tool_name: &str,
    recent_tools: &[String],
    surface: ToolSurface,
) -> Option<(Vec<String>, String)> {
    if is_workflow_tool_name(tool_name) || !has_recent_low_level_chain(recent_tools) {
        return None;
    }

    let suggestions = composite_suggestions_for_surface(surface)
        .iter()
        .copied()
        .filter(|candidate| *candidate != tool_name)
        .take(3)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if suggestions.is_empty() {
        return None;
    }

    let hint = format!(
        "Repeated low-level chain detected. Prefer {} for compressed context before continuing.",
        suggestions.join(", ")
    );
    Some((suggestions, hint))
}

/// Static `(tool, suggested_next)` table backing [`suggest_next`].
///
/// Kept as an iterable table — instead of an opaque `match` — so the drift-gate
/// integration test (`suggestion_drift`) can cross-check every key and value
/// against the canonical `tools.toml` registry. That is what stops a future
/// rename/removal in `tools.toml` from silently leaving a suggestion pointing at
/// a tool the client can never call.
///
/// Invariants enforced by `suggest_next_table_only_references_live_tools`:
/// every **value** is a real `tools.toml` tool *or* a dispatch-only pending-D3
/// tool (callable via `tools/call` behind the `:7838` mutation gate but absent
/// from `tools.toml`, e.g. `rename_symbol`), and every **key** is a real tool
/// *or* the single documented deprecated alias `analyze_change_impact` (retained
/// so agents that still emit the legacy name get canonical next-step guidance —
/// guarded by `suggest_next_prefers_canonical_workflows`).
pub(crate) const SUGGEST_NEXT_TABLE: &[(&str, &[&str])] = &[
    // ── Symbols / index ──────────────────────────────────────────
    (
        "get_symbols_overview",
        &["find_symbol", "impact_report", "get_ranked_context"],
    ),
    (
        "find_symbol",
        &[
            "find_referencing_symbols",
            "find_declaration",
            "find_implementations",
            "impact_report",
        ],
    ),
    (
        "get_ranked_context",
        &["find_symbol", "plan_safe_refactor", "semantic_search"],
    ),
    (
        "refresh_symbol_index",
        &["index_embeddings", "get_symbols_overview"],
    ),
    ("get_complexity", &["find_symbol", "get_symbols_overview"]),
    (
        "search_symbols_fuzzy",
        &["find_symbol", "get_ranked_context"],
    ),
    // ── LSP ──────────────────────────────────────────────────────
    (
        "find_referencing_symbols",
        &["impact_report", "plan_symbol_rename"],
    ),
    (
        "find_declaration",
        &["find_referencing_symbols", "get_diagnostics_for_symbol"],
    ),
    (
        "find_implementations",
        &["find_referencing_symbols", "get_type_hierarchy"],
    ),
    (
        "get_diagnostics_for_symbol",
        &["get_file_diagnostics", "find_symbol"],
    ),
    (
        "get_file_diagnostics",
        &["find_symbol", "get_symbols_overview"],
    ),
    (
        "search_workspace_symbols",
        &["find_symbol", "get_symbols_overview"],
    ),
    (
        "get_type_hierarchy",
        &["find_referencing_symbols", "get_symbols_overview"],
    ),
    ("plan_symbol_rename", &["safe_rename_report"]),
    (
        "get_lsp_recipe",
        &["get_capabilities", "get_file_diagnostics"],
    ),
    // ── Graph / analysis ─────────────────────────────────────────
    (
        "get_changed_files",
        &["impact_report", "get_symbols_overview"],
    ),
    (
        "get_symbol_importance",
        &["find_referencing_symbols", "impact_report"],
    ),
    ("get_callers", &["get_callees", "find_symbol"]),
    ("get_callees", &["get_callers", "find_symbol"]),
    (
        "find_scoped_references",
        &["plan_symbol_rename", "find_referencing_symbols"],
    ),
    // ── Filesystem ───────────────────────────────────────────────
    (
        "get_current_config",
        &["get_capabilities", "get_symbols_overview"],
    ),
    ("read_file", &["get_symbols_overview", "find_symbol"]),
    ("find_annotations", &["get_symbols_overview", "find_symbol"]),
    ("find_tests", &["get_symbols_overview"]),
    // ── Memory ───────────────────────────────────────────────────
    ("write_memory", &["list_memories", "read_memory"]),
    ("read_memory", &["write_memory", "list_memories"]),
    ("list_memories", &["read_memory", "write_memory"]),
    // ── Session ──────────────────────────────────────────────────
    (
        "activate_project",
        &[
            "get_symbols_overview",
            "get_current_config",
            "get_capabilities",
        ],
    ),
    (
        "prepare_harness_session",
        &[
            "get_current_config",
            "get_capabilities",
            "get_ranked_context",
        ],
    ),
    (
        "explore_codebase",
        &["find_symbol", "review_architecture", "review_changes"],
    ),
    (
        "trace_request_path",
        &["plan_safe_refactor", "find_symbol", "review_changes"],
    ),
    (
        "review_architecture",
        &["review_changes", "explore_codebase", "plan_safe_refactor"],
    ),
    (
        "plan_safe_refactor",
        &[
            "trace_request_path",
            "review_changes",
            "get_file_diagnostics",
        ],
    ),
    // `analyze_change_impact` is a removed v2.0 alias — not a registered tool —
    // but its suggestion arm is deliberately retained so agents that still emit
    // the legacy name are routed to canonical workflows. Guarded by
    // `suggest_next_prefers_canonical_workflows`; allowlisted in the drift gate.
    (
        "analyze_change_impact",
        &[
            "review_architecture",
            "review_changes",
            "get_analysis_section",
        ],
    ),
    (
        "cleanup_duplicate_logic",
        &[
            "review_changes",
            "review_architecture",
            "get_analysis_section",
        ],
    ),
    (
        "review_changes",
        &["get_analysis_section", "impact_report", "diagnose_issues"],
    ),
    (
        "diagnose_issues",
        &["review_changes", "find_symbol", "verify_change_readiness"],
    ),
    (
        "onboard_project",
        &["get_ranked_context", "find_symbol", "get_capabilities"],
    ),
    (
        "get_watch_status",
        &["refresh_symbol_index", "prune_index_failures"],
    ),
    (
        "prune_index_failures",
        &["get_watch_status", "refresh_symbol_index"],
    ),
    (
        "list_queryable_projects",
        &["add_queryable_project", "query_project"],
    ),
    (
        "add_queryable_project",
        &["query_project", "list_queryable_projects"],
    ),
    ("query_project", &["find_symbol", "list_queryable_projects"]),
    ("set_preset", &["get_capabilities"]),
    (
        "get_capabilities",
        &["get_symbols_overview", "get_ranked_context"],
    ),
    (
        "get_tool_metrics",
        &[
            "audit_builder_session",
            "export_session_markdown",
            "get_capabilities",
        ],
    ),
    (
        "audit_builder_session",
        &[
            "get_tool_metrics",
            "export_session_markdown",
            "list_active_agents",
        ],
    ),
    // ── Semantic ─────────────────────────────────────────────────
    (
        "semantic_search",
        &["find_symbol", "get_symbols_overview", "find_similar_code"],
    ),
    (
        "index_embeddings",
        &[
            "semantic_search",
            "find_code_duplicates",
            "find_misplaced_code",
        ],
    ),
    (
        "find_similar_code",
        &["get_symbols_overview", "semantic_search"],
    ),
    (
        "find_code_duplicates",
        &["find_similar_code", "get_symbols_overview"],
    ),
    (
        "classify_symbol",
        &["find_similar_code", "get_symbols_overview"],
    ),
    (
        "find_misplaced_code",
        &["get_symbols_overview", "find_similar_code"],
    ),
    // ── Composite / analysis-handle ──────────────────────────────
    (
        "orchestrate_change",
        &[
            "get_analysis_section",
            "verify_change_readiness",
            "audit_planner_session",
        ],
    ),
    (
        "analyze_change_request",
        &[
            "orchestrate_change",
            "get_analysis_section",
            "verify_change_readiness",
            "impact_report",
            "refactor_safety_report",
        ],
    ),
    (
        "verify_change_readiness",
        &[
            "get_analysis_section",
            "safe_rename_report",
            "unresolved_reference_check",
        ],
    ),
    (
        "module_boundary_report",
        &[
            "get_analysis_section",
            "mermaid_module_graph",
            "impact_report",
            "dead_code_report",
        ],
    ),
    (
        "mermaid_module_graph",
        &[
            "get_analysis_section",
            "module_boundary_report",
            "impact_report",
        ],
    ),
    // `rename_symbol` is dispatch-only (ADR-0009/D3, #346): callable via
    // `tools/call` behind the `:7838` mutation gate but intentionally absent from
    // `tools.toml`. It is the sole `codex-builder`-tagged suggestion in this
    // table, so it must stay here to keep the planner→builder delegate handoff
    // firing (`inject_delegate_to_codex_builder_hint`). Allowlisted in the drift
    // gate via the pending-D3 carve-out.
    (
        "safe_rename_report",
        &[
            "get_analysis_section",
            "unresolved_reference_check",
            "rename_symbol",
            "refactor_safety_report",
        ],
    ),
    (
        "unresolved_reference_check",
        &[
            "get_analysis_section",
            "safe_rename_report",
            "find_referencing_symbols",
        ],
    ),
    (
        "dead_code_report",
        &["get_analysis_section", "impact_report"],
    ),
    (
        "impact_report",
        &["get_analysis_section", "diff_aware_references"],
    ),
    (
        "refactor_safety_report",
        &[
            "get_analysis_section",
            "verify_change_readiness",
            "safe_rename_report",
        ],
    ),
    (
        "diff_aware_references",
        &["get_analysis_section", "impact_report", "review_changes"],
    ),
    ("start_analysis_job", &["get_analysis_job"]),
    ("get_analysis_job", &["get_analysis_section"]),
    ("cancel_analysis_job", &["start_analysis_job"]),
];

pub fn suggest_next(tool_name: &str) -> Option<Vec<String>> {
    SUGGEST_NEXT_TABLE
        .iter()
        .find(|(name, _)| *name == tool_name)
        .map(|(_, tools)| tools.iter().map(|s| (*s).to_string()).collect())
}

/// Returns a map of tool name → brief reason explaining why it is suggested.
/// Called after `suggest_next_contextual` / doom-loop overrides have finalized the list.
pub fn suggestion_reasons_for(
    tools: &[String],
    _tool_name: &str,
) -> std::collections::HashMap<String, String> {
    let mut reasons = std::collections::HashMap::new();
    for tool in tools {
        let reason = match tool.as_str() {
            "delegate_to_codex_builder" => {
                "Hand off the next builder-heavy step to a Codex-class executor"
            }
            "get_file_diagnostics" => "Check for type errors or lint issues after this change",
            "get_analysis_section" => "Expand a specific section from the analysis handle",
            "verify_change_readiness" => "Validate mutation safety before editing code",
            "impact_report" => "Assess blast radius of the changes",
            "module_boundary_report" => "Check coupling and boundary violations",
            "safe_rename_report" => "Preview rename safety before executing",
            "diff_aware_references" => "Find references affected by recent changes",
            "dead_code_report" => "Identify unused code after refactoring",
            "find_referencing_symbols" => "Find all callers/users of this symbol",
            "get_ranked_context" => "Get relevant context ranked by multiple signals",
            "start_analysis_job" => "Run heavy analysis asynchronously",
            "orchestrate_change" => {
                "Dry-run the run state, approval boundary, and evidence handles"
            }
            "analyze_change_request" => "Compress the change request into ranked files and risks",
            "explore_codebase" => "Get a high-level overview or targeted search",
            "review_changes" => "Review impact of changed files before merge",
            "diagnose_issues" => "Check for diagnostics or unresolved references",
            _ => "Suggested as next step in the workflow chain",
        };
        reasons.insert(tool.clone(), reason.to_owned());
    }
    reasons
}

#[cfg(test)]
mod phase_inference_tests {
    use super::infer_harness_phase;

    fn tools(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn mutation_at_end_infers_build() {
        let recent = tools(&["find_symbol", "verify_change_readiness", "rename_symbol"]);
        assert_eq!(infer_harness_phase(&recent), Some("build"));
    }

    #[test]
    fn review_signal_without_mutation_infers_review() {
        let recent = tools(&["find_symbol", "dead_code_report", "get_symbols_overview"]);
        // Most-recent window scans in reverse; dead_code_report wins over plan-only tools.
        assert_eq!(infer_harness_phase(&recent), Some("review"));
    }

    #[test]
    fn plan_only_trail_infers_plan() {
        let recent = tools(&["onboard_project", "explore_codebase", "get_ranked_context"]);
        assert_eq!(infer_harness_phase(&recent), Some("plan"));
    }

    #[test]
    fn empty_recent_returns_none() {
        assert_eq!(infer_harness_phase(&[]), None);
    }

    #[test]
    fn unknown_tools_only_returns_none() {
        let recent = tools(&["my_custom_thing", "another_unknown"]);
        assert_eq!(infer_harness_phase(&recent), None);
    }

    #[test]
    fn only_most_recent_five_are_considered() {
        // Six tools: the oldest is a build signal, but it should be outside the window.
        let recent = tools(&[
            "rename_symbol", // oldest — outside window
            "find_symbol",
            "find_symbol",
            "find_symbol",
            "find_symbol",
            "find_symbol",
        ]);
        assert_eq!(infer_harness_phase(&recent), None);
    }

    #[test]
    fn most_recent_distinctive_signal_wins() {
        // Build signal is older, review signal is newer within the window.
        let recent = tools(&[
            "rename_symbol", // build (oldest of window)
            "find_symbol",
            "review_changes", // review (newer)
        ]);
        assert_eq!(infer_harness_phase(&recent), Some("review"));
    }
}
