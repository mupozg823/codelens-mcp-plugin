//! Output schema definitions for MCP tools.

use serde_json::json;

pub(super) fn symbol_output_schema() -> serde_json::Value {
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
            "body_preview": {"type": "boolean"}
        }
    })
}

pub(super) fn ranked_context_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string"},
            "symbols": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "kind": {"type": "string"},
                        "file": {"type": "string"},
                        "line": {"type": "integer"},
                        "signature": {"type": "string"},
                        "body": {"type": ["string", "null"]},
                        "relevance_score": {"type": "integer"},
                        "provenance": {
                            "type": "object",
                            "properties": {
                                "source": {
                                    "type": "string",
                                    "enum": ["structural", "semantic_boosted", "semantic_added"]
                                },
                                "structural_candidate": {"type": "boolean"},
                                "semantic_score": {"type": ["number", "null"]}
                            }
                        }
                    }
                }
            },
            "count": {"type": "integer"},
            "token_budget": {"type": "integer"},
            "chars_used": {"type": "integer"},
            "retrieval": {
                "type": "object",
                "properties": {
                    "semantic_enabled": {"type": "boolean"},
                    "semantic_used_in_core": {"type": "boolean"},
                    "lexical_query": {"type": "string"},
                    "semantic_query": {"type": "string"}
                }
            },
            "semantic_evidence": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "symbol": {"type": "string"},
                        "file": {"type": "string"},
                        "score": {"type": "number"},
                        "selected": {"type": "boolean"},
                        "final_rank": {"type": ["integer", "null"]}
                    }
                }
            }
        }
    })
}

pub(super) fn semantic_search_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string"},
            "results": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "symbol_name": {"type": "string"},
                        "kind": {"type": "string"},
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "signature": {"type": "string"},
                        "name_path": {"type": "string"},
                        "score": {"type": "number"},
                        "provenance": {
                            "type": "object",
                            "properties": {
                                "source": {"type": "string", "enum": ["semantic"]},
                                "retrieval_rank": {"type": "integer"}
                            }
                        }
                    }
                }
            },
            "count": {"type": "integer"},
            "retrieval": {
                "type": "object",
                "properties": {
                    "semantic_enabled": {"type": "boolean"},
                    "requested_query": {"type": "string"},
                    "semantic_query": {"type": "string"}
                }
            }
        }
    })
}

pub(super) fn references_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "references": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "column": {"type": "integer"},
                        "line_content": {"type": "string"},
                        "is_declaration": {"type": "boolean"},
                        "enclosing_symbol": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"},
            "returned_count": {"type": "integer"},
            "sampled": {"type": "boolean"},
            "include_context": {"type": "boolean"},
            "backend": {"type": "string"}
        }
    })
}

pub(super) fn impact_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "file": {"type": "string"},
            "symbols": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "kind": {"type": "string"},
                        "line": {"type": "integer"}
                    }
                }
            },
            "symbol_count": {"type": "integer"},
            "direct_importers": {"type": "array", "items": {"type": "string"}},
            "total_affected_files": {"type": "integer"},
            "blast_radius": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file": {"type": "string"},
                        "depth": {"type": "integer"},
                        "symbol_count": {"type": "integer"}
                    }
                }
            }
        }
    })
}

pub(super) fn diagnostics_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "diagnostics": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file": {"type": "string"},
                        "line": {"type": "integer"},
                        "severity": {"type": "string"},
                        "message": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"}
        }
    })
}

pub(super) fn rename_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "success": {"type": "boolean"},
            "modified_files": {"type": "integer"},
            "total_replacements": {"type": "integer"},
            "edits": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "old_text": {"type": "string"},
                        "new_text": {"type": "string"}
                    }
                }
            }
        }
    })
}

pub(super) fn file_content_output_schema() -> serde_json::Value {
    json!({"type":"object","properties":{"content":{"type":"string"}}})
}

pub(super) fn changed_files_output_schema() -> serde_json::Value {
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

pub(super) fn onboard_output_schema() -> serde_json::Value {
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

pub(super) fn prune_index_failures_output_schema() -> serde_json::Value {
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

pub(super) fn memory_list_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "memories": {"type": "array", "items": {"type": "string"}},
            "count": {"type": "integer"}
        }
    })
}

pub(super) fn analysis_handle_output_schema() -> serde_json::Value {
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

pub(super) fn analysis_section_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "analysis_id": {"type": "string"},
            "section": {"type": "string"},
            "content": {}
        }
    })
}

pub(super) fn analysis_job_output_schema() -> serde_json::Value {
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
            "error": {"type": ["string", "null"]},
            "updated_at_ms": {"type": "integer"}
        }
    })
}
