//! Tool call envelope — normalized JSON-RPC params with profile/compact/harness routing.

use crate::AppState;
use crate::tool_defs::{ToolProfile, default_budget_for_profile};
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
        // `relative_path` is only aliased for read-only soft-alias tools.
        // Mutation primitives keep it as a deliberate "rooted at project,
        // no escape" marker distinct from arbitrary paths.
        apply_path_alias_normalisation(name.as_str(), &mut arguments);
        let session = crate::session_context::SessionRequestContext::from_json(&arguments);
        let default_budget = state.execution_token_budget(&session);
        // P2-A: honour explicit max_tokens parameter before profile defaults.
        // Agents pass max_tokens per-request; ignoring it caused hard 4000-cap
        // errors even when the caller asked for 6000+.
        let budget = arguments
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .or_else(|| {
                arguments
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

/// Check that all `required` fields from the tool's input_schema are present.
/// Returns early with MissingParam error before the handler runs.
pub(crate) fn validate_required_params(
    name: &str,
    arguments: &serde_json::Value,
) -> Result<(), crate::error::CodeLensError> {
    let tool = match crate::tool_defs::tool_definition(name) {
        Some(t) => t,
        None => return Ok(()), // unknown tool handled later by dispatch table
    };
    let required = match tool.input_schema.get("required").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return Ok(()), // no required fields
    };
    for field in required {
        if let Some(key) = field.as_str() {
            // Skip routing metadata (underscore-prefixed) — never user-visible
            if key.starts_with('_') {
                continue;
            }
            let present = arguments
                .get(key)
                .is_some_and(|v| !v.is_null() && v.as_str() != Some(""));
            if !present {
                return Err(crate::error::CodeLensError::MissingParam(key.to_owned()));
            }
        }
    }
    Ok(())
}

/// P1-C: bidirectional alias between `file_path` and `path`. When the
/// caller sets one, populate the other with the same value so handler
/// `arguments.get("...")` lookups succeed regardless of which name
/// the schema actually requires. If both are present and differ, the
/// caller's choice wins (we do not overwrite either side); if both are
/// present and equal, this is a no-op.
///
/// `relative_path` is intentionally only aliased for read-only tools that
/// explicitly maintain it as a soft alias — see envelope::parse for the
/// mutation-safety rationale.
fn apply_path_alias_normalisation(tool_name: &str, arguments: &mut serde_json::Value) {
    let Some(obj) = arguments.as_object_mut() else {
        return;
    };
    let has_file_path = obj.contains_key("file_path");
    let has_path = obj.contains_key("path");
    let has_relative_path = obj.contains_key("relative_path");
    let supports_relative_path_alias = matches!(
        tool_name,
        "get_symbols_overview" | "find_referencing_symbols" | "get_file_diagnostics"
    );
    match (has_file_path, has_path) {
        (true, false) => {
            if let Some(value) = obj.get("file_path").cloned() {
                obj.insert("path".to_owned(), value);
                obj.insert("_path_alias_source".to_owned(), json!("file_path"));
            }
        }
        (false, true) => {
            if let Some(value) = obj.get("path").cloned() {
                obj.insert("file_path".to_owned(), value);
            }
        }
        (false, false) if supports_relative_path_alias && has_relative_path => {
            if let Some(value) = obj.get("relative_path").cloned() {
                obj.insert("path".to_owned(), value.clone());
                obj.insert("file_path".to_owned(), value);
                obj.insert("_path_alias_source".to_owned(), json!("relative_path"));
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
        apply_path_alias_normalisation("test_tool", &mut v);
        v
    }

    #[test]
    fn file_path_alone_is_aliased_to_path() {
        let out = run_alias(json!({ "file_path": "src/foo.rs" }));
        assert_eq!(out["file_path"], "src/foo.rs");
        assert_eq!(out["path"], "src/foo.rs");
        assert_eq!(out["_path_alias_source"], "file_path");
    }

    #[test]
    fn path_alone_is_aliased_to_file_path() {
        let out = run_alias(json!({ "path": "src/bar.rs" }));
        assert_eq!(out["path"], "src/bar.rs");
        assert_eq!(out["file_path"], "src/bar.rs");
        assert!(out.get("_path_alias_source").is_none());
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
    fn relative_path_alias_is_limited_to_soft_alias_tools() {
        let mut out = json!({ "relative_path": "src/foo.rs" });
        apply_path_alias_normalisation("get_symbols_overview", &mut out);
        assert_eq!(out["relative_path"], "src/foo.rs");
        assert_eq!(out["file_path"], "src/foo.rs");
        assert_eq!(out["path"], "src/foo.rs");
        assert_eq!(out["_path_alias_source"], "relative_path");
    }

    #[test]
    fn non_object_argument_no_op() {
        let out = run_alias(json!("scalar string"));
        assert_eq!(out, json!("scalar string"));
        let out = run_alias(json!([1, 2, 3]));
        assert_eq!(out, json!([1, 2, 3]));
    }
}
