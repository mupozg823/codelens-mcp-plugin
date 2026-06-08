//! Output schemas for file, session, agent, and shared tools.

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
            "token_bill": {"type": "object"},
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

pub(crate) fn memory_list_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "memories": {"type": "array", "items": {"type": "string"}},
            "count": {"type": "integer"}
        }
    })
}

pub fn register_agent_work_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "entry": {"type": "object"},
            "note": {"type": "string"}
        }
    })
}

pub fn list_active_agents_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "agents": {"type": "array"},
            "count": {"type": "integer"}
        }
    })
}

pub fn claim_files_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "claim": {"type": "object"},
            "overlapping_claims": {"type": "array"},
            "note": {"type": "string"}
        }
    })
}

pub fn release_files_output_schema() -> serde_json::Value {
    serde_json::json!({
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
