//! Output schemas for harness capabilities, configuration, and session tools.

use serde_json::json;

mod prepare;

pub(crate) use prepare::prepare_harness_session_output_schema;

fn health_summary_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "status": {"type": "string", "enum": ["ok", "warn"]},
            "warning_count": {"type": "integer"},
            "warnings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "code": {"type": "string"},
                        "severity": {"type": "string", "enum": ["warn"]},
                        "message": {"type": "string"},
                        "recommended_action": {"type": ["string", "null"]},
                        "action_target": {"type": ["string", "null"]}
                    }
                }
            }
        }
    })
}

fn surface_generation_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "schema_version": {"type": "integer"},
            "binary_version": {"type": "string"},
            "tool_schema_fingerprint": {"type": "string"},
            "refresh_action": {"type": "string", "enum": ["reissue_tools_list_or_reconnect"]},
            "refresh_hint": {"type": "string"},
            "runtime": {
                "type": "object",
                "properties": {
                    "binary_git_sha": {"type": "string"},
                    "binary_build_time": {"type": "string"}
                }
            }
        }
    })
}

pub(crate) fn get_capabilities_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "language": {"type": ["string", "null"]},
            "lsp_attached": {"type": "boolean"},
            "diagnostics_guidance": {
                "type": "object",
                "properties": {
                    "status": {"type": "string", "enum": ["available", "file_path_required", "unsupported_extension", "lsp_binary_missing"]},
                    "available": {"type": "boolean"},
                    "reason": {"type": ["string", "null"]},
                    "reason_code": {"type": ["string", "null"]},
                    "recommended_action": {"type": ["string", "null"]},
                    "action_target": {"type": ["string", "null"]},
                    "file_extension": {"type": ["string", "null"]},
                    "language": {"type": ["string", "null"]},
                    "lsp_command": {"type": ["string", "null"]},
                    "server_name": {"type": ["string", "null"]},
                    "install_command": {"type": ["string", "null"]},
                    "package_manager": {"type": ["string", "null"]}
                }
            },
            "embeddings_loaded": {"type": "boolean"},
            "semantic_search_status": {"type": "string", "enum": ["available", "model_assets_unavailable", "not_in_active_surface", "index_missing", "feature_disabled", "not_compiled"]},
            "semantic_search_guidance": {
                "type": "object",
                "properties": {
                    "status": {"type": "string", "enum": ["available", "model_assets_unavailable", "not_in_active_surface", "index_missing", "feature_disabled", "not_compiled"]},
                    "available": {"type": "boolean"},
                    "reason": {"type": ["string", "null"]},
                    "reason_code": {"type": ["string", "null"]},
                    "recommended_action": {"type": ["string", "null"]},
                    "action_target": {"type": ["string", "null"]}
                }
            },
            "embedding_model": {"type": "string"},
            "embedding_runtime_preference": {"type": "string"},
            "embedding_runtime_backend": {"type": "string"},
            "embedding_threads": {"type": ["integer", "null"]},
            "embedding_max_length": {"type": ["integer", "null"]},
            "embedding_indexed": {"type": "boolean"},
            "embedding_indexed_symbols": {"type": "integer"},
            "index_fresh": {"type": "boolean"},
            "indexed_files": {"type": "integer"},
            "supported_files": {"type": "integer"},
            "stale_files": {"type": "integer"},
            "health_summary": health_summary_output_schema(),
            "available": {"type": "array", "items": {"type": "string"}},
            "unavailable": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "feature": {"type": "string"},
                        "reason": {"type": "string"},
                        "status": {"type": "string"},
                        "reason_code": {"type": ["string", "null"]},
                        "recommended_action": {"type": ["string", "null"]},
                        "action_target": {"type": ["string", "null"]},
                        "file_extension": {"type": ["string", "null"]},
                        "language": {"type": ["string", "null"]},
                        "lsp_command": {"type": ["string", "null"]},
                        "server_name": {"type": ["string", "null"]},
                        "install_command": {"type": ["string", "null"]},
                        "package_manager": {"type": ["string", "null"]}
                    }
                }
            },
            "binary_version": {"type": "string"},
            "daemon_started_at": {"type": "string"},
            "daemon_binary_drift": {
                "type": "object",
                "properties": {
                    "status": {"type": "string", "enum": ["ok", "stale", "unknown"]},
                    "stale_daemon": {"type": "boolean"},
                    "restart_recommended": {"type": "boolean"},
                    "reason_code": {"type": ["string", "null"]},
                    "recommended_action": {"type": ["string", "null"]},
                    "action_target": {"type": ["string", "null"]},
                    "executable_path": {"type": "string"},
                    "executable_modified_at": {"type": "string"},
                    "daemon_started_at": {"type": "string"},
                    "binary_build_time": {"type": "string"},
                    "binary_git_sha": {"type": "string"},
                    "reason": {"type": ["string", "null"]}
                }
            },
            "binary_build_info": {
                "type": "object",
                "properties": {
                    "version": {"type": "string"},
                    "git_sha": {"type": "string"},
                    "git_dirty": {"type": "boolean"},
                    "build_time": {"type": "string"}
                }
            }
        }
    })
}

pub(crate) fn get_current_config_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "project_name": {"type": "string"},
            "project_base_path": {"type": "string"},
            "preset": {"type": "string"},
            "profile": {"type": "string"},
            "surface": {"type": "string"},
            "token_budget": {"type": "integer"},
            "tool_count": {"type": "integer"},
            "surface_generation": surface_generation_output_schema(),
            "project_activation": {
                "type": "object",
                "properties": {
                    "status": {"type": "string"},
                    "active_project_root": {"type": "string"},
                    "daemon_default_project_root": {"type": "string"},
                    "native_fallback_recommended": {"type": "boolean"},
                    "recommended_action": {"type": "string"},
                    "message": {"type": "string"},
                    "remediation": {"type": "object"}
                }
            },
            "effort_level": {"type": "string"},
            "daemon_mode": {"type": "boolean"},
            "transport": {"type": "string"}
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

pub(crate) fn get_type_hierarchy_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "root": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "kind": {"type": "string"},
                    "file": {"type": "string"},
                    "line": {"type": "integer"},
                    "children": {"type": "array"}
                }
            },
            "depth": {"type": "integer"}
        }
    })
}
