use crate::tool_defs::{preferred_tier_labels, ToolProfile, ToolSurface};
use serde_json::{json, Value};

pub(crate) const PROFILE_GUIDE_PROFILES: [ToolProfile; 7] = [
    ToolProfile::PlannerReadonly,
    ToolProfile::BuilderMinimal,
    ToolProfile::ReviewerGraph,
    ToolProfile::EvaluatorCompact,
    ToolProfile::RefactorFull,
    ToolProfile::CiAudit,
    ToolProfile::WorkflowFirst,
];

pub(crate) fn profile_guide(profile: ToolProfile) -> Value {
    match profile {
        ToolProfile::PlannerReadonly => json!({
            "profile": profile.as_str(),
            "intent": "Use bounded, read-only analysis to plan changes and rank context before implementation.",
            "preferred_tools": ["explore_codebase", "review_architecture", "analyze_change_impact", "plan_safe_refactor"],
            "preferred_namespaces": ["reports", "symbols", "graph", "filesystem", "session"],
            "avoid": ["rename_symbol", "replace_content", "raw graph expansion unless necessary"]
        }),
        ToolProfile::BuilderMinimal => json!({
            "profile": profile.as_str(),
            "intent": "Keep the visible surface small while implementing changes with only the essential symbol and edit tools.",
            "preferred_tools": ["explore_codebase", "trace_request_path", "plan_safe_refactor", "analyze_change_impact"],
            "preferred_namespaces": ["reports", "symbols", "filesystem", "session"],
            "avoid": ["dead-code audits", "full-graph exploration", "broad multi-project search"]
        }),
        ToolProfile::ReviewerGraph => json!({
            "profile": profile.as_str(),
            "intent": "Review risky changes with graph-aware, read-only evidence.",
            "preferred_tools": ["review_architecture", "analyze_change_impact", "audit_security_context", "cleanup_duplicate_logic"],
            "preferred_namespaces": ["reports", "graph", "symbols", "session"],
            "avoid": ["mutation tools"]
        }),
        ToolProfile::RefactorFull => json!({
            "profile": profile.as_str(),
            "intent": "Run high-safety refactors only after a fresh preflight has narrowed the target surface and cleared blockers.",
            "preferred_tools": ["plan_safe_refactor", "analyze_change_impact", "trace_request_path", "review_architecture"],
            "preferred_namespaces": ["reports", "session"],
            "avoid": ["mutation before preflight", "broad edits without diagnostics or preview"]
        }),
        ToolProfile::CiAudit => json!({
            "profile": profile.as_str(),
            "intent": "Produce machine-friendly review output around diffs, impact, dead code, and structural risk.",
            "preferred_tools": ["analyze_change_impact", "audit_security_context", "review_architecture", "cleanup_duplicate_logic"],
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
        ToolProfile::WorkflowFirst => json!({
            "profile": profile.as_str(),
            "description": "Problem-first workflow surface. Agents see 12 high-level workflow tools; low-level tools are deferred.",
            "surface_size": "workflow",
            "mutation": false,
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
