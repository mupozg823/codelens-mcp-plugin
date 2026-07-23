use serde_json::json;

use super::super::health_summary_output_schema;

pub(super) fn config_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "runtime": {"type": "string"},
            "project_root": {"type": "string"},
            "surface": {"type": "string"},
            "token_budget": {"type": "integer"},
            "tool_count": {"type": "integer"},
            "client_profile": {"type": "string"}
        }
    })
}

pub(super) fn index_recovery_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "enabled": {"type": "boolean"},
            "threshold": {"type": ["integer", "null"]},
            "status": {"type": "string"},
            "reason": {"type": "string"},
            "error": {"type": "string"},
            "before": {
                "type": "object",
                "properties": {
                    "indexed_files": {"type": "integer"},
                    "supported_files": {"type": "integer"},
                    "stale_files": {"type": "integer"}
                }
            },
            "after": {
                "type": "object",
                "properties": {
                    "indexed_files": {"type": "integer"},
                    "supported_files": {"type": "integer"},
                    "stale_files": {"type": "integer"}
                }
            }
        }
    })
}

pub(super) fn warnings_output_schema() -> serde_json::Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "code": {"type": "string"},
                "message": {"type": "string"},
                "restart_recommended": {"type": "boolean"},
                "recommended_action": {"type": "string"},
                "action_target": {"type": "string"}
            }
        }
    })
}

pub(super) fn http_session_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "enabled": {"type": "boolean"},
            "active_sessions": {"type": "integer"},
            "active_coordination_agents": {"type": "integer"},
            "active_coordination_claims": {"type": "integer"},
            "timeout_seconds": {"type": "integer"},
            "resume_supported": {"type": "boolean"},
            "daemon_mode": {"type": "string"},
            "client_profile": {"type": "string"},
            "client_name": {"type": ["string", "null"]},
            "active_surface": {"type": "string"},
            "semantic_search_status": {"type": "string", "enum": ["available", "model_assets_unavailable", "not_in_active_surface", "index_missing", "feature_disabled", "not_compiled"]},
            "indexed_files": {"type": "integer"},
            "supported_files": {"type": "integer"},
            "stale_files": {"type": "integer"},
            "daemon_binary_drift": {
                "type": "object",
                "properties": {
                    "status": {"type": "string"},
                    "stale_daemon": {"type": "boolean"},
                    "reason": {"type": "string"},
                    "reason_code": {"type": "string"},
                    "recommended_action": {"type": "string"},
                    "action_target": {"type": "string"},
                    "restart_recommended": {"type": "boolean"}
                }
            },
            "health_summary": health_summary_output_schema(),
            "deferred_loading_supported": {"type": "boolean"},
            "default_deferred_tool_loading": {"type": "boolean"},
            "default_tools_list_contract_mode": {"type": "string"},
            "loaded_namespaces": {"type": "array", "items": {"type": "string"}},
            "loaded_tiers": {"type": "array", "items": {"type": "string"}},
            "full_tool_exposure": {"type": "boolean"},
            "deferred_namespace_gate": {"type": "boolean"},
            "deferred_tier_gate": {"type": "boolean"},
            "preferred_namespaces": {"type": "array", "items": {"type": "string"}},
            "preferred_tiers": {"type": "array", "items": {"type": "string"}},
            "trusted_client_hook": {"type": "boolean"},
            "mutation_requires_trusted_client": {"type": "boolean"},
            "mutation_preflight_required": {"type": "boolean"},
            "preflight_ttl_seconds": {"type": "integer"},
            "rename_requires_symbol_preflight": {"type": "boolean"},
            "requires_namespace_listing_before_tool_call": {"type": "boolean"},
            "requires_tier_listing_before_tool_call": {"type": "boolean"}
        }
    })
}

pub(super) fn visible_tools_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "tool_count": {"type": "integer"},
            "tool_count_total": {"type": "integer"},
            "default_listed_tool_count": {"type": "integer"},
            "default_listed_tool_names": {"type": "array", "items": {"type": "string"}},
            "tool_names": {"type": "array", "items": {"type": "string"}},
            "execution_classes": {"type": "object"},
            "all_namespaces": {"type": "array", "items": {"type": "string"}},
            "all_tiers": {"type": "array", "items": {"type": "string"}},
            "preferred_namespaces": {"type": "array", "items": {"type": "string"}},
            "preferred_tiers": {"type": "array", "items": {"type": "string"}},
            "loaded_namespaces": {"type": "array", "items": {"type": "string"}},
            "loaded_tiers": {"type": "array", "items": {"type": "string"}},
            "effective_namespaces": {"type": "array", "items": {"type": "string"}},
            "effective_tiers": {"type": "array", "items": {"type": "string"}},
            "selected_namespace": {"type": ["string", "null"]},
            "selected_tier": {"type": ["string", "null"]},
            "deferred_loading_active": {"type": "boolean"},
            "full_tool_exposure": {"type": "boolean"}
        }
    })
}

pub(super) fn routing_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "preferred_entrypoints": {"type": "array", "items": {"type": "string"}},
            "preferred_entrypoints_source": {"type": "string"},
            "agent_role": {"type": ["string", "null"]},
            "preferred_entrypoints_visible": {"type": "array", "items": {"type": "string"}},
            "preferred_entrypoints_omitted": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "tool": {"type": "string"},
                        "requested_tool": {"type": "string"},
                        "reason": {"type": "string", "enum": ["not_in_active_surface", "deferred_tool_not_loaded", "unknown_tool"]},
                        "recommended_action": {"type": "string", "enum": ["switch_tool_surface", "load_deferred_tool_namespace", "fix_preferred_entrypoint"]},
                        "execution_policy": execution_policy_output_schema(),
                        "tool_namespace": {"type": "string"},
                        "tool_loading_request": {
                            "type": "object",
                            "properties": {
                                "method": {"type": "string", "enum": ["tools/list"]},
                                "params": {
                                    "type": "object",
                                    "properties": {
                                        "namespace": {"type": "string"},
                                        "tier": {"type": "string", "enum": ["primitive", "analysis", "workflow"]}
                                    }
                                }
                            }
                        },
                        "tool_tier": {"type": "string", "enum": ["primitive", "analysis", "workflow"]},
                        "included_in": {"type": "array", "items": {"type": "string"}},
                        "recommended_profile": {"type": "string"}
                    }
                }
            },
            "preferred_entrypoints_with_policies": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "tool": {"type": "string"},
                        "execution_policy": execution_policy_output_schema()
                    }
                }
            },
            "recommended_entrypoint": {"type": ["string", "null"]},
            "recommended_entrypoint_execution_policy": {
                "anyOf": [execution_policy_output_schema(), {"type": "null"}]
            }
        }
    })
}

fn execution_policy_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "execution_class": {"type": "string", "enum": ["read", "analyze", "mutate"]},
            "risk": {"type": "string", "enum": ["low", "medium", "high"]},
            "cost_hint": {"type": "string", "enum": ["low", "medium", "high"]},
            "concurrency_safe": {"type": "boolean"}
        },
        "required": ["execution_class", "risk", "cost_hint", "concurrency_safe"]
    })
}

pub(super) fn harness_runtime_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "effort_level": {"type": "string"},
            "compression_offset": {"type": "integer"},
            "meta_max_result_size": {"type": "boolean"},
            "rapid_burst_detection": {"type": "boolean"},
            "schema_pre_validation": {"type": "boolean"},
            "doom_loop_threshold": {"type": "integer"},
            "preflight_ttl_seconds": {"type": "integer"}
        }
    })
}
