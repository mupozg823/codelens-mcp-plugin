use super::evidence::evidence_output_schema;
use serde_json::json;

fn resolution_summary_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "same_file": {"type": "integer"},
            "import_map": {"type": "integer"},
            "import_suffix": {"type": "integer"},
            "unique_name": {"type": "integer"},
            "path_proximity": {"type": "integer"},
            "unresolved": {"type": "integer"}
        }
    })
}

pub(crate) fn get_callers_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "function": {"type": "string"},
            "callers": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file": {"type": "string"},
                        "function": {"type": "string"},
                        "line": {"type": "integer"},
                        "confidence": {"type": "number"},
                        "resolution": {"type": ["string", "null"]}
                    }
                }
            },
            "count": {"type": "integer"},
            "confidence_basis": {
                "type": "string",
                "enum": [
                    "import_evidence",
                    "same_file_only",
                    "name_only_unique",
                    "mixed_with_fallback",
                    "fallback_only",
                    "unresolved_only"
                ]
            },
            "resolution_summary": resolution_summary_output_schema(),
            "evidence": evidence_output_schema()
        }
    })
}

pub(crate) fn get_callees_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "function": {"type": "string"},
            "callees": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "line": {"type": "integer"},
                        "resolved_file": {"type": ["string", "null"]},
                        "confidence": {"type": "number"},
                        "resolution": {"type": ["string", "null"]}
                    }
                }
            },
            "count": {"type": "integer"},
            "confidence_basis": {
                "type": "string",
                "enum": [
                    "import_evidence",
                    "same_file_only",
                    "name_only_unique",
                    "mixed_with_fallback",
                    "fallback_only",
                    "unresolved_only"
                ]
            },
            "resolution_summary": resolution_summary_output_schema(),
            "evidence": evidence_output_schema()
        }
    })
}
