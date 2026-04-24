use crate::protocol::ToolResponseMeta;
use serde_json::{Value, json};

pub(crate) const EVIDENCE_SCHEMA_VERSION: &str = "codelens-evidence-v1";

pub(crate) fn tool_evidence(
    domain: &str,
    meta: &ToolResponseMeta,
    confidence_basis: &str,
    signals: Value,
) -> Value {
    json!({
        "schema_version": EVIDENCE_SCHEMA_VERSION,
        "domain": domain,
        "active_backend": meta.backend_used,
        "confidence": meta.confidence,
        "confidence_basis": confidence_basis,
        "degraded_reason": meta.degraded_reason,
        "signals": signals,
    })
}

pub(crate) fn precision_signals(
    precise_available: bool,
    precise_used: bool,
    precise_source: Option<&str>,
    fallback_source: Option<&str>,
    precise_result_count: usize,
) -> Value {
    json!({
        "precise_available": precise_available,
        "precise_used": precise_used,
        "precise_source": precise_source,
        "fallback_source": fallback_source,
        "precise_result_count": precise_result_count,
    })
}
