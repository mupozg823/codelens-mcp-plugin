use crate::tool_defs::{ToolProfile, ToolSurface};

fn is_workflow_tool_name(name: &str) -> bool {
    // `analyze_change_impact` is the removed v2.0 alias retained for
    // historical-name matching (an agent still emitting the legacy name is
    // classified as a workflow tool, matching the `suggest_next` key carve-out).
    // The other removed aliases (`audit_security_context`,
    // `assess_change_readiness`) were dropped — they are neither live tools nor
    // intentional aliases, so classifying them was inert.
    crate::tools::verbs::is_verb_facade(name)
        || matches!(
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
