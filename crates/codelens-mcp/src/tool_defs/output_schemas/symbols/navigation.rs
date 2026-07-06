use super::evidence::evidence_output_schema;
use serde_json::json;

pub(crate) fn references_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "references": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "column": {"type": "integer"},
                        "line_content": {"type": "string"},
                        "is_declaration": {"type": "boolean"},
                        "enclosing_symbol": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"},
            "returned_count": {"type": "integer"},
            "sampled": {"type": "boolean"},
            "include_context": {"type": "boolean"},
            "backend": {"type": "string"},
            "evidence": evidence_output_schema()
        }
    })
}

pub(crate) fn resolve_symbol_target_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "success": {"type": "boolean"},
            "semantic_backend": {"type": "string"},
            "edit_authority": {"type": "object"},
            "targets": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "column": {"type": "integer"},
                        "end_line": {"type": "integer"},
                        "end_column": {"type": "integer"},
                        "target": {"type": "string"},
                        "method": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"}
        }
    })
}

/// D1 (#346 Phase 4): shared shape for `find_declaration` /
/// `find_implementations` — LSP-resolved locations plus the graceful
/// degradation contract (`degraded_reason` + `fallback_hint`).
pub(crate) fn lsp_navigation_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "success": {"type": "boolean"},
            "operation": {"type": "string", "enum": ["declaration", "implementation"]},
            "backend": {"type": "string", "enum": ["lsp"]},
            "symbol_name": {"type": ["string", "null"]},
            "language": {"type": "string"},
            "position": {
                "type": "object",
                "properties": {
                    "file_path": {"type": "string"},
                    "line": {"type": "integer"},
                    "column": {"type": "integer"}
                }
            },
            "targets": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "column": {"type": "integer"},
                        "end_line": {"type": "integer"},
                        "end_column": {"type": "integer"},
                        "target": {"type": "string"},
                        "method": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"},
            "degraded_reason": {"type": "string"},
            "fallback_hint": {"type": "array", "items": {"type": "string"}}
        }
    })
}
