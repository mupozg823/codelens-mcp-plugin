//! One candidate contract: generated descriptors also drive dispatch validation
//! and tools/list fingerprints. Private target names are never a public API.

use crate::error::CodeLensError;
use crate::protocol::{Tool, ToolAnnotations, ToolTier};
use serde_json::Value;
use std::sync::LazyLock;

static TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(|| {
    let annotations = ToolAnnotations::read_only().with_tier(ToolTier::Analysis);
    let mut tools = super::generated::analysis_lite_tools(&annotations);
    for tool in &mut tools {
        tool.estimated_tokens = serde_json::to_string(tool)
            .expect("generated descriptor serializes")
            .len()
            / 4;
    }
    tools
});

pub(crate) fn tools() -> &'static [Tool] {
    &TOOLS
}

pub(crate) fn max_response_bytes() -> usize {
    super::generated::ANALYSIS_LITE_MAX_RESPONSE_BYTES
}

pub(crate) fn public_name(target: &str) -> Option<&'static str> {
    super::generated::analysis_lite_public_name(target)
}

pub(crate) fn resolve(name: &str, arguments: &Value) -> Result<&'static str, CodeLensError> {
    let tool = tools()
        .iter()
        .find(|tool| tool.name == name)
        .ok_or_else(|| {
            CodeLensError::Validation(format!("Tool `{name}` is not available in analysis-lite"))
        })?;
    validate(&tool.input_schema, arguments, "arguments", true)?;
    super::generated::analysis_lite_target(name)
        .ok_or_else(|| CodeLensError::ToolNotFound(name.to_owned()))
}

/// Defaults and bounds are read from the same schema presented to the client.
pub(crate) fn limit(target: &str, arguments: &Value, key: &str) -> usize {
    let tool = tools()
        .iter()
        .find(|tool| Some(tool.name) == public_name(target))
        .expect("generated candidate target has a descriptor");
    arguments
        .get(key)
        .and_then(Value::as_u64)
        .or_else(|| tool.input_schema["properties"][key]["default"].as_u64())
        .expect("candidate limit is schema-validated and has a default") as usize
}

// The canonical contract deliberately uses only this JSON Schema subset.
// This validates actual values, not just required-field presence. Transport
// routing metadata is permitted at the argument root, never inside user data.
fn validate(
    schema: &Value,
    value: &Value,
    field: &str,
    transport_root: bool,
) -> Result<(), CodeLensError> {
    let fail = |detail: &str| CodeLensError::Validation(format!("analysis-lite {field}: {detail}"));
    let correct_type = match schema.get("type").and_then(Value::as_str) {
        Some("object") => value.is_object(),
        Some("string") => value.is_string(),
        Some("integer") => value.as_u64().is_some(),
        Some("boolean") => value.is_boolean(),
        None => true,
        Some(_) => return Err(fail("unsupported canonical schema type")),
    };
    if !correct_type {
        return Err(fail("wrong type"));
    }
    if let Some(choices) = schema.get("enum").and_then(Value::as_array)
        && !choices.contains(value)
    {
        return Err(fail(&format!("expected one of {choices:?}")));
    }
    if let Some(min) = schema.get("minLength").and_then(Value::as_u64)
        && value
            .as_str()
            .is_some_and(|s| s.chars().count() < min as usize)
    {
        return Err(fail("empty or too-short string"));
    }
    if let Some(n) = value.as_u64()
        && (schema
            .get("minimum")
            .and_then(Value::as_u64)
            .is_some_and(|min| n < min)
            || schema
                .get("maximum")
                .and_then(Value::as_u64)
                .is_some_and(|max| n > max))
    {
        return Err(fail("integer outside advertised bounds"));
    }
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str) {
            if value.get(key).is_none_or(Value::is_null) {
                return Err(fail(&format!("missing {key}")));
            }
        }
    }
    if let Some(alternatives) = schema.get("anyOf").and_then(Value::as_array)
        && !alternatives
            .iter()
            .any(|schema| validate(schema, value, field, false).is_ok())
    {
        return Err(fail("requires a declaration name or exact symbol_id"));
    }
    if let Some(object) = value.as_object() {
        for (key, child) in object {
            if transport_root && key.starts_with('_') {
                continue;
            }
            if let Some(property) = schema.get("properties").and_then(|p| p.get(key)) {
                validate(property, child, &format!("{field}.{key}"), false)?;
            } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                return Err(fail(&format!("unknown property {key}")));
            }
        }
    }
    Ok(())
}
