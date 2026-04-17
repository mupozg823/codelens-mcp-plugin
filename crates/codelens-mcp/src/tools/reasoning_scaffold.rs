//! Reasoning scaffold — structured cognitive hints attached to workflow responses.
//!
//! A scaffold names what the current response *is evidence for*, what it
//! *is not*, and where to go next when the user's question does not fall
//! inside the current tool's scope. It is a token-cheap way to prevent
//! agents from mis-reading workflow output (e.g. treating an architecture
//! review as a correctness audit).
//!
//! Only a small set of planner/reviewer workflow tools carry scaffolds.
//! Tools without an entry return `None`, and the field is omitted from the
//! response payload so silent tools stay token-lean.

use serde_json::{json, Value};

/// Return the structured reasoning scaffold for a workflow tool, if any.
///
/// The returned object has three conventional keys:
///
/// - `what_this_tells_you` — the evidence this response actually supports.
/// - `what_this_does_not_tell_you` — adjacent questions this response
///   cannot answer.
/// - `if_looking_for_*` entries — named escape hatches that point the
///   agent to a better tool for a different intent.
pub(crate) fn reasoning_scaffold_for(tool_name: &str) -> Option<Value> {
    match tool_name {
        "review_architecture" | "module_boundary_report" => Some(json!({
            "what_this_tells_you": "Inbound/outbound coupling, cycle risk, and structural importers for the scoped path.",
            "what_this_does_not_tell_you": "Runtime correctness, test coverage gaps, or performance regressions.",
            "if_looking_for_change_safety": "run verify_change_readiness next",
            "if_looking_for_blast_radius": "run impact_report next",
            "if_looking_for_dead_branches": "run dead_code_report next"
        })),
        "impact_report" | "analyze_change_impact" => Some(json!({
            "what_this_tells_you": "Downstream files and symbols reachable from the changed set, ranked by reference density.",
            "what_this_does_not_tell_you": "Whether the change is semantically correct or whether tests exercise the reachable paths.",
            "if_looking_for_classified_references": "run diff_aware_references next",
            "if_looking_for_mutation_safety": "run verify_change_readiness next"
        })),
        "review_changes" => Some(json!({
            "what_this_tells_you": "Classified references in the changed files and readiness signals for merge.",
            "what_this_does_not_tell_you": "Semantic regressions, security posture, or UX impact.",
            "if_looking_for_impact_beyond_changed_files": "run impact_report next",
            "if_looking_for_diagnostics": "run get_file_diagnostics next"
        })),
        "dead_code_report" => Some(json!({
            "what_this_tells_you": "Bounded candidates with reference-count evidence. Ordered by deletion risk.",
            "what_this_does_not_tell_you": "Runtime entry points hit via reflection, codegen, or external consumers.",
            "if_looking_for_misplaced_rather_than_dead": "run find_misplaced_code next",
            "if_looking_for_dup_extraction": "run find_code_duplicates next"
        })),
        "analyze_change_request" => Some(json!({
            "what_this_tells_you": "Ranked files and risk estimate for the task, plus a suggested verifier chain.",
            "what_this_does_not_tell_you": "Whether the request itself is well-scoped or aligned with product goals.",
            "if_looking_for_mutation_gate": "run verify_change_readiness next",
            "if_looking_for_minimal_context": "run find_minimal_context_for_change next"
        })),
        "explore_codebase" | "onboard_project" => Some(json!({
            "what_this_tells_you": "Directory layout, top importance files, and cycle health for first-look orientation.",
            "what_this_does_not_tell_you": "Any single workflow's correctness — this is a map, not a verdict.",
            "if_looking_for_specific_function": "run find_symbol next",
            "if_looking_for_architecture_review": "run review_architecture next"
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::reasoning_scaffold_for;

    #[test]
    fn known_workflow_tools_have_scaffold() {
        for name in [
            "review_architecture",
            "impact_report",
            "review_changes",
            "dead_code_report",
            "analyze_change_request",
            "explore_codebase",
        ] {
            let scaffold = reasoning_scaffold_for(name).unwrap_or_else(|| panic!("{name} missing"));
            assert!(scaffold.get("what_this_tells_you").is_some());
            assert!(scaffold.get("what_this_does_not_tell_you").is_some());
        }
    }

    #[test]
    fn unknown_tool_returns_none() {
        assert!(reasoning_scaffold_for("get_symbols_overview").is_none());
        assert!(reasoning_scaffold_for("find_symbol").is_none());
    }

    #[test]
    fn scaffold_has_at_least_one_escape_hatch() {
        let scaffold = reasoning_scaffold_for("review_architecture").unwrap();
        let obj = scaffold.as_object().unwrap();
        let has_escape = obj.keys().any(|k| k.starts_with("if_looking_for_"));
        assert!(
            has_escape,
            "scaffold must name at least one alternative tool"
        );
    }
}
