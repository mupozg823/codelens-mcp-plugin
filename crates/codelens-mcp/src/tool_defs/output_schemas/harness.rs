//! Output schemas for harness capabilities, configuration, and session tools.

use serde_json::json;
use super::jobs::activate_project_output_schema;

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

pub(crate) fn prepare_harness_session_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "activated": {"type": "boolean"},
            "project": activate_project_output_schema(),
            "active_surface": {"type": "string"},
            "token_budget": {"type": "integer"},
            "surface_generation": surface_generation_output_schema(),
            "config": {
                "type": "object",
                "properties": {
                    "runtime": {"type": "string"},
                    "project_root": {"type": "string"},
                    "surface": {"type": "string"},
                    "token_budget": {"type": "integer"},
                    "tool_count": {"type": "integer"},
                    "client_profile": {"type": "string"}
                }
            },
            "index_recovery": {
                "type": "object",
                "properties": {
                    "enabled": {"type": "boolean"},
                    "threshold": {"type": "integer"},
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
            },
            "capabilities": get_capabilities_output_schema(),
            "health_summary": health_summary_output_schema(),
            "warnings": {
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
            },
            "overlay": {
                "type": "object",
                "properties": {
                    "applied": {"type": "boolean"},
                    "host_context": {"type": ["string", "null"]},
                    "task_overlay": {"type": ["string", "null"]},
                    "preferred_executor_bias": {"type": ["string", "null"]},
                    "preferred_entrypoints": {"type": "array", "items": {"type": "string"}},
                    "preferred_entrypoints_visible": {"type": "array", "items": {"type": "string"}},
                    "emphasized_tools": {"type": "array", "items": {"type": "string"}},
                    "emphasized_tools_visible": {"type": "array", "items": {"type": "string"}},
                    "avoid_tools": {"type": "array", "items": {"type": "string"}},
                    "avoid_tools_visible": {"type": "array", "items": {"type": "string"}},
                    "routing_notes": {"type": "array", "items": {"type": "string"}}
                }
            },
            "coordination": {
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
            },
            "http_session": {
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
            },
            "visible_tools": {
                "type": "object",
                "properties": {
                    "tool_count": {"type": "integer"},
                    "tool_count_total": {"type": "integer"},
                    "tool_names": {"type": "array", "items": {"type": "string"}},
                    "preferred_executors": {"type": "object"},
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
            },
            "routing": {
                "type": "object",
                "properties": {
                    "preferred_entrypoints": {"type": "array", "items": {"type": "string"}},
                    "preferred_entrypoints_source": {"type": "string"},
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
                                "preferred_executor": {"type": "string"},
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
                    "preferred_entrypoints_with_executors": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "tool": {"type": "string"},
                                "preferred_executor": {"type": "string"}
                            }
                        }
                    },
                    "recommended_entrypoint": {"type": ["string", "null"]},
                    "recommended_entrypoint_preferred_executor": {"type": ["string", "null"]}
                }
            },
            "harness": {
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
