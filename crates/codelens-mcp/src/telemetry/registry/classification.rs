use crate::telemetry::ToolInvocation;

pub(super) fn is_workflow_tool(name: &str) -> bool {
    crate::tools::verbs::is_verb_facade(name)
        || matches!(
            name,
            "tools/list"
                | "explore_codebase"
                | "trace_request_path"
                | "review_architecture"
                | "plan_safe_refactor"
                | "audit_security_context"
                | "analyze_change_impact"
                | "cleanup_duplicate_logic"
                | "review_changes"
                | "assess_change_readiness"
                | "diagnose_issues"
                | "onboard_project"
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
                | "semantic_code_review"
                | "start_analysis_job"
                | "get_analysis_job"
                | "cancel_analysis_job"
        )
}

fn is_low_level_tool(name: &str) -> bool {
    !is_workflow_tool(name)
}

pub(super) fn has_low_level_chain(timeline: &[ToolInvocation]) -> bool {
    if timeline.len() < 3 {
        return false;
    }
    let recent = &timeline[timeline.len() - 3..];
    recent.iter().all(|entry| is_low_level_tool(&entry.tool))
}
