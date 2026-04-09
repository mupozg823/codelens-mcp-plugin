use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::{BackendKind, ToolResponseMeta};

/// Tool handler result type — every handler returns this.
pub type ToolResult = Result<(serde_json::Value, ToolResponseMeta), CodeLensError>;

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
