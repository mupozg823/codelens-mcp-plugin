//! Tool call envelope — normalized JSON-RPC params with profile/compact/harness routing.

use crate::client_profile::ClientProfile;
use crate::tool_defs::{default_budget_for_profile, ToolProfile};
use crate::AppState;
use serde_json::json;

/// Phase P1 response shape tiering.
///
/// Every tool response emitted by the server picks one of these three
/// shapes before serialization:
///
/// * [`DetailLevel::Primitive`] — Serena-class bytes (~1.5× Serena).
///   Drops `_meta`, `structuredContent`, empty decisions/limits, and
///   emits the text block in compact JSON (no indentation). Intended
///   for the high-frequency lookup path (`find_symbol`,
///   `find_referencing_symbols`, `get_symbols_overview`,
///   `get_ranked_context`, …) when called by a harness that reads the
///   text body directly — e.g. Claude Code. Default for those tools
///   when the client profile is [`ClientProfile::Claude`].
/// * [`DetailLevel::Core`] — Phase 5 "lean" envelope. Drops the heavy
///   Codex delegation scaffold and per-suggestion rationales but
///   keeps `_meta`, `structuredContent`, and `suggested_next_tools`
///   (just tool names, no scaffolds). Default for lookup tools when
///   the client is not Claude Code.
/// * [`DetailLevel::Rich`] — full envelope with orchestration
///   scaffolds, `suggested_next_calls` delegate briefs, and
///   rationales. Default for workflow tools (`impact_report`,
///   `review_changes`, `explore_codebase`, `analyze_change_request`,
///   …) where the scaffold is often the actionable output.
///
/// Callers always win via `_detail="primitive"|"core"|"rich"` in
/// `arguments`. The legacy `_compact=true|false` argument still works
/// as a shim (see [`DetailLevel::from_args`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailLevel {
    Primitive,
    Core,
    Rich,
}

impl DetailLevel {
    /// Parse the `_detail` / `_compact` arguments into an explicit
    /// override, falling back to `None` when the caller was silent so
    /// the dispatcher can apply the tool+client default.
    pub(crate) fn from_args(arguments: &serde_json::Value) -> Option<Self> {
        if let Some(label) = arguments.get("_detail").and_then(|v| v.as_str()) {
            return match label {
                "primitive" | "Primitive" => Some(Self::Primitive),
                "core" | "Core" | "lean" | "compact" => Some(Self::Core),
                "rich" | "Rich" | "full" | "verbose" => Some(Self::Rich),
                _ => None,
            };
        }
        arguments
            .get("_compact")
            .and_then(|v| v.as_bool())
            .map(|compact| if compact { Self::Core } else { Self::Rich })
    }

    pub(crate) fn is_compact(self) -> bool {
        matches!(self, Self::Primitive | Self::Core)
    }

    pub(crate) fn is_primitive(self) -> bool {
        matches!(self, Self::Primitive)
    }
}

/// Normalized tool call request — extracted from raw JSON-RPC params.
pub(crate) struct ToolCallEnvelope {
    pub name: String,
    pub arguments: serde_json::Value,
    pub session: crate::session_context::SessionRequestContext,
    pub budget: usize,
    /// Phase 5 back-compat shim: `compact` is `true` iff the resolved
    /// [`DetailLevel`] is `Primitive` or `Core`. New code should read
    /// `detail` directly.
    pub compact: bool,
    pub detail: DetailLevel,
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
        // Phase P1 response-shape tiering. Resolve the caller's
        // explicit override first, then fall back to the tool+client
        // default derived by [`default_detail_level`]. The legacy
        // `compact` bool is kept in the envelope for back-compat with
        // pre-P1 call sites — new code should prefer `detail`.
        // Phase P1: only trust the caller's `clientInfo.name` when it
        // was propagated into the session. Falling through to
        // [`ClientProfile::detect(None)`] picks up `CLAUDE_PROJECT_DIR`
        // /`CODEX_SANDBOX_DIR` env hints that are useful for
        // interactive sessions but catastrophic for unit tests — the
        // surrounding shell may set those variables and flip every
        // dispatch to primitive mode silently. Keep env hints for
        // the ergonomic case (no explicit clientInfo) only when we
        // *also* saw an explicit client name; otherwise fall back to
        // the safest Generic/Core default.
        let client = match session.client_name.as_deref() {
            Some(name) => ClientProfile::detect(Some(name)),
            None => ClientProfile::Generic,
        };
        let detail = DetailLevel::from_args(&arguments)
            .unwrap_or_else(|| default_detail_level(&name, client));
        let compact = detail.is_compact();
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
            detail,
            harness_phase,
        })
    }
}

/// Phase P1 default shape resolver.
///
/// Lookup tools served to a Claude Code session default to
/// [`DetailLevel::Primitive`] — the Serena-class envelope. Lookup
/// tools served to any other client keep the Phase 5 "lean"
/// ([`DetailLevel::Core`]) envelope because those clients may still
/// consume the `_meta`/`structuredContent` extras for typed access.
/// Non-lookup (workflow/mutation/analysis) tools always start at
/// [`DetailLevel::Rich`] because the orchestration scaffold is often
/// the actionable output.
pub(crate) fn default_detail_level(name: &str, client: ClientProfile) -> DetailLevel {
    if !is_lean_default_tool(name) {
        return DetailLevel::Rich;
    }
    match client {
        ClientProfile::Claude => DetailLevel::Primitive,
        _ => DetailLevel::Core,
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
    use super::{default_detail_level, is_lean_default_tool, DetailLevel};
    use crate::client_profile::ClientProfile;
    use serde_json::json;

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

    #[test]
    fn lookup_tools_default_to_primitive_for_claude_code() {
        // Phase P1 contract: Claude Code sessions get the
        // Serena-class envelope on every high-traffic lookup call.
        assert_eq!(
            default_detail_level("find_symbol", ClientProfile::Claude),
            DetailLevel::Primitive
        );
        assert_eq!(
            default_detail_level("find_referencing_symbols", ClientProfile::Claude),
            DetailLevel::Primitive
        );
        assert_eq!(
            default_detail_level("get_ranked_context", ClientProfile::Claude),
            DetailLevel::Primitive
        );
    }

    #[test]
    fn lookup_tools_stay_core_for_non_claude_clients() {
        // Codex and other clients still get the Phase 5 "core"
        // envelope — they may still want `_meta`/`structuredContent`
        // for typed access.
        assert_eq!(
            default_detail_level("find_symbol", ClientProfile::Codex),
            DetailLevel::Core
        );
        assert_eq!(
            default_detail_level("find_symbol", ClientProfile::Generic),
            DetailLevel::Core
        );
    }

    #[test]
    fn workflow_tools_keep_rich_detail_regardless_of_client() {
        // Workflow tools always default to Rich — their orchestration
        // scaffold is often the actionable output and the response
        // already fits the workflow latency budget.
        assert_eq!(
            default_detail_level("impact_report", ClientProfile::Claude),
            DetailLevel::Rich
        );
        assert_eq!(
            default_detail_level("review_changes", ClientProfile::Claude),
            DetailLevel::Rich
        );
        assert_eq!(
            default_detail_level("analyze_change_request", ClientProfile::Codex),
            DetailLevel::Rich
        );
    }

    #[test]
    fn detail_from_args_explicit_override_wins() {
        assert_eq!(
            DetailLevel::from_args(&json!({"_detail": "primitive"})),
            Some(DetailLevel::Primitive)
        );
        assert_eq!(
            DetailLevel::from_args(&json!({"_detail": "rich"})),
            Some(DetailLevel::Rich)
        );
        assert_eq!(
            DetailLevel::from_args(&json!({"_detail": "core"})),
            Some(DetailLevel::Core)
        );
    }

    #[test]
    fn detail_from_args_honours_legacy_compact_alias() {
        // Phase 5 callers sent `_compact=true` — keep behaviour stable.
        assert_eq!(
            DetailLevel::from_args(&json!({"_compact": true})),
            Some(DetailLevel::Core)
        );
        assert_eq!(
            DetailLevel::from_args(&json!({"_compact": false})),
            Some(DetailLevel::Rich)
        );
    }
}
