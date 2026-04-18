use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::{BackendKind, ToolResponseMeta};

/// Tool handler result type — every handler returns this.
pub type ToolResult = Result<(serde_json::Value, ToolResponseMeta), CodeLensError>;

pub use crate::tool_defs::tool::{McpTool, ToolBuilder};

pub fn success_meta(backend: BackendKind, confidence: f64) -> ToolResponseMeta {
    ToolResponseMeta {
        backend_used: backend.to_string(),
        confidence,
        degraded_reason: None,
        source: crate::protocol::AnalysisSource::Native,
        partial: false,
        freshness: crate::protocol::Freshness::Live,
        staleness_ms: None,
    }
}

pub fn required_string<'a>(
    value: &'a serde_json::Value,
    key: &str,
) -> Result<&'a str, CodeLensError> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| CodeLensError::MissingParam(key.to_owned()))
}

pub type ToolHandler = fn(&AppState, &serde_json::Value) -> ToolResult;

// ── Common argument extractors ────────────────────────────────────────

/// Extract an optional string argument.
pub fn optional_string<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(|v| v.as_str())
}

/// Extract an optional u64 argument with a default value.
#[allow(dead_code)]
pub fn optional_u64(value: &serde_json::Value, key: &str, default: u64) -> u64 {
    value.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
}

/// Extract an optional usize argument with a default value.
pub fn optional_usize(value: &serde_json::Value, key: &str, default: usize) -> usize {
    value
        .get(key)
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(default)
}

/// Extract an optional bool argument with a default value.
pub fn optional_bool(value: &serde_json::Value, key: &str, default: bool) -> bool {
    value.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}
