use serde_json::json;

pub(super) fn evidence_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "schema_version": {"type": "string", "enum": ["codelens-evidence-v1"]},
            "domain": {"type": "string", "enum": ["call_graph", "retrieval", "symbol", "references"]},
            "active_backend": {
                "type": "string",
                "enum": ["tree-sitter", "hybrid", "semantic", "sqlite", "scip", "lsp"]
            },
            "confidence": {"type": "number"},
            "confidence_basis": {"type": "string"},
            "degraded_reason": {"type": ["string", "null"]},
            "signals": {"type": "object"}
        }
    })
}
