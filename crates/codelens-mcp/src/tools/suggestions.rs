//! Tool recommendation engine — static `suggest_next` tables, contextual
//! overrides (doom-loop / mutation / review / exploration), phase filters,
//! composite guidance for low-level chains, and harness-phase inference.
//!
//! Extracted from `tools/mod.rs` as of v1.9.32. The public API is re-exported
//! through `crate::tools::*` so existing consumers compile unchanged.

use crate::protocol::ToolTier;
use crate::tool_defs::{self, ToolProfile, ToolSurface};

pub(crate) const MUTATION_TOOLS: &[&str] = &[
    "rename_symbol",
    "replace_symbol_body",
    "replace_content",
    "replace_lines",
    "delete_lines",
    "insert_at_line",
    "insert_before_symbol",
    "insert_after_symbol",
    "insert_content",
    "replace",
    "create_text_file",
    "add_import",
    "refactor_extract_function",
    "refactor_inline_function",
    "refactor_move_to_file",
    "refactor_change_signature",
];

const REVIEW_TOOLS: &[&str] = &[
    "review_architecture",
    "review_changes",
    "diagnose_issues",
    "cleanup_duplicate_logic",
    "get_changed_files",
    "get_impact_analysis",
    "find_scoped_references",
];

const EXPLORATION_TOOLS: &[&str] = &[
    "explore_codebase",
    "trace_request_path",
    "get_symbols_overview",
    "get_project_structure",
    "onboard_project",
    "get_current_config",
];

fn push_canonical_suggestion(suggestions: &mut Vec<String>, name: &str) {
    if let Some(name) = tool_defs::canonical_tool_name(name)
        && !suggestions.iter().any(|suggestion| suggestion == name)
    {
        suggestions.push(name.to_owned());
    }
}

fn filter_suggestions_for_phase(mut suggestions: Vec<String>, phase: &str) -> Vec<String> {
    let Some(phase_tools) = tool_defs::phase_tool_names_from_label(phase) else {
        return suggestions;
    };
    suggestions.retain(|suggestion| phase_tools.contains(&suggestion.as_str()));
    suggestions
}

/// Infer the harness phase from recent tool usage when the client has not
/// supplied `_harness_phase` explicitly.
///
/// Walks the most recent tools (newest first) and returns the first
/// distinctive phase signal it finds. Priority order is
/// `build` > `review` > `plan` because progression is one-way: once an
/// agent has reached a mutation, it is building; mutations followed by
/// review tools means the build is done and review is active.
pub(crate) fn infer_harness_phase(recent_tools: &[String]) -> Option<&'static str> {
    // Distinctive signals — tools that strongly indicate a single phase.
    // These are a subset of PLAN_/BUILD_/REVIEW_/EVAL_PHASE_TOOLS filtered
    // down to entries that do not appear in multiple phase lists.
    const BUILD_SIGNAL: &[&str] = &[
        "rename_symbol",
        "replace_symbol_body",
        "replace",
        "replace_content",
        "insert_content",
        "insert_at_line",
        "delete_lines",
        "add_import",
        "create_text_file",
        "refactor_extract_function",
        "refactor_inline_function",
        "refactor_move_to_file",
        "refactor_change_signature",
        "plan_safe_refactor",
        "trace_request_path",
    ];
    const REVIEW_SIGNAL: &[&str] = &[
        "diff_aware_references",
        "semantic_code_review",
        "refactor_safety_report",
        "dead_code_report",
        "find_circular_dependencies",
        "unresolved_reference_check",
        "find_misplaced_code",
        "find_code_duplicates",
        "review_changes",
        "review_architecture",
    ];
    const PLAN_SIGNAL: &[&str] = &[
        "analyze_change_request",
        "find_minimal_context_for_change",
        "onboard_project",
        "explore_codebase",
    ];

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
        if let Some(name) = tool_defs::canonical_tool_name("get_file_diagnostics") {
            suggestions.insert(0, name.to_owned());
        }
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
            push_canonical_suggestion(&mut suggestions, "verify_change_readiness");
            suggestions.truncate(4);
        }
    }

    // During review workflow: boost review-oriented tools
    let recent_has_review = recent_tools
        .iter()
        .any(|t| REVIEW_TOOLS.contains(&t.as_str()));
    if recent_has_review
        && !MUTATION_TOOLS.contains(&tool_name)
        && !suggestions.contains(&"get_impact_analysis".to_owned())
    {
        push_canonical_suggestion(&mut suggestions, "get_impact_analysis");
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
        push_canonical_suggestion(&mut suggestions, "get_ranked_context");
        suggestions.truncate(3);
    }

    // Filter suggestions by harness phase if specified
    if let Some(phase) = harness_phase {
        let filtered = filter_suggestions_for_phase(suggestions.clone(), phase);
        suggestions = filtered;
        // Ensure we always have at least 1 suggestion
        if suggestions.is_empty() {
            suggestions = suggest_next(tool_name).unwrap_or_default();
        }
    }

    Some(suggestions)
}

fn is_workflow_tool_name(name: &str) -> bool {
    matches!(
        tool_defs::tool_definition(name)
            .and_then(|tool| tool.annotations.as_ref())
            .and_then(|annotations| annotations.tier),
        Some(ToolTier::Workflow)
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

fn composite_suggestions_for_surface(surface: ToolSurface) -> Vec<&'static str> {
    match surface {
        ToolSurface::Profile(ToolProfile::PlannerReadonly) => {
            tool_defs::canonical_surface_tool_names(
                surface,
                &[
                    "explore_codebase",
                    "review_architecture",
                    "review_changes",
                    "plan_safe_refactor",
                ],
            )
        }
        ToolSurface::Profile(ToolProfile::ReviewerGraph)
        | ToolSurface::Profile(ToolProfile::CiAudit) => tool_defs::canonical_surface_tool_names(
            surface,
            &[
                "review_architecture",
                "review_changes",
                "cleanup_duplicate_logic",
                "diagnose_issues",
            ],
        ),
        ToolSurface::Profile(ToolProfile::RefactorFull) => tool_defs::canonical_surface_tool_names(
            surface,
            &[
                "plan_safe_refactor",
                "review_changes",
                "trace_request_path",
                "review_architecture",
            ],
        ),
        ToolSurface::Profile(ToolProfile::EvaluatorCompact) => {
            tool_defs::canonical_surface_tool_names(
                surface,
                &[
                    "verify_change_readiness",
                    "get_file_diagnostics",
                    "find_tests",
                ],
            )
        }
        ToolSurface::Profile(ToolProfile::WorkflowFirst) => {
            tool_defs::canonical_surface_tool_names(
                surface,
                &[
                    "explore_codebase",
                    "review_architecture",
                    "plan_safe_refactor",
                    "review_changes",
                    "diagnose_issues",
                ],
            )
        }
        ToolSurface::Profile(ToolProfile::BuilderMinimal) | ToolSurface::Preset(_) => {
            tool_defs::canonical_surface_tool_names(
                surface,
                &[
                    "explore_codebase",
                    "trace_request_path",
                    "plan_safe_refactor",
                    "review_changes",
                ],
            )
        }
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
        .into_iter()
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

fn raw_suggest_next(tool_name: &str) -> Option<&'static [&'static str]> {
    let suggestions: &[&str] = match tool_name {
        // ── Symbols / index ──────────────────────────────────────────
        "get_symbols_overview" => &["find_symbol", "get_impact_analysis", "get_ranked_context"],
        "find_symbol" => &[
            "find_referencing_symbols",
            "get_impact_analysis",
            "replace_symbol_body",
        ],
        "get_ranked_context" => &["find_symbol", "replace_symbol_body", "semantic_search"],
        "refresh_symbol_index" => &["index_embeddings", "get_symbols_overview"],
        "get_project_structure" => &["get_symbols_overview", "get_ranked_context", "find_symbol"],
        "get_complexity" => &["find_symbol", "get_symbols_overview"],
        "search_symbols_fuzzy" => &["find_symbol", "get_ranked_context"],

        // ── LSP ──────────────────────────────────────────────────────
        "find_referencing_symbols" => &["get_impact_analysis", "rename_symbol"],
        "get_file_diagnostics" => &["find_symbol", "get_symbols_overview"],
        "search_workspace_symbols" => &["find_symbol", "get_symbols_overview"],
        "get_type_hierarchy" => &["find_referencing_symbols", "get_symbols_overview"],
        "plan_symbol_rename" => &["rename_symbol"],
        "check_lsp_status" => &["get_capabilities", "get_file_diagnostics"],
        "get_lsp_recipe" => &["check_lsp_status"],

        // ── Graph / analysis ─────────────────────────────────────────
        "get_changed_files" => &["get_impact_analysis", "get_symbols_overview"],
        "get_impact_analysis" => &["find_referencing_symbols", "get_symbols_overview"],
        "find_importers" => &["get_impact_analysis", "get_symbol_importance"],
        "get_symbol_importance" => &["find_importers", "get_impact_analysis"],
        "find_dead_code" => &["get_symbols_overview", "delete_lines"],
        "find_circular_dependencies" => &["get_impact_analysis", "get_symbols_overview"],
        "get_change_coupling" => &["get_impact_analysis", "find_dead_code"],
        "get_callers" => &["get_callees", "find_symbol"],
        "get_callees" => &["get_callers", "find_symbol"],
        "find_scoped_references" => &["rename_symbol", "find_referencing_symbols"],

        // ── Filesystem ───────────────────────────────────────────────
        "get_current_config" => &["get_capabilities", "get_project_structure"],
        "read_file" => &["get_symbols_overview", "find_symbol"],
        "search_for_pattern" => &["find_referencing_symbols", "get_ranked_context"],
        "find_annotations" => &["get_symbols_overview", "find_symbol"],
        "find_tests" => &["get_symbols_overview"],

        // ── Mutation ─────────────────────────────────────────────────
        "rename_symbol" => &[
            "safe_rename_report",
            "unresolved_reference_check",
            "get_file_diagnostics",
        ],
        "replace_symbol_body" => &["find_symbol", "get_file_diagnostics"],
        "replace_content" => &["get_file_diagnostics", "get_symbols_overview"],
        "replace_lines" => &["get_file_diagnostics"],
        "delete_lines" => &["get_file_diagnostics"],
        "insert_at_line" => &["get_file_diagnostics"],
        "insert_before_symbol" => &["get_file_diagnostics", "find_symbol"],
        "insert_after_symbol" => &["get_file_diagnostics", "find_symbol"],
        "insert_content" => &["get_file_diagnostics", "find_symbol"],
        "replace" => &["get_file_diagnostics", "get_symbols_overview"],
        "create_text_file" => &["verify_change_readiness", "get_symbols_overview"],
        "add_import" => &["get_file_diagnostics", "analyze_missing_imports"],
        "analyze_missing_imports" => &["add_import"],

        // ── Memory ───────────────────────────────────────────────────
        "write_memory" => &["list_memories", "read_memory"],
        "read_memory" => &["write_memory", "list_memories"],
        "list_memories" => &["read_memory", "write_memory"],

        // ── Session ──────────────────────────────────────────────────
        "activate_project" => &[
            "get_project_structure",
            "get_current_config",
            "get_capabilities",
        ],
        "prepare_harness_session" => &[
            "get_current_config",
            "get_capabilities",
            "get_ranked_context",
        ],
        "explore_codebase" => &["find_symbol", "review_architecture", "review_changes"],
        "trace_request_path" => &["plan_safe_refactor", "find_symbol", "review_changes"],
        "review_architecture" => &["review_changes", "explore_codebase", "plan_safe_refactor"],
        "plan_safe_refactor" => &[
            "trace_request_path",
            "review_changes",
            "get_file_diagnostics",
        ],
        "cleanup_duplicate_logic" => &[
            "review_changes",
            "review_architecture",
            "get_analysis_section",
        ],
        "review_changes" => &["get_analysis_section", "impact_report", "diagnose_issues"],
        "diagnose_issues" => &["review_changes", "find_symbol", "verify_change_readiness"],
        "onboard_project" => &["get_ranked_context", "find_symbol", "get_capabilities"],
        "get_watch_status" => &["refresh_symbol_index", "prune_index_failures"],
        "prune_index_failures" => &["get_watch_status", "refresh_symbol_index"],
        "list_queryable_projects" => &["add_queryable_project", "query_project"],
        "add_queryable_project" => &["query_project", "list_queryable_projects"],
        "query_project" => &["find_symbol", "list_queryable_projects"],
        "set_preset" => &["get_capabilities"],
        "get_capabilities" => &[
            "get_project_structure",
            "get_ranked_context",
            "check_lsp_status",
        ],
        "get_tool_metrics" => &[
            "audit_builder_session",
            "export_session_markdown",
            "get_capabilities",
        ],
        "audit_builder_session" => &[
            "get_tool_metrics",
            "export_session_markdown",
            "list_active_agents",
        ],

        // ── Semantic ─────────────────────────────────────────────────
        "semantic_search" => &["find_symbol", "get_symbols_overview", "find_similar_code"],
        "index_embeddings" => &[
            "semantic_search",
            "find_code_duplicates",
            "find_misplaced_code",
        ],
        "find_similar_code" => &["get_symbols_overview", "semantic_search"],
        "find_code_duplicates" => &["find_similar_code", "get_symbols_overview"],
        "classify_symbol" => &["find_similar_code", "get_symbols_overview"],
        "find_misplaced_code" => &["get_symbols_overview", "find_similar_code"],

        // ── Composite ────────────────────────────────────────────────
        "summarize_file" => &["get_symbols_overview", "find_symbol"],
        "refactor_extract_function" => &["get_file_diagnostics", "find_symbol"],
        "refactor_inline_function" => &["get_file_diagnostics", "find_symbol"],
        "refactor_move_to_file" => &["get_file_diagnostics", "find_referencing_symbols"],
        "refactor_change_signature" => &["get_file_diagnostics", "find_referencing_symbols"],
        "propagate_deletions" => &[
            "delete_lines",
            "get_file_diagnostics",
            "get_impact_analysis",
        ],
        "analyze_change_request" => &[
            "get_analysis_section",
            "verify_change_readiness",
            "impact_report",
            "refactor_safety_report",
        ],
        "verify_change_readiness" => &[
            "get_analysis_section",
            "safe_rename_report",
            "unresolved_reference_check",
        ],
        "find_minimal_context_for_change" => &["get_analysis_section", "analyze_change_request"],
        "summarize_symbol_impact" => &["get_analysis_section", "safe_rename_report"],
        "module_boundary_report" => &[
            "get_analysis_section",
            "mermaid_module_graph",
            "impact_report",
            "dead_code_report",
        ],
        "mermaid_module_graph" => &[
            "get_analysis_section",
            "module_boundary_report",
            "impact_report",
        ],
        "safe_rename_report" => &[
            "get_analysis_section",
            "unresolved_reference_check",
            "rename_symbol",
            "refactor_safety_report",
        ],
        "unresolved_reference_check" => &[
            "get_analysis_section",
            "safe_rename_report",
            "find_referencing_symbols",
        ],
        "dead_code_report" => &["get_analysis_section", "impact_report"],
        "impact_report" => &["get_analysis_section", "diff_aware_references"],
        "refactor_safety_report" => &[
            "get_analysis_section",
            "verify_change_readiness",
            "safe_rename_report",
        ],
        "diff_aware_references" => &[
            "get_analysis_section",
            "impact_report",
            "semantic_code_review",
        ],
        "semantic_code_review" => &["get_analysis_section", "impact_report"],
        "start_analysis_job" => &["get_analysis_job"],
        "get_analysis_job" => &["get_analysis_section"],
        "cancel_analysis_job" => &["start_analysis_job"],

        _ => return None,
    };
    Some(suggestions)
}

pub fn suggest_next(tool_name: &str) -> Option<Vec<String>> {
    let suggestions = raw_suggest_next(tool_name)?;
    Some(
        tool_defs::canonical_tool_names(suggestions)
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
    )
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
            "find_minimal_context_for_change" => "Get smallest context needed for this task",
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
    use super::{
        filter_suggestions_for_phase, infer_harness_phase, raw_suggest_next,
        suggest_next_contextual,
    };

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

    #[test]
    fn public_tool_suggestions_only_reference_registered_tools() {
        for tool in crate::tool_defs::tools() {
            if let Some(suggestions) = raw_suggest_next(tool.name) {
                for suggested in suggestions {
                    assert!(
                        crate::tool_defs::tool_definition(suggested).is_some(),
                        "tool `{}` suggests unknown tool `{}`",
                        tool.name,
                        suggested
                    );
                }
            }
        }
    }

    #[test]
    fn contextual_suggestions_only_reference_registered_tools() {
        let recent = tools(&["rename_symbol", "review_changes", "explore_codebase"]);
        for tool in crate::tool_defs::tools() {
            if let Some(suggestions) = suggest_next_contextual(tool.name, &recent, Some("review")) {
                for suggested in suggestions {
                    assert!(
                        crate::tool_defs::tool_definition(&suggested).is_some(),
                        "tool `{}` suggests unknown tool `{}`",
                        tool.name,
                        suggested
                    );
                }
            }
        }
    }

    #[test]
    fn canonical_phase_tool_sets_match_phase_labels() {
        for phase in ["plan", "build", "review", "eval"] {
            let phase_tools =
                crate::tool_defs::phase_tool_names_from_label(phase).expect("known phase");
            let expected = crate::tool_defs::tools()
                .iter()
                .filter_map(|tool| {
                    (crate::tool_defs::tool_phase_label(tool.name) == Some(phase))
                        .then_some(tool.name)
                })
                .collect::<Vec<_>>();
            assert_eq!(phase_tools, expected, "phase `{phase}` drifted");
        }
    }

    #[test]
    fn phase_filter_uses_canonical_phase_mapping() {
        let suggestions = vec![
            "find_symbol".to_owned(),
            "rename_symbol".to_owned(),
            "review_changes".to_owned(),
            "get_analysis_section".to_owned(),
        ];

        assert_eq!(
            filter_suggestions_for_phase(suggestions.clone(), "plan"),
            vec!["find_symbol".to_owned()]
        );
        assert_eq!(
            filter_suggestions_for_phase(suggestions.clone(), "build"),
            vec!["rename_symbol".to_owned()]
        );
        assert_eq!(
            filter_suggestions_for_phase(suggestions.clone(), "review"),
            vec!["review_changes".to_owned()]
        );
        assert_eq!(
            filter_suggestions_for_phase(suggestions.clone(), "eval"),
            vec!["get_analysis_section".to_owned()]
        );
        assert_eq!(
            filter_suggestions_for_phase(suggestions, "unknown"),
            vec![
                "find_symbol".to_owned(),
                "rename_symbol".to_owned(),
                "review_changes".to_owned(),
                "get_analysis_section".to_owned(),
            ]
        );
    }
}
