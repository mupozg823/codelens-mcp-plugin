use serde_json::json;

pub(crate) fn analysis_handle_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "analysis_id": {"type": "string"},
            "summary": {"type": "string"},
            "top_findings": {"type": "array", "items": {"type": "string"}},
            "risk_level": {"type": "string", "enum": ["low", "medium", "high"]},
            "confidence": {"type": "number"},
            "next_actions": {"type": "array", "items": {"type": "string"}},
            "blockers": {"type": "array", "items": {"type": "string"}},
            "blocker_count": {"type": "integer"},
            "readiness": {
                "type": "object",
                "properties": {
                    "diagnostics_ready": {"type": "string", "enum": ["ready", "caution", "blocked"]},
                    "reference_safety": {"type": "string", "enum": ["ready", "caution", "blocked"]},
                    "test_readiness": {"type": "string", "enum": ["ready", "caution", "blocked"]},
                    "mutation_ready": {"type": "string", "enum": ["ready", "caution", "blocked"]}
                }
            },
            "readiness_score": {"type": "number", "minimum": 0.0, "maximum": 1.0, "description": "Aggregate readiness score: ready=1.0, caution=0.5, blocked=0.0, averaged across 4 dimensions"},
            "verifier_checks": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "check": {"type": "string"},
                        "status": {"type": "string", "enum": ["ready", "caution", "blocked"]},
                        "summary": {"type": "string"},
                        "evidence_section": {"type": ["string", "null"]}
                    }
                }
            },
            "quality_focus": {"type": "array", "items": {"type": "string"}},
            "recommended_checks": {"type": "array", "items": {"type": "string"}},
            "performance_watchpoints": {"type": "array", "items": {"type": "string"}},
            "available_sections": {"type": "array", "items": {"type": "string"}},
            "summary_resource": {
                "type": "object",
                "properties": {
                    "uri": {"type": "string"}
                }
            },
            "section_handles": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "section": {"type": "string"},
                        "uri": {"type": "string"}
                    }
                }
            },
            "reused": {"type": "boolean"},
            "schema_version": {"type": "string"},
            "report_kind": {"type": "string"},
            "profile": {"type": "string"},
            "machine_summary": {
                "type": "object",
                "properties": {
                    "finding_count": {"type": "integer"},
                    "next_action_count": {"type": "integer"},
                    "section_count": {"type": "integer"},
                    "blocker_count": {"type": "integer"},
                    "verifier_check_count": {"type": "integer"},
                    "ready_check_count": {"type": "integer"},
                    "blocked_check_count": {"type": "integer"},
                    "quality_focus_count": {"type": "integer"},
                    "recommended_check_count": {"type": "integer"},
                    "performance_watchpoint_count": {"type": "integer"}
                }
            },
            "evidence_handles": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "section": {"type": "string"},
                        "uri": {"type": "string"}
                    }
                }
            }
        }
    })
}

pub(crate) fn workflow_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "workflow": {"type": "string"},
            "delegated_tool": {"type": "string"},
            "result": {}
        }
    })
}

pub(crate) fn analysis_section_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "analysis_id": {"type": "string"},
            "section": {"type": "string"},
            "content": {}
        }
    })
}

pub(crate) fn analysis_job_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "job_id": {"type": "string"},
            "kind": {"type": "string"},
            "status": {"type": "string"},
            "progress": {"type": "integer"},
            "current_step": {"type": ["string", "null"]},
            "profile_hint": {"type": ["string", "null"]},
            "analysis_id": {"type": ["string", "null"]},
            "estimated_sections": {"type": "array", "items": {"type": "string"}},
            "summary_resource": {
                "type": ["object", "null"],
                "properties": {
                    "uri": {"type": "string"}
                }
            },
            "section_handles": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "section": {"type": "string"},
                        "uri": {"type": "string"}
                    }
                }
            },
            "error": {"type": ["string", "null"]},
            "updated_at_ms": {"type": "integer"}
        }
    })
}

pub(crate) fn analysis_job_list_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "jobs": {
                "type": "array",
                "items": analysis_job_output_schema()
            },
            "count": {"type": "integer"},
            "active_count": {"type": "integer"},
            "status_counts": {
                "type": "object",
                "additionalProperties": {"type": "integer"}
            }
        }
    })
}

pub(crate) fn analysis_artifact_list_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "artifacts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "analysis_id": {"type": "string"},
                        "tool_name": {"type": "string"},
                        "summary": {"type": "string"},
                        "created_at_ms": {"type": "integer"},
                        "surface": {"type": "string"},
                        "summary_resource": {
                            "type": "object",
                            "properties": {
                                "uri": {"type": "string"}
                            }
                        }
                    }
                }
            },
            "count": {"type": "integer"},
            "latest_created_at_ms": {"type": "integer"},
            "tool_counts": {
                "type": "object",
                "additionalProperties": {"type": "integer"}
            }
        }
    })
}
