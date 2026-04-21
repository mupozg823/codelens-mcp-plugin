use serde_json::json;

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

pub(crate) fn prune_index_failures_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "pruned_missing_failures": {"type": "integer"},
            "index_failures": {"type": "integer"},
            "index_failures_total": {"type": "integer"},
            "stale_index_failures": {"type": "integer"},
            "persistent_index_failures": {"type": "integer"},
            "recent_failure_window_seconds": {"type": "integer"}
        }
    })
}

pub(crate) fn watch_status_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "running": {"type": "boolean"},
            "events_processed": {"type": "integer"},
            "files_reindexed": {"type": "integer"},
            "lock_contention_batches": {"type": "integer"},
            "index_failures": {"type": ["integer", "null"]},
            "index_failures_total": {"type": "integer"},
            "stale_index_failures": {"type": "integer"},
            "persistent_index_failures": {"type": "integer"},
            "pruned_missing_failures": {"type": "integer"},
            "recent_failure_window_seconds": {"type": "integer"},
            "note": {"type": "string"}
        }
    })
}

pub(crate) fn tool_metrics_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "tools": {"type": "array", "items": {"type": "object"}},
            "per_tool": {"type": "array", "items": {"type": "object"}},
            "count": {"type": "integer"},
            "surfaces": {"type": "array", "items": {"type": "object"}},
            "per_surface": {"type": "array", "items": {"type": "object"}},
            "scope": {"type": "string", "enum": ["global", "session"]},
            "session_id": {"type": ["string", "null"]},
            "session": {"type": "object"},
            "derived_kpis": {"type": "object"}
        }
    })
}

pub(crate) fn builder_session_audit_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "status": {"type": "string", "enum": ["pass", "warn", "fail", "not_applicable"]},
            "score": {"type": "number", "minimum": 0.0, "maximum": 1.0},
            "checks": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "code": {"type": "string"},
                        "status": {"type": "string", "enum": ["pass", "warn", "fail", "not_applicable"]},
                        "summary": {"type": "string"},
                        "evidence": {"type": "object"}
                    }
                }
            },
            "findings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "code": {"type": "string"},
                        "severity": {"type": "string", "enum": ["warn", "fail"]},
                        "summary": {"type": "string"},
                        "evidence": {"type": "object"}
                    }
                }
            },
            "recommended_next_tools": {"type": "array", "items": {"type": "string"}},
            "session_summary": {"type": "object"},
            "session_metrics": {"type": "object"},
            "coordination_snapshot": {"type": "object"}
        }
    })
}

pub(crate) fn planner_session_audit_output_schema() -> serde_json::Value {
    builder_session_audit_output_schema()
}

pub(crate) fn session_markdown_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "required": ["markdown", "session_name", "scope", "tool_count", "total_calls"],
        "properties": {
            "markdown": {"type": "string"},
            "session_name": {"type": "string"},
            "session_id": {"type": ["string", "null"]},
            "scope": {"type": "string", "enum": ["global", "session"]},
            "tool_count": {"type": "integer"},
            "total_calls": {"type": "integer"}
        }
    })
}

pub(crate) fn activate_project_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "activated": {"type": "boolean"},
            "switched": {"type": "boolean"},
            "project_name": {"type": "string"},
            "project_base_path": {"type": "string"},
            "backend_id": {"type": "string"},
            "memory_count": {"type": "integer"},
            "file_watcher": {"type": "boolean"},
            "frameworks": {"type": "array", "items": {"type": "string"}},
            "auto_surface": {"type": "string"},
            "auto_budget": {"type": "integer"},
            "indexed_files": {"type": "integer"},
            "embedding_ready": {"type": "boolean"}
        }
    })
}

pub(crate) fn prepare_harness_session_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "activated": {"type": "boolean"},
            "project": {"type": "object"},
            "active_surface": {"type": "string"},
            "token_budget": {"type": "integer"},
            "config": {"type": "object"},
            "index_recovery": {"type": "object"},
            "capabilities": {"type": "object"},
            "health_summary": health_summary_output_schema(),
            "warnings": {"type": "array"},
            "overlay": {"type": "object"},
            "coordination": {"type": "object"},
            "http_session": {"type": "object"},
            "visible_tools": {"type": "object"},
            "routing": {"type": "object"},
            "harness": {"type": "object"},
            "shared_analysis_pool": {"type": "array"}
        }
    })
}

pub(crate) fn get_capabilities_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "language": {"type": ["string", "null"]},
            "lsp_attached": {"type": "boolean"},
            "coordination_mode": {"type": "string", "enum": ["advisory", "strict"]},
            "coordination_enforcement": {"type": "object"},
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
            "semantic_runtime_ready": {"type": "boolean"},
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
            "intelligence_sources": {"type": "array", "items": {"type": "string"}},
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
            "binary_git_sha": {"type": "string"},
            "binary_build_time": {"type": "string"},
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
            },
            "scip_available": {"type": "boolean"},
            "scip_file_count": {"type": ["integer", "null"]},
            "scip_symbol_count": {"type": ["integer", "null"]}
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
            "effort_level": {"type": "string"},
            "daemon_mode": {"type": "boolean"},
            "transport": {"type": "string"}
        }
    })
}

pub(crate) fn register_agent_work_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "entry": {"type": "object"},
            "note": {"type": "string"}
        }
    })
}

pub(crate) fn list_active_agents_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "agents": {"type": "array"},
            "count": {"type": "integer"}
        }
    })
}

pub(crate) fn claim_files_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "claim": {"type": "object"},
            "overlapping_claims": {"type": "array"},
            "note": {"type": "string"}
        }
    })
}

pub(crate) fn release_files_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string"},
            "released_paths": {"type": "array"},
            "released_count": {"type": "integer"},
            "remaining_claim": {"type": ["object", "null"]},
            "remaining_claim_count": {"type": "integer"}
        }
    })
}
