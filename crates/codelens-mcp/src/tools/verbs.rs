//! Verb facade tools — Phase-1 read-only consolidation (search / graph / review).
//!
//! Each verb is a thin mode-router over existing tools: it resolves
//! `mode` → target tool name, strips the routing key, and delegates
//! through the dispatch table. The facade is additive — every absorbed
//! tool ID stays registered and callable, so hooks, RBAC tiers, and
//! suggestion chains keyed by the original IDs remain valid. Feature
//! gates are inherited from the table: a mode whose target is not
//! registered in this build (semantic-off) fails with a rebuild hint
//! instead of a bare unknown-tool error.

use crate::AppState;
use crate::error::CodeLensError;
use crate::tool_runtime::ToolResult;
use serde_json::Value;

/// `search` — one entry point for "find code by name/reference/meaning".
pub(crate) const SEARCH_MODES: &[(&str, &str)] = &[
    ("symbol", "find_symbol"),
    ("refs", "find_referencing_symbols"),
    ("defn", "find_declaration"),
    ("impl", "find_implementations"),
    ("scoped", "find_scoped_references"),
    ("workspace", "search_workspace_symbols"),
    ("bm25", "bm25_symbol_search"),
    ("fuzzy", "search_symbols_fuzzy"),
    ("semantic", "semantic_search"),
    ("ranked", "get_ranked_context"),
];

/// `graph` — structural relationships: who calls / what breaks / how it flows.
pub(crate) const GRAPH_MODES: &[(&str, &str)] = &[
    ("callers", "get_callers"),
    ("callees", "get_callees"),
    ("types", "get_type_hierarchy"),
    ("trace", "trace_request_path"),
    ("impact", "impact_report"),
    ("diff-refs", "diff_aware_references"),
];

/// `review` — quality reports: architecture, boundaries, dead/duplicate code.
pub(crate) const REVIEW_MODES: &[(&str, &str)] = &[
    ("architecture", "review_architecture"),
    ("changes", "review_changes"),
    ("boundary", "module_boundary_report"),
    ("dead", "dead_code_report"),
    ("dupes", "find_code_duplicates"),
    ("similar", "find_similar_code"),
    ("misplaced", "find_misplaced_code"),
];

/// `overview` — structural maps: file symbols, guided exploration.
/// (No `project` mode: `get_project_structure` is a codelens-explorer
/// agent-surface name, not a tool registered in this server — the
/// drift-gate unit test rejects phantom targets.)
pub(crate) const OVERVIEW_MODES: &[(&str, &str)] = &[
    ("file", "get_symbols_overview"),
    ("explore", "explore_codebase"),
    ("classify", "classify_symbol"),
];

/// `diagnose` — health checks: LSP diagnostics, unresolved references.
pub(crate) const DIAGNOSE_MODES: &[(&str, &str)] = &[
    ("file", "get_file_diagnostics"),
    ("symbol", "get_diagnostics_for_symbol"),
    ("unresolved", "unresolved_reference_check"),
    ("issues", "diagnose_issues"),
];

/// `analyze` — durable analysis jobs: start, poll, expand, manage.
pub(crate) const ANALYZE_MODES: &[(&str, &str)] = &[
    ("start", "start_analysis_job"),
    ("status", "get_analysis_job"),
    ("section", "get_analysis_section"),
    ("list", "list_analysis_jobs"),
    ("cancel", "cancel_analysis_job"),
    ("artifacts", "list_analysis_artifacts"),
];

pub fn search(state: &AppState, args: &Value) -> ToolResult {
    run_verb("search", state, args)
}

pub fn overview(state: &AppState, args: &Value) -> ToolResult {
    run_verb("overview", state, args)
}

pub fn diagnose(state: &AppState, args: &Value) -> ToolResult {
    run_verb("diagnose", state, args)
}

pub fn analyze(state: &AppState, args: &Value) -> ToolResult {
    run_verb("analyze", state, args)
}

pub fn graph(state: &AppState, args: &Value) -> ToolResult {
    run_verb("graph", state, args)
}

pub fn review(state: &AppState, args: &Value) -> ToolResult {
    run_verb("review", state, args)
}

fn run_verb(verb: &'static str, state: &AppState, args: &Value) -> ToolResult {
    let (target, inner) = resolve_verb_target(verb, args)?.ok_or_else(|| {
        CodeLensError::ToolNotFound(format!("verb facade `{verb}` is not registered"))
    })?;
    match crate::dispatch::invoke_registered(state, target, &inner) {
        Some(result) => result,
        None => Err(CodeLensError::Validation(format!(
            "{verb} delegates to `{target}`, which is not registered in this \
             build (feature-gated — rebuild with `--features semantic`)"
        ))),
    }
}

/// Resolve a mode-routed facade to its target handler and target arguments.
///
/// The routing key is stripped after resolution so target handlers receive
/// only their own arguments. `None` marks ordinary tools that are not verb
/// facades.
pub(crate) fn resolve_verb_target(
    verb: &str,
    args: &Value,
) -> Result<Option<(&'static str, Value)>, CodeLensError> {
    let Some((target, _mode)) = resolve_verb_operation(verb, args)? else {
        return Ok(None);
    };
    let mut inner = args.clone();
    if let Some(obj) = inner.as_object_mut() {
        obj.remove("mode");
    }
    Ok(Some((target, inner)))
}

/// Resolve only the operation identity without cloning or rewriting arguments.
pub(crate) fn resolve_verb_operation<'a>(
    verb: &str,
    args: &'a Value,
) -> Result<Option<(&'static str, &'a str)>, CodeLensError> {
    let Some(modes) = modes_for_verb(verb) else {
        return Ok(None);
    };
    let mode = args
        .get("mode")
        .and_then(Value::as_str)
        .ok_or_else(|| CodeLensError::MissingParam("mode".to_owned()))?;
    let target = resolve_mode(verb, modes, mode)?;
    Ok(Some((target, mode)))
}

fn modes_for_verb(verb: &str) -> Option<&'static [(&'static str, &'static str)]> {
    match verb {
        "search" => Some(SEARCH_MODES),
        "overview" => Some(OVERVIEW_MODES),
        "diagnose" => Some(DIAGNOSE_MODES),
        "analyze" => Some(ANALYZE_MODES),
        "graph" => Some(GRAPH_MODES),
        "review" => Some(REVIEW_MODES),
        _ => None,
    }
}

pub(crate) fn is_verb_facade(name: &str) -> bool {
    modes_for_verb(name).is_some()
}

fn resolve_mode(
    verb: &str,
    modes: &'static [(&'static str, &'static str)],
    mode: &str,
) -> Result<&'static str, CodeLensError> {
    modes
        .iter()
        .find(|(name, _)| *name == mode)
        .map(|(_, target)| *target)
        .ok_or_else(|| {
            let valid = modes
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(", ");
            CodeLensError::Validation(format!(
                "unknown mode '{mode}' for `{verb}` — valid modes: [{valid}]"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Semantic-gated targets are absent from the dispatch table in
    /// `--no-default-features` builds; the router returns a rebuild
    /// hint for those instead of dispatching.
    const SEMANTIC_GATED: &[&str] = &[
        "semantic_search",
        "find_code_duplicates",
        "find_similar_code",
        "find_misplaced_code",
        "classify_symbol",
    ];

    #[test]
    fn every_declared_mode_resolves_to_a_registered_tool() {
        let registered = crate::dispatch::registered_tool_names();
        for (verb, modes) in [
            ("search", SEARCH_MODES),
            ("graph", GRAPH_MODES),
            ("review", REVIEW_MODES),
            ("overview", OVERVIEW_MODES),
            ("diagnose", DIAGNOSE_MODES),
            ("analyze", ANALYZE_MODES),
        ] {
            for (mode, target) in modes {
                if SEMANTIC_GATED.contains(target) {
                    #[cfg(feature = "semantic")]
                    assert!(
                        registered.contains(*target),
                        "{verb}:{mode} → {target} missing from semantic dispatch table"
                    );
                    continue;
                }
                assert!(
                    registered.contains(*target),
                    "{verb}:{mode} → {target} missing from dispatch table"
                );
            }
        }
    }

    #[test]
    fn resolve_mode_maps_known_mode_to_target() {
        assert_eq!(
            resolve_mode("search", SEARCH_MODES, "symbol").unwrap(),
            "find_symbol"
        );
        assert_eq!(
            resolve_mode("graph", GRAPH_MODES, "impact").unwrap(),
            "impact_report"
        );
        assert_eq!(
            resolve_mode("review", REVIEW_MODES, "dead").unwrap(),
            "dead_code_report"
        );
    }

    #[test]
    fn resolve_mode_unknown_lists_valid_modes() {
        let err = resolve_mode("search", SEARCH_MODES, "bogus").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("bogus"));
        assert!(msg.contains("symbol") && msg.contains("ranked"));
    }

    #[test]
    fn graph_facade_forwards_full_results_to_call_graph_targets() {
        // mode=callers|callees must carry `full_results` through to the target
        // tool unchanged (the routing key `mode` is the only stripped arg), so
        // the full-array preservation contract reaches get_callers/get_callees.
        for (mode, expected_target) in [("callers", "get_callers"), ("callees", "get_callees")] {
            let (target, inner) = resolve_verb_target(
                "graph",
                &serde_json::json!({
                    "mode": mode,
                    "function_name": "f",
                    "full_results": true,
                }),
            )
            .unwrap()
            .expect("graph is a verb facade");
            assert_eq!(target, expected_target);
            assert_eq!(
                inner.get("full_results"),
                Some(&Value::Bool(true)),
                "full_results must pass through to {expected_target}"
            );
            assert!(inner.get("mode").is_none(), "routing key stripped");
            assert_eq!(
                inner.get("function_name").and_then(Value::as_str),
                Some("f"),
                "other args survive the facade"
            );
        }
    }

    #[test]
    fn verb_mode_names_are_unique_per_verb() {
        for (verb, modes) in [
            ("search", SEARCH_MODES),
            ("graph", GRAPH_MODES),
            ("review", REVIEW_MODES),
            ("overview", OVERVIEW_MODES),
            ("diagnose", DIAGNOSE_MODES),
            ("analyze", ANALYZE_MODES),
        ] {
            let mut seen = std::collections::HashSet::new();
            for (mode, _) in modes {
                assert!(seen.insert(*mode), "{verb}: duplicate mode '{mode}'");
            }
        }
    }
}
