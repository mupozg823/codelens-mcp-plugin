use crate::tool_defs::{ToolProfile, ToolSurface, preferred_tier_labels};
use serde_json::{Value, json};

pub(crate) const PROFILE_GUIDE_PROFILES: [ToolProfile; 6] = [
    ToolProfile::PlannerReadonly,
    ToolProfile::BuilderMinimal,
    ToolProfile::ReviewerGraph,
    ToolProfile::EvaluatorCompact,
    ToolProfile::RefactorFull,
    ToolProfile::CiAudit,
];

pub(crate) fn profile_guide(profile: ToolProfile) -> Value {
    match profile {
        ToolProfile::PlannerReadonly => json!({
            "profile": profile.as_str(),
            "intent": "Use bounded, read-only analysis to plan changes and rank context before implementation.",
            "preferred_tools": ["verify_change_readiness", "analyze_change_request", "find_minimal_context_for_change", "impact_report"],
            "preferred_namespaces": ["reports", "symbols", "graph", "filesystem", "session"],
            "avoid": ["rename_symbol", "replace_content", "raw graph expansion unless necessary"]
        }),
        ToolProfile::BuilderMinimal => json!({
            "profile": profile.as_str(),
            "intent": "Keep the visible surface small while implementing changes with only the essential symbol and edit tools.",
            "preferred_tools": ["verify_change_readiness", "find_symbol", "get_symbols_overview", "add_import"],
            "preferred_namespaces": ["symbols", "filesystem", "session"],
            "avoid": ["dead-code audits", "full-graph exploration", "broad multi-project search"]
        }),
        ToolProfile::ReviewerGraph => json!({
            "profile": profile.as_str(),
            "intent": "Review risky changes with graph-aware, read-only evidence.",
            "preferred_tools": ["verify_change_readiness", "impact_report", "diff_aware_references", "dead_code_report"],
            "preferred_namespaces": ["reports", "graph", "symbols", "session"],
            "avoid": ["mutation tools"]
        }),
        ToolProfile::RefactorFull => json!({
            "profile": profile.as_str(),
            "intent": "Run high-safety refactors only after a fresh preflight has narrowed the target surface and cleared blockers.",
            "preferred_tools": ["verify_change_readiness", "safe_rename_report", "unresolved_reference_check", "rename_symbol"],
            "preferred_namespaces": ["reports", "mutation", "symbols", "session"],
            "avoid": ["mutation before preflight", "broad edits without diagnostics or preview"]
        }),
        ToolProfile::CiAudit => json!({
            "profile": profile.as_str(),
            "intent": "Produce machine-friendly review output around diffs, impact, dead code, and structural risk.",
            "preferred_tools": ["verify_change_readiness", "impact_report", "diff_aware_references", "dead_code_report"],
            "preferred_namespaces": ["reports", "graph", "session"],
            "avoid": ["interactive mutation flows"]
        }),
        ToolProfile::EvaluatorCompact => json!({
            "profile": profile.as_str(),
            "intent": "Minimal read-only profile for scoring harnesses — diagnostics, test discovery, and symbol lookup only.",
            "preferred_tools": ["verify_change_readiness", "get_file_diagnostics", "find_tests", "find_symbol"],
            "preferred_namespaces": ["reports", "symbols", "lsp", "session"],
            "avoid": ["mutation tools", "graph expansion", "broad analysis reports"]
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
