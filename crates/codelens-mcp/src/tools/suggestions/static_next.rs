// allow: SIZE_OK — declarative suggestion registry; drift-gated by suggestion_drift tests.

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
