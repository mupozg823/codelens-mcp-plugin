use crate::tool_defs::{ToolProfile, ToolSurface, preferred_tier_labels};
use serde_json::{Value, json};

const PROFILE_GUIDE_PROFILES: [ToolProfile; 7] = [
    ToolProfile::PlannerReadonly,
    ToolProfile::BuilderMinimal,
    ToolProfile::ReviewerGraph,
    ToolProfile::EvaluatorCompact,
    ToolProfile::RefactorFull,
    ToolProfile::CiAudit,
    ToolProfile::WorkflowFirst,
];

pub(crate) fn profile_guide(profile: ToolProfile) -> Value {
    // Deprecated profiles delegate to their canonical core equivalent.
    let display_profile = profile;
    let guide_profile = profile.canonical();
    match guide_profile {
        ToolProfile::PlannerReadonly => json!({
            "profile": display_profile.as_str(),
            "canonical": guide_profile.as_str(),
            "deprecated": display_profile.is_deprecated(),
            "intent": "Use bounded, read-only analysis to plan changes and rank context before implementation.",
            "preferred_tools": ["explore_codebase", "review_architecture", "review_changes", "plan_safe_refactor"],
            "preferred_namespaces": ["reports", "symbols", "graph", "filesystem", "session"],
            "avoid": ["rename_symbol", "replace_content", "raw graph expansion unless necessary"]
        }),
        ToolProfile::BuilderMinimal => json!({
            "profile": display_profile.as_str(),
            "canonical": guide_profile.as_str(),
            "deprecated": display_profile.is_deprecated(),
            "intent": "Keep the visible surface small while implementing changes with only the essential symbol and edit tools.",
            "preferred_tools": ["explore_codebase", "trace_request_path", "plan_safe_refactor", "review_changes"],
            "preferred_namespaces": ["reports", "symbols", "filesystem", "session"],
            "avoid": ["dead-code audits", "full-graph exploration", "broad multi-project search"]
        }),
        ToolProfile::ReviewerGraph => json!({
            "profile": display_profile.as_str(),
            "canonical": guide_profile.as_str(),
            "deprecated": display_profile.is_deprecated(),
            "intent": "Review risky changes with graph-aware, read-only evidence.",
            "preferred_tools": ["review_architecture", "review_changes", "cleanup_duplicate_logic", "diagnose_issues"],
            "preferred_namespaces": ["reports", "graph", "symbols", "session"],
            "avoid": ["mutation tools"]
        }),
        dep => unreachable!("canonical() should not return {dep:?}"),
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
