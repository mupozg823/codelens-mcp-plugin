use crate::tool_defs::{
    ToolProfile, ToolSurface, bootstrap_visible_tools, preferred_tier_labels, visible_tools,
};
use serde_json::{Value, json};

pub(crate) const PROFILE_GUIDE_PROFILES: [ToolProfile; 7] = [
    ToolProfile::PlannerReadonly,
    ToolProfile::BuilderMinimal,
    ToolProfile::ReviewerGraph,
    ToolProfile::EvaluatorCompact,
    ToolProfile::RefactorFull,
    ToolProfile::CiAudit,
    ToolProfile::WorkflowFirst,
];

fn profile_surface_metrics(profile: ToolProfile) -> Value {
    let surface = ToolSurface::Profile(profile);
    let total_tools = visible_tools(surface);
    let bootstrap_tools = bootstrap_visible_tools(surface);
    json!({
        "total_tool_count": total_tools.len(),
        "bootstrap_tool_count": bootstrap_tools.len(),
        "bootstrap_contract": "codex_deferred_default",
        "bootstrap_tools": bootstrap_tools.iter().map(|tool| tool.name).collect::<Vec<_>>(),
    })
}

pub(crate) fn profile_guide(profile: ToolProfile) -> Value {
    let surface_metrics = profile_surface_metrics(profile);
    match profile {
        ToolProfile::PlannerReadonly => json!({
            "profile": profile.as_str(),
            "intent": "Use bounded, read-only analysis to plan changes and rank context before implementation.",
            "preferred_tools": ["explore_codebase", "review_architecture", "analyze_change_impact", "plan_safe_refactor"],
            "preferred_namespaces": ["reports", "symbols", "graph", "filesystem", "session"],
            "surface_metrics": surface_metrics,
            "avoid": ["rename_symbol", "replace_content", "raw graph expansion unless necessary"]
        }),
        ToolProfile::BuilderMinimal => json!({
            "profile": profile.as_str(),
            "intent": "Keep the visible surface small while implementing changes with only the essential symbol and edit tools.",
            "preferred_tools": ["explore_codebase", "trace_request_path", "plan_safe_refactor", "analyze_change_impact"],
            "preferred_namespaces": ["reports", "symbols", "filesystem", "session"],
            "surface_metrics": surface_metrics,
            "avoid": ["dead-code audits", "full-graph exploration", "broad multi-project search"]
        }),
        ToolProfile::ReviewerGraph => json!({
            "profile": profile.as_str(),
            "intent": "Review risky changes with graph-aware, read-only evidence.",
            "preferred_tools": ["review_architecture", "analyze_change_impact", "audit_security_context", "cleanup_duplicate_logic"],
            "preferred_namespaces": ["reports", "graph", "symbols", "session"],
            "surface_metrics": surface_metrics,
            "avoid": ["mutation tools"]
        }),
        ToolProfile::RefactorFull => json!({
            "profile": profile.as_str(),
            "intent": "Run high-safety refactors only after a fresh preflight has narrowed the target surface and cleared blockers.",
            "preferred_tools": ["verify_change_readiness", "safe_rename_report", "refactor_safety_report"],
            "preferred_namespaces": ["reports", "session"],
            "surface_metrics": surface_metrics,
            "avoid": ["mutation before preflight", "broad edits without diagnostics or preview"]
        }),
        ToolProfile::CiAudit => json!({
            "profile": profile.as_str(),
            "intent": "Produce machine-friendly review output around diffs, impact, dead code, and structural risk.",
            "preferred_tools": ["analyze_change_impact", "audit_security_context", "review_architecture", "cleanup_duplicate_logic"],
            "preferred_namespaces": ["reports", "graph", "session"],
            "surface_metrics": surface_metrics,
            "avoid": ["interactive mutation flows"]
        }),
        ToolProfile::EvaluatorCompact => json!({
            "profile": profile.as_str(),
            "intent": "Minimal read-only profile for scoring harnesses — diagnostics, test discovery, and symbol lookup only.",
            "preferred_tools": ["verify_change_readiness", "get_file_diagnostics", "find_tests", "find_symbol"],
            "preferred_namespaces": ["reports", "symbols", "lsp", "session"],
            "surface_metrics": surface_metrics,
            "avoid": ["mutation tools", "graph expansion", "broad analysis reports"]
        }),
        ToolProfile::WorkflowFirst => json!({
            "profile": profile.as_str(),
            "intent": "Keep Codex on problem-first workflow entrypoints by default and defer low-level lookup until it is clearly needed.",
            "preferred_tools": ["explore_codebase", "trace_request_path", "review_architecture", "analyze_change_impact", "plan_safe_refactor", "verify_change_readiness"],
            "preferred_namespaces": ["reports", "session"],
            "description": "Problem-first workflow surface. High-level workflow tools stay visible; low-level tools are deferred until follow-up or surface promotion.",
            "surface_size": "workflow",
            "mutation": false,
            "surface_metrics": surface_metrics,
            "preferred_tiers": preferred_tier_labels(ToolSurface::Profile(profile)),
        }),
    }
}

pub(crate) fn profile_guide_summary(profile: ToolProfile) -> Value {
    let guide = profile_guide(profile);
    json!({
        "profile": guide.get("profile").cloned().unwrap_or(json!(profile.as_str())),
        "intent": guide.get("intent").cloned().unwrap_or(json!("")),
        "preferred_tools": guide.get("preferred_tools").cloned().unwrap_or(json!([])),
        "preferred_namespaces": guide.get("preferred_namespaces").cloned().unwrap_or(json!([])),
        "preferred_tiers": preferred_tier_labels(ToolSurface::Profile(profile)),
        "surface_metrics": guide.get("surface_metrics").cloned().unwrap_or_else(|| profile_surface_metrics(profile)),
    })
}

pub(crate) fn profile_resource_entries() -> Vec<Value> {
    PROFILE_GUIDE_PROFILES
        .iter()
        .flat_map(|profile| {
            [
                json!({
                    "uri": format!("codelens://profile/{}/guide", profile.as_str()),
                    "name": format!("Profile Guide: {}", profile.as_str()),
                    "description": "Compressed role profile guide",
                    "mimeType": "application/json"
                }),
                json!({
                    "uri": format!("codelens://profile/{}/guide/full", profile.as_str()),
                    "name": format!("Profile Guide (Full): {}", profile.as_str()),
                    "description": "Expanded role profile guide with anti-patterns",
                    "mimeType": "application/json"
                }),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{profile_guide_summary, profile_surface_metrics};
    use crate::tool_defs::ToolProfile;

    fn assert_bounded_surface_metrics(
        metrics: &serde_json::Value,
        expected_bootstrap: u64,
        min_total: u64,
        max_total: u64,
    ) {
        let bootstrap = metrics["bootstrap_tool_count"]
            .as_u64()
            .expect("bootstrap_tool_count should be numeric");
        let total = metrics["total_tool_count"]
            .as_u64()
            .expect("total_tool_count should be numeric");

        assert_eq!(bootstrap, expected_bootstrap);
        assert!(
            total >= min_total,
            "expected total >= {min_total}, got {total}"
        );
        assert!(
            total <= max_total,
            "expected total <= {max_total}, got {total}"
        );
        assert!(total >= bootstrap, "expected total >= bootstrap");
    }

    #[test]
    fn codex_bootstrap_counts_stay_bounded_for_core_profiles() {
        let planner = profile_surface_metrics(ToolProfile::PlannerReadonly);
        assert_bounded_surface_metrics(&planner, 9, 27, 31);

        let builder = profile_surface_metrics(ToolProfile::BuilderMinimal);
        assert_bounded_surface_metrics(&builder, 9, 26, 30);

        let reviewer = profile_surface_metrics(ToolProfile::ReviewerGraph);
        assert_bounded_surface_metrics(&reviewer, 8, 27, 31);

        let refactor = profile_surface_metrics(ToolProfile::RefactorFull);
        assert_bounded_surface_metrics(&refactor, 6, 40, 46);
    }

    #[test]
    fn profile_guide_summary_includes_surface_metrics() {
        let summary = profile_guide_summary(ToolProfile::PlannerReadonly);
        assert_eq!(
            summary["surface_metrics"]["bootstrap_contract"],
            "codex_deferred_default"
        );
        assert_eq!(summary["surface_metrics"]["bootstrap_tool_count"], 9);
        assert!(summary["surface_metrics"]["bootstrap_tools"].is_array());
    }
}
