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
        let mut arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        // P1-C: tool parameter naming consistency. Different tools have
        // historically expected `file_path` (capabilities, diagnostics,
        // refactor) or `path` (impact reports, workflows) for the same
        // semantic input. Onboarding clients hit -32602 when they
        // guess wrong. We normalise here so callers may use either
        // name; the canonical key required by the handler is
        // populated alongside whatever the client sent.
        //
        // `relative_path` is *not* aliased — mutation primitives use
        // it as a deliberate "rooted at project, no escape" marker
        // distinct from arbitrary paths. Preserving the divergence
        // keeps that contract honest.
        apply_path_alias_normalisation(&mut arguments);
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

/// P1-C: bidirectional alias between `file_path` and `path`. When the
/// caller sets one, populate the other with the same value so handler
/// `arguments.get("...")` lookups succeed regardless of which name
/// the schema actually requires. If both are present and differ, the
/// caller's choice wins (we do not overwrite either side); if both are
/// present and equal, this is a no-op.
///
/// `relative_path` is intentionally NOT aliased — see envelope::parse
/// for the rationale.
fn apply_path_alias_normalisation(arguments: &mut serde_json::Value) {
    let Some(obj) = arguments.as_object_mut() else {
        return;
    };
    let has_file_path = obj.contains_key("file_path");
    let has_path = obj.contains_key("path");
    match (has_file_path, has_path) {
        (true, false) => {
            if let Some(value) = obj.get("file_path").cloned() {
                obj.insert("path".to_owned(), value);
            }
        }
        (false, true) => {
            if let Some(value) = obj.get("path").cloned() {
                obj.insert("file_path".to_owned(), value);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run_alias(value: serde_json::Value) -> serde_json::Value {
        let mut v = value;
        apply_path_alias_normalisation(&mut v);
        v
    }

    #[test]
    fn file_path_alone_is_aliased_to_path() {
        let out = run_alias(json!({ "file_path": "src/foo.rs" }));
        assert_eq!(out["file_path"], "src/foo.rs");
        assert_eq!(out["path"], "src/foo.rs");
    }

    #[test]
    fn path_alone_is_aliased_to_file_path() {
        let out = run_alias(json!({ "path": "src/bar.rs" }));
        assert_eq!(out["path"], "src/bar.rs");
        assert_eq!(out["file_path"], "src/bar.rs");
    }

    #[test]
    fn both_keys_present_left_intact() {
        let out = run_alias(json!({
            "file_path": "src/foo.rs",
            "path": "src/bar.rs",
        }));
        assert_eq!(out["file_path"], "src/foo.rs");
        assert_eq!(out["path"], "src/bar.rs");
    }

    #[test]
    fn neither_key_present_no_op() {
        let out = run_alias(json!({ "name": "MyType" }));
        assert!(out.get("file_path").is_none());
        assert!(out.get("path").is_none());
    }

    #[test]
    fn relative_path_not_aliased() {
        // Mutation primitives keep `relative_path` distinct from
        // both `file_path` and `path` to mark the rooted-no-escape
        // contract.
        let out = run_alias(json!({ "relative_path": "src/foo.rs" }));
        assert_eq!(out["relative_path"], "src/foo.rs");
        assert!(out.get("file_path").is_none());
        assert!(out.get("path").is_none());
    }

    #[test]
    fn non_object_argument_no_op() {
        let out = run_alias(json!("scalar string"));
        assert_eq!(out, json!("scalar string"));
        let out = run_alias(json!([1, 2, 3]));
        assert_eq!(out, json!([1, 2, 3]));
    }
}
