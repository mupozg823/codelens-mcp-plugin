pub(crate) fn tool_deprecation(name: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match name {
        // Deprecated in v1.13.27, removal v2.0
        "find_circular_dependencies"
        | "find_redundant_definitions"
        | "audit_tool_surface_consistency"
        | "find_orphan_handlers"
        | "find_over_visible_apis"
        | "find_phantom_modules"
        | "search_for_pattern"
        | "analyze_missing_imports"
        | "add_import"
        | "refactor_extract_function"
        | "refactor_inline_function"
        | "refactor_move_to_file"
        | "refactor_change_signature"
        | "replace_symbol_body"
        | "replace_content"
        | "replace_lines"
        | "delete_lines"
        | "insert_at_line"
        | "insert_before_symbol"
        | "insert_after_symbol"
        | "insert_content"
        | "replace"
        | "create_text_file"
        | "rename_symbol"
        | "propagate_deletions"
        | "onboard_project"
        | "analyze_change_request"
        | "orchestrate_change" => Some(("1.13.27", "", "2.0")),
        _ => None,
    }
}

pub(crate) fn apply_tool_deprecation_meta(meta: &mut serde_json::Value, name: &str) {
    if let Some((since, replacement, removal)) = tool_deprecation(name) {
        meta["codelens/deprecatedSince"] = serde_json::json!(since);
        meta["codelens/deprecatedReplacement"] = serde_json::json!(replacement);
        meta["codelens/deprecatedRemovalTarget"] = serde_json::json!(removal);
    }
}

pub(crate) fn deprecated_workflow_alias(name: &str) -> Option<(&'static str, &'static str)> {
    tool_deprecation(name).map(|(_, replacement, removal)| (replacement, removal))
}

/// Phase alias per ADR-0005 step 4 — harness-phase scoping for `tools/list`
/// filter without introducing a second tool registry. Returning `None` marks
/// the tool as phase-agnostic (infrastructure / coordination).
pub(crate) fn tool_phase(name: &str) -> Option<crate::protocol::ToolPhase> {
    use crate::protocol::ToolPhase;
    match name {
        // Plan — analyze/retrieve/orient before deciding to edit.
        "explore_codebase"
        | "trace_request_path"
        | "review_architecture"
        | "plan_safe_refactor"
        | "plan_symbol_rename"
        | "analyze_change_impact"
        | "module_boundary_report"
        | "mermaid_module_graph"
        | "impact_report"
        | "get_impact_analysis"
        | "get_ranked_context"
        | "get_symbols_overview"
        | "find_symbol"
        | "find_referencing_symbols"
        | "search_symbols_fuzzy"
        | "search_workspace_symbols"
        | "get_type_hierarchy"
        | "semantic_search"
        | "index_embeddings"
        | "find_scoped_references"
        | "get_symbol_importance"
        | "get_complexity"
        | "find_similar_code"
        | "get_changed_files" => Some(ToolPhase::Plan),

        // Build — mutation surface.
        "cleanup_duplicate_logic" => Some(ToolPhase::Build),

        // Review — post-edit safety, verifier, diff-aware inspection, audits.
        "review_changes"
        | "diff_aware_references"
        | "verify_change_readiness"
        | "assess_change_readiness"
        | "safe_rename_report"
        | "refactor_safety_report"
        | "unresolved_reference_check"
        | "dead_code_report"
        | "find_dead_code"
        | "find_circular_dependencies"
        | "find_misplaced_code"
        | "find_code_duplicates"
        | "classify_symbol"
        | "diagnose_issues"
        | "audit_security_context"
        | "get_file_diagnostics"
        | "get_lsp_recipe"
        | "audit_builder_session"
        | "audit_planner_session" => Some(ToolPhase::Review),

        // Eval — telemetry, audit export, analysis artifact retrieval.
        "get_tool_metrics"
        | "export_session_markdown"
        | "start_analysis_job"
        | "get_analysis_job"
        | "cancel_analysis_job"
        | "get_analysis_section"
        | "list_analysis_jobs"
        | "list_analysis_artifacts" => Some(ToolPhase::Eval),

        // Infrastructure (filesystem, memory, session coordination) is
        // deliberately phase-agnostic: used in every phase.
        _ => None,
    }
}

pub(crate) fn tool_phase_label(name: &str) -> Option<&'static str> {
    tool_phase(name).map(|p| p.as_label())
}

/// ADR-0006 Layer 1 — routing hint advising which executor class is a
/// better fit for this tool. Advisory only: the host is free to ignore.
/// `Some("codex-builder")` — bulk implementation, pure relocation,
///   multi-file refactor. Cheap/fast executor wins here.
/// `Some("claude")` — orchestration, synthesis, design compression.
///   Reasoning budget is the bottleneck.
/// `None` — either executor is fine (retrieval primitives, reads,
///   audits, session coordination, eval, diagnostics).
pub(crate) fn tool_preferred_executor(name: &str) -> Option<&'static str> {
    match name {
        // Bulk implementation / mutation — Codex-class executor preferred.
        // (includes deprecated mutation tools still in dispatch for backward compat)
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
        | "cleanup_duplicate_logic"
        | "propagate_deletions" => Some("codex-builder"),

        // Orchestration / synthesis — Claude-class executor preferred.
        "plan_safe_refactor" | "review_architecture" | "trace_request_path" | "review_changes" => {
            Some("claude")
        }

        // Everything else — retrieval primitives, file ops, audits,
        // session coordination, eval, diagnostics — both executors
        // handle equally well. Keep None conservative until we have
        // measured divergence.
        _ => None,
    }
}

pub(crate) fn tool_preferred_executor_label(name: &str) -> &'static str {
    tool_preferred_executor(name).unwrap_or("any")
}

/// Claude Code snapshot compatibility:
/// these `_meta["anthropic/searchHint"]` phrases are consumed by the upstream
/// ToolSearch scorer for deferred MCP tools. Keep hints short and capability-
/// focused; omit low-signal tools instead of auto-generating noisy strings.
pub(crate) fn tool_anthropic_search_hint(name: &str) -> Option<&'static str> {
    match name {
        "prepare_harness_session" => Some("bootstrap CodeLens harness session"),
        "explore_codebase" => Some("explore codebase with compressed context"),
        "trace_request_path" => Some("trace a request path"),
        "review_changes" => Some("review changed files and risk"),
        "review_architecture" => Some("review architecture, boundaries, coupling"),
        "plan_safe_refactor" => Some("plan a safe refactor with preview"),
        "cleanup_duplicate_logic" => Some("surface duplicate code before cleanup"),
        "diagnose_issues" => Some("diagnose file diagnostics or unresolved refs"),
        "verify_change_readiness" => Some("verify edit safety before mutation"),
        "safe_rename_report" => Some("preview rename safety and blockers"),
        "refactor_safety_report" => Some("preview refactor safety and impact"),
        "start_analysis_job" => Some("run durable analysis in background"),
        "get_analysis_section" => Some("expand one analysis report section"),
        "audit_builder_session" => Some("audit builder session process"),
        "audit_planner_session" => Some("audit planner session process"),
        // Core navigation primitives (raised to deferred surface in v1.10.1)
        "find_symbol" => Some("find function class type by exact name"),
        "get_symbols_overview" => Some("list all symbols in a file"),
        "find_referencing_symbols" => Some("find all usages of a symbol"),
        "get_file_diagnostics" => Some("read LSP diagnostics for a file"),
        "bm25_symbol_search" => Some("BM25 sparse symbol search by token"),
        "search_symbols_fuzzy" => Some("fuzzy symbol search tolerates typos"),
        "semantic_search" => Some("natural language code search via embeddings"),
        "get_callers" => Some("find functions that call this function"),
        "get_callees" => Some("find functions called by this function"),
        "get_ranked_context" => Some("smart context retrieval within token budget"),
        "impact_report" => Some("blast-radius for changed files"),
        "module_boundary_report" => Some("module dependency and coupling report"),
        "dead_code_report" => Some("dead-code candidates with evidence"),
        "diff_aware_references" => Some("compress references for changed files"),
        _ => None,
    }
}

/// Claude Code MCP tools are deferred by default. The always-load set
/// is what gets `meta["anthropic/alwaysLoad"] = true` — schemas
/// pre-loaded so the model can call them without a `ToolSearch` round
/// trip. v1.10.1 widens this from 3 → 8 to cover the workflow-first
/// surface that the product is positioned around. The remaining
/// `DEFAULT_LISTED_TOOL_NAMES` entries stay deferred-discoverable but
/// require explicit ToolSearch select before invocation, which keeps
/// the initial tool prompt bounded.
pub(crate) fn tool_anthropic_always_load(name: &str) -> bool {
    matches!(
        name,
        "prepare_harness_session"
            | "explore_codebase"
            | "review_changes"
            | "plan_safe_refactor"
            | "review_architecture"
            | "verify_change_readiness"
            | "trace_request_path"
    )
}

pub(crate) fn tool_namespace(name: &str) -> &'static str {
    match name {
        "read_file" | "list_dir" | "find_file" | "find_annotations" | "find_tests" => "filesystem",
        "get_symbols_overview"
        | "find_symbol"
        | "get_ranked_context"
        | "search_symbols_fuzzy"
        | "bm25_symbol_search"
        | "find_referencing_symbols"
        | "search_workspace_symbols"
        | "get_type_hierarchy"
        | "plan_symbol_rename"
        | "semantic_search"
        | "index_embeddings" => "symbols",
        "get_changed_files"
        | "get_impact_analysis"
        | "get_callers"
        | "get_callees"
        | "find_scoped_references"
        | "get_symbol_importance"
        | "find_dead_code"
        | "find_circular_dependencies"
        | "find_similar_code"
        | "find_code_duplicates"
        | "classify_symbol"
        | "find_misplaced_code"
        | "get_complexity" => "graph",
        "cleanup_duplicate_logic"
        | "explore_codebase"
        | "trace_request_path"
        | "review_architecture"
        | "plan_safe_refactor"
        | "audit_security_context"
        | "analyze_change_impact"
        | "review_changes"
        | "assess_change_readiness"
        | "diagnose_issues"
        | "verify_change_readiness"
        | "module_boundary_report"
        | "mermaid_module_graph"
        | "safe_rename_report"
        | "unresolved_reference_check"
        | "dead_code_report"
        | "impact_report"
        | "refactor_safety_report"
        | "diff_aware_references"
        | "start_analysis_job"
        | "get_analysis_job"
        | "cancel_analysis_job"
        | "get_analysis_section" => "reports",
        "list_memories" | "read_memory" | "write_memory" | "delete_memory" | "rename_memory" => {
            "memory"
        }
        "get_file_diagnostics" | "get_lsp_recipe" => "lsp",
        _ => "session",
    }
}
