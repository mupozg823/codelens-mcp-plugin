use serde_json::json;

pub(super) fn skill_hints_output_schema() -> serde_json::Value {
    json!({
        "type": ["object", "null"],
        "properties": {
            "target_host": {"type": "string"},
            "catalog_resource": {"type": "string"},
            "total_skill_count": {"type": ["integer", "null"]},
            "roots": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "path": {"type": ["string", "null"]},
                        "exists": {"type": ["boolean", "null"]},
                        "skill_count": {"type": ["integer", "null"]},
                        "truncated": {"type": ["boolean", "null"]}
                    }
                }
            },
            "selection_limit": {"type": "integer"},
            "load_policy": {"type": "string"},
            "candidate_skills": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "path": {"type": "string"},
                        "source_root": {"type": "string"},
                        "score": {"type": "integer"},
                        "matched_terms": {"type": "array", "items": {"type": "string"}},
                        "description": {"type": "string"},
                        "mtime_epoch_secs": {"type": "integer"},
                        "content_hash": {"type": "string"},
                        "load_policy": {"type": "string"}
                    }
                }
            },
            "recommended_sequence": {"type": "array", "items": {"type": "string"}}
        }
    })
}

pub(super) fn host_environment_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "client_profile": {"type": "string"},
            "client_name": {"type": ["string", "null"]},
            "client_version": {"type": ["string", "null"]},
            "snapshot_source": {"type": "string", "enum": ["explicit_host_snapshot", "session_defaults"]},
            "requested_profile": {"type": ["string", "null"]},
            "harness_profile": {"type": ["string", "null"]},
            "deferred_tool_loading": {"type": "boolean"},
            "loaded_namespaces": {"type": "array", "items": {"type": "string"}},
            "loaded_tiers": {"type": "array", "items": {"type": "string"}},
            "full_tool_exposure": {"type": "boolean"},
            "available_mcp_servers": {"type": "array", "items": {"type": "string"}},
            "available_mcp_tools": {"type": "array", "items": {"type": "string"}},
            "skill_roots": {"type": "array", "items": {"type": "string"}},
            "skill_root_source": {"type": "string", "enum": ["host_snapshot", "codex_default_roots", "none"]},
            "memory_roots": {"type": "array", "items": {"type": "string"}},
            "memory_entrypoints": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "root": {"type": "string"},
                        "path": {"type": "string"},
                        "relative_path": {"type": "string"},
                        "kind": {"type": "string"},
                        "reason": {"type": "string"}
                    }
                }
            },
            "host_setting_keys": {"type": "array", "items": {"type": "string"}},
            "available_mcp_server_count": {"type": "integer"},
            "available_mcp_tool_count": {"type": "integer"},
            "skill_root_count": {"type": "integer"},
            "memory_root_count": {"type": "integer"},
            "memory_entrypoint_count": {"type": "integer"},
            "host_setting_key_count": {"type": "integer"},
            "counts": {"type": "object"},
            "adaptation_notes": {"type": "array", "items": {"type": "string"}}
        }
    })
}

pub(super) fn overlay_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "applied": {"type": "boolean"},
            "host_context": {"type": ["string", "null"]},
            "task_overlay": {"type": ["string", "null"]},
            "agent_role": {"type": ["string", "null"]},
            "preferred_executor_bias": {"type": ["string", "null"]},
            "preferred_entrypoints": {"type": "array", "items": {"type": "string"}},
            "preferred_entrypoints_visible": {"type": "array", "items": {"type": "string"}},
            "emphasized_tools": {"type": "array", "items": {"type": "string"}},
            "emphasized_tools_visible": {"type": "array", "items": {"type": "string"}},
            "avoid_tools": {"type": "array", "items": {"type": "string"}},
            "avoid_tools_visible": {"type": "array", "items": {"type": "string"}},
            "routing_notes": {"type": "array", "items": {"type": "string"}}
        }
    })
}

pub(super) fn coordination_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "mode": {"type": "string"},
            "risk_status": {"type": "string", "enum": ["idle", "observe", "caution"]},
            "active_agents": {"type": "integer"},
            "active_claims": {"type": "integer"},
            "recommended_sequence": {"type": "array", "items": {"type": "string"}},
            "host_actions_on_overlap": {"type": "array", "items": {"type": "string"}},
            "recommended_topology": {"type": "string"}
        }
    })
}
