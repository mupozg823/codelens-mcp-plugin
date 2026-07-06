use serde_json::json;

pub(crate) fn embedding_coverage_report_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "compiled": {"type": "boolean"},
            "status": {"type": "string"},
            "reason": {"type": "string"},
            "model_assets": {
                "type": "object",
                "properties": {
                    "available": {"type": "boolean"},
                    "configured_model": {"type": "string"},
                    "model_path": {"type": ["string", "null"]},
                    "sha256": {"type": ["string", "null"]},
                    "size_bytes": {"type": ["integer", "null"]}
                }
            },
            "index": {
                "type": "object",
                "properties": {
                    "model": {"type": "string"},
                    "expected_model": {"type": "string"},
                    "model_mismatch": {"type": "boolean"},
                    "schema_version": {"type": ["integer", "null"]},
                    "expected_schema_version": {"type": "integer"},
                    "schema_mismatch": {"type": ["boolean", "null"]},
                    "indexed_symbols": {"type": "integer"},
                    "indexed_files": {"type": "integer"},
                    "checked_files": {"type": "integer"},
                    "ready_files": {"type": "integer"},
                    "readiness_percent": {"type": "integer", "minimum": 0, "maximum": 100},
                    "unchanged_files": {"type": "integer"},
                    "stale_files": {"type": "integer"},
                    "missing_files": {"type": "integer"},
                    "extra_files": {"type": "integer"},
                    "skipped_new_files": {"type": "integer"},
                    "stale_file_reasons": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "file_path": {"type": "string"},
                                "reason": {
                                    "type": "string",
                                    "enum": [
                                        "missing_embeddings",
                                        "embedding_keys_changed",
                                        "orphaned_embeddings"
                                    ]
                                }
                            }
                        }
                    },
                    "stale_file_reasons_omitted": {"type": "integer"},
                    "current_git_sha": {"type": ["string", "null"]},
                    "last_index_sha": {"type": ["string", "null"]},
                    "last_index_sha_source": {
                        "type": "string",
                        "enum": ["persisted", "inferred_current_clean_index", "unavailable"]
                    },
                    "freshness": {
                        "type": "object",
                        "properties": {
                            "schema": {"type": "object"},
                            "model": {"type": "object"},
                            "git": {"type": "object"},
                            "files": {"type": "object"}
                        }
                    }
                }
            },
            "query_cache": {
                "type": "object",
                "properties": {
                    "enabled": {"type": "boolean"},
                    "entries": {"type": "integer"},
                    "max_entries": {"type": "integer"}
                }
            },
            "recommended_action": {"type": "string"},
            "remediation": {
                "type": "object",
                "properties": {
                    "reason": {"type": "string"},
                    "action": {"type": "string"},
                    "description": {"type": "string"}
                }
            }
        }
    })
}
