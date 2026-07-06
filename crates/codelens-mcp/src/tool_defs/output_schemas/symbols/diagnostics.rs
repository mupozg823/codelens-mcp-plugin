use serde_json::json;

pub(crate) fn diagnostics_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "diagnostics": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "column": {"type": "integer"},
                        "end_line": {"type": "integer"},
                        "end_column": {"type": "integer"},
                        "severity": {"type": ["integer", "null"]},
                        "severity_label": {"type": ["string", "null"]},
                        "code": {"type": ["string", "null"]},
                        "source": {"type": ["string", "null"]},
                        "message": {"type": "string"},
                        "classification": {"type": "string"},
                        "actionability": {"type": "string"},
                        "recommended_action": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"},
            "backend": {"type": "string", "enum": ["lsp", "scip"]},
            "suppressed_diagnostics_count": {"type": "integer"},
            "suppressed_diagnostics": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "column": {"type": "integer"},
                        "code": {"type": ["string", "null"]},
                        "source": {"type": ["string", "null"]},
                        "message": {"type": "string"},
                        "suppression": {"type": "string"}
                    }
                }
            }
        }
    })
}

/// D1 (#346 Phase 4): `get_diagnostics_for_symbol` — the file
/// diagnostics shape filtered to one symbol's span.
pub(crate) fn symbol_diagnostics_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "success": {"type": "boolean"},
            "symbol": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "kind": {"type": "string"},
                    "file_path": {"type": "string"},
                    "span": {
                        "type": "object",
                        "properties": {
                            "start_line": {"type": "integer"},
                            "end_line": {"type": "integer"}
                        }
                    }
                }
            },
            "diagnostics": {"type": "array", "items": {"type": "object"}},
            "count": {"type": "integer"},
            "file_diagnostics_count": {"type": "integer"},
            "backend": {"type": ["string", "null"]},
            "degraded_reason": {"type": "string"},
            "fallback_hint": {"type": "array", "items": {"type": "string"}}
        }
    })
}
