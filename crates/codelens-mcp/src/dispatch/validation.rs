//! Schema pre-validation: check `required` fields before the handler runs.

use crate::error::CodeLensError;

/// Check that all `required` fields from the tool's input_schema are present.
/// Returns early with MissingParam error before the handler runs.
pub(crate) fn validate_required_params(
    name: &str,
    arguments: &serde_json::Value,
) -> Result<(), CodeLensError> {
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
                return Err(CodeLensError::MissingParam(key.to_owned()));
            }
        }
    }
    Ok(())
}
