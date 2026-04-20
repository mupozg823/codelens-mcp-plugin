//! Tool call envelope — normalized JSON-RPC params with profile/compact/harness routing.

use crate::tool_defs::{default_budget_for_profile, ToolProfile};
use crate::AppState;
use serde_json::json;

/// Normalized tool call request — extracted from raw JSON-RPC params.
pub(crate) struct ToolCallEnvelope {
    pub name: String,
    pub arguments: serde_json::Value,
    pub session: crate::session_context::SessionRequestContext,
    pub budget: usize,
    pub compact: bool,
    pub harness_phase: Option<String>,
}

impl ToolCallEnvelope {
    /// Parse raw JSON-RPC params into a normalized envelope.
    pub fn parse(
        params: &serde_json::Value,
        state: &AppState,
    ) -> Result<Self, (&'static str, i64)> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or(("Missing tool name", -32602i64))?
            .to_owned();
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let session = crate::session_context::SessionRequestContext::from_json(&arguments);
        let default_budget = state.execution_token_budget(&session);
        let budget = arguments
            .get("_profile")
            .and_then(|v| v.as_str())
            .map(|profile| {
                ToolProfile::from_str(profile)
                    .map(default_budget_for_profile)
                    .unwrap_or_else(|| match profile {
                        "fast_local" => 2000usize,
                        "deep_semantic" => 16000,
                        "safe_mutation" => 4000,
                        _ => default_budget,
                    })
            })
            .unwrap_or(default_budget);
        // Phase 5: "lean by default for lookups". Low-level lookup
        // tools (find_symbol, references, overview, ranked_context,
        // fuzzy/BM25 search) default to compact=true so the common
        // case — a harness pulling one symbol or one callsite — pays
        // zero tokens for Codex delegation scaffolds or rationales it
        // will discard. Workflow tools (impact_report, review_*,
        // explore_codebase, analyze_change_request, …) keep the
        // verbose envelope by default because the scaffold is often
        // the actionable output. Callers always win via an explicit
        // `_compact` argument.
        let default_compact = is_lean_default_tool(&name);
        let compact = arguments
            .get("_compact")
            .and_then(|v| v.as_bool())
            .unwrap_or(default_compact);
        let harness_phase = arguments
            .get("_harness_phase")
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned());
        Ok(Self {
            name,
            arguments,
            session,
            budget,
            compact,
            harness_phase,
        })
    }
}

/// Return `true` for tools that should default to the compact
/// response shape (Phase 5). These are the high-traffic "lookup"
/// tools where the Codex delegation scaffold and per-entry rationale
/// text are pure overhead for a harness already following a chain.
/// Any tool not listed keeps the verbose envelope (workflow tools,
/// analysis reports, mutation tools) because the scaffold is often
/// the actionable output.
fn is_lean_default_tool(name: &str) -> bool {
    matches!(
        name,
        "find_symbol"
            | "find_referencing_symbols"
            | "find_scoped_references"
            | "diff_aware_references"
            | "get_symbols_overview"
            | "get_ranked_context"
            | "bm25_symbol_search"
            | "search_symbols_fuzzy"
            | "search_workspace_symbols"
            | "get_file_diagnostics"
            | "get_type_hierarchy"
            | "extract_word_at_position"
            | "read_file"
            | "list_dir"
            | "get_symbol_importance"
            | "get_ranked_symbols"
    )
}

#[cfg(test)]
mod tests {
    use super::is_lean_default_tool;

    #[test]
    fn lookup_tools_default_to_compact() {
        assert!(is_lean_default_tool("find_symbol"));
        assert!(is_lean_default_tool("find_referencing_symbols"));
        assert!(is_lean_default_tool("get_ranked_context"));
    }

    #[test]
    fn workflow_tools_keep_verbose_envelope() {
        assert!(!is_lean_default_tool("impact_report"));
        assert!(!is_lean_default_tool("review_changes"));
        assert!(!is_lean_default_tool("explore_codebase"));
        assert!(!is_lean_default_tool("analyze_change_request"));
        assert!(!is_lean_default_tool("summarize_symbol_impact"));
    }

    #[test]
    fn unknown_tool_stays_verbose_by_default() {
        // Additive safety: any tool not in the explicit lean list
        // keeps the existing behavior so surface additions never
        // silently lose their orchestration scaffold.
        assert!(!is_lean_default_tool("some_future_analysis_tool"));
    }
}
