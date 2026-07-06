use super::evidence::evidence_output_schema;
use serde_json::json;

pub(crate) fn symbol_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "symbols": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "kind": {"type": "string", "enum": ["function","class","method","interface","enum","variable","module","typealias"]},
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "column": {"type": "integer"},
                        "signature": {"type": "string"},
                        "body": {"type": ["string", "null"]},
                        "name_path": {"type": "string"},
                        "id": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"},
            "truncated": {"type": "boolean"},
            "auto_summarized": {"type": "boolean"},
            "body_truncated_count": {"type": "integer"},
            "body_preview": {"type": "boolean"},
            "evidence": evidence_output_schema()
        }
    })
}
