/// Returns a map of tool name → brief reason explaining why it is suggested.
/// Called after `suggest_next_contextual` / doom-loop overrides have finalized the list.
pub fn suggestion_reasons_for(
    tools: &[String],
    _tool_name: &str,
) -> std::collections::HashMap<String, String> {
    let mut reasons = std::collections::HashMap::new();
    for tool in tools {
        let reason = match tool.as_str() {
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
