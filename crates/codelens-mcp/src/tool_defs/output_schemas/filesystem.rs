use serde_json::json;

pub(crate) fn file_content_output_schema() -> serde_json::Value {
    json!({"type":"object","properties":{"content":{"type":"string"}}})
}

pub(crate) fn changed_files_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "files": {"type": "array", "items": {"type": "object", "properties": {
                "path": {"type": "string"}, "status": {"type": "string"},
                "symbol_count": {"type": "integer"}
            }}},
            "count": {"type": "integer"}
        }
    })
}

pub(crate) fn onboard_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "project_root": {"type": "string"},
            "directory_structure": {"type": "array"},
            "key_files": {"type": "array"},
            "circular_dependencies": {"type": "array"},
            "health": {"type": "object"},
            "semantic": {"type": "object"},
            "suggested_next_tools": {"type": "array", "items": {"type": "string"}}
        }
    })
}

pub(crate) fn memory_list_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "memories": {"type": "array", "items": {"type": "string"}},
            "count": {"type": "integer"}
        }
    })
}

pub(crate) fn replace_content_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "content": {"type": "string", "description": "Full updated file content after replacement"},
            "replacements": {"type": "integer", "description": "Number of replacements performed (text mode only)"}
        }
    })
}

pub(crate) fn create_text_file_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "created": {"type": "string", "description": "Relative path of the newly created file"}
        }
    })
}

pub(crate) fn add_import_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "success": {"type": "boolean"},
            "file_path": {"type": "string"},
            "content_length": {"type": "integer", "description": "Byte length of the updated file content"}
        }
    })
}

pub(crate) fn search_for_pattern_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "matches": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file": {"type": "string"},
                        "line": {"type": "integer"},
                        "column": {"type": "integer"},
                        "text": {"type": "string"},
                        "context_before": {"type": "string"},
                        "context_after": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"},
            "truncated": {"type": "boolean"}
        }
    })
}

pub(crate) fn find_annotations_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "annotations": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file": {"type": "string"},
                        "line": {"type": "integer"},
                        "tag": {"type": "string"},
                        "text": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"}
        }
    })
}

pub(crate) fn find_tests_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "tests": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file": {"type": "string"},
                        "name": {"type": "string"},
                        "line": {"type": "integer"},
                        "kind": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"}
        }
    })
}

pub(crate) fn get_project_structure_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "directories": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "files": {"type": "integer"},
                        "languages": {"type": "array", "items": {"type": "string"}}
                    }
                }
            },
            "total_files": {"type": "integer"},
            "total_directories": {"type": "integer"}
        }
    })
}
