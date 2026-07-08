pub(crate) fn tool_deprecation(name: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match name {
        // Deprecated in v1.13.27, removal v2.0.
        // `audit_tool_surface_consistency` was on this list but was resurrected
        // in ae8c6f2f (P1-4 Sprint A) — removing it from the deprecation list
        // closes the daemon "deprecated" / CLI "Unknown tool" mismatch (#G7).
        // #346 re-classified the rest of the v1.13.27 wave:
        // - line-edit family → removed outright (TOMBSTONED_TOOLS guidance)
        // - symbolic edit core + refactor substrate → pending-D3 allowlist
        //   (dispatch-only, decision open — not "deprecated for removal")
        // - onboard_project / analyze_change_request / orchestrate_change →
        //   promoted to first-class tools.toml entries (must list).
        "find_circular_dependencies"
        | "find_redundant_definitions"
        | "find_orphan_handlers"
        | "find_over_visible_apis"
        | "find_phantom_modules"
        | "search_for_pattern"
        | "get_project_structure"
        | "analyze_missing_imports" => Some(("1.13.27", "", "2.0")),
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

pub(crate) fn tool_feature_gate(name: &str) -> Option<&'static str> {
    crate::tool_defs::generated::tool_feature_gate(name)
}

pub(crate) fn default_listed_tool_names() -> &'static [&'static str] {
    crate::tool_defs::generated::default_listed_tool_names()
}

/// Phase alias per ADR-0005 step 4 — harness-phase scoping for `tools/list`
/// filter without introducing a second tool registry. Returning `None` marks
/// the tool as phase-agnostic (infrastructure / coordination).
pub(crate) fn tool_phase(name: &str) -> Option<crate::protocol::ToolPhase> {
    use crate::protocol::ToolPhase;
    match crate::tool_defs::generated::tool_phase(name) {
        Some("plan") => Some(ToolPhase::Plan),
        Some("build") => Some(ToolPhase::Build),
        Some("review") => Some(ToolPhase::Review),
        Some("eval") => Some(ToolPhase::Eval),
        Some(_) | None => None,
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
        // (pending-D3 edit core + refactor substrate; line-edit family
        // tombstoned, #346)
        "rename_symbol"
        | "replace_symbol_body"
        | "insert_before_symbol"
        | "insert_after_symbol"
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
/// trip. Keep this as a Claude-native affordance list, separate from
/// the compact default `tools/list` slice used by generic hosts.
pub(crate) fn tool_anthropic_always_load(name: &str) -> bool {
    crate::tool_defs::generated::tool_default_listed(name)
        || matches!(
            name,
            "activate_project"
                | "set_preset"
                | "set_profile"
                | "trace_request_path"
                | "analyze_change_request"
                | "cleanup_duplicate_logic"
                | "diagnose_issues"
                | "get_symbols_overview"
                | "find_referencing_symbols"
                | "bm25_symbol_search"
                | "get_file_diagnostics"
                | "semantic_search"
                | "get_callers"
                | "get_callees"
                | "start_analysis_job"
                | "get_analysis_job"
                | "get_analysis_section"
        )
}

pub(crate) fn tool_namespace(name: &str) -> &'static str {
    crate::tool_defs::generated::tool_namespace(name).unwrap_or("session")
}
