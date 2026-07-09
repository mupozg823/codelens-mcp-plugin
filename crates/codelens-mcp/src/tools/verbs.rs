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

pub fn search(state: &AppState, args: &Value) -> ToolResult {
    run_verb("search", SEARCH_MODES, state, args)
}

pub fn graph(state: &AppState, args: &Value) -> ToolResult {
    run_verb("graph", GRAPH_MODES, state, args)
}

pub fn review(state: &AppState, args: &Value) -> ToolResult {
    run_verb("review", REVIEW_MODES, state, args)
}

fn run_verb(
    verb: &'static str,
    modes: &'static [(&'static str, &'static str)],
    state: &AppState,
    args: &Value,
) -> ToolResult {
    let mode = args
        .get("mode")
        .and_then(Value::as_str)
        .ok_or_else(|| CodeLensError::MissingParam("mode".to_owned()))?;
    let target = resolve_mode(verb, modes, mode)?;
    // Pass arguments through unchanged (target handlers read their own
    // keys and ignore extras); only the routing key is stripped.
    let mut inner = args.clone();
    if let Some(obj) = inner.as_object_mut() {
        obj.remove("mode");
    }
    match crate::dispatch::invoke_registered(state, target, &inner) {
        Some(result) => result,
        None => Err(CodeLensError::Validation(format!(
            "{verb} mode '{mode}' delegates to `{target}`, which is not registered in this \
             build (feature-gated — rebuild with `--features semantic`)"
        ))),
    }
}

fn resolve_mode(
    verb: &'static str,
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
    ];

    #[test]
    fn every_declared_mode_resolves_to_a_registered_tool() {
        let registered = crate::dispatch::registered_tool_names();
        for (verb, modes) in [
            ("search", SEARCH_MODES),
            ("graph", GRAPH_MODES),
            ("review", REVIEW_MODES),
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
    fn verb_mode_names_are_unique_per_verb() {
        for (verb, modes) in [
            ("search", SEARCH_MODES),
            ("graph", GRAPH_MODES),
            ("review", REVIEW_MODES),
        ] {
            let mut seen = std::collections::HashSet::new();
            for (mode, _) in modes {
                assert!(seen.insert(*mode), "{verb}: duplicate mode '{mode}'");
            }
        }
    }
}
