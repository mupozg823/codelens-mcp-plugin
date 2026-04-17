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
        let compact = arguments
            .get("_compact")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
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
