//! Output schema definitions for MCP tools.

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
                "preferred_lane": {"type": "string"},
                "sparse_lane_recommended": {"type": "boolean"},
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

pub(super) fn bm25_symbol_search_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string"},
            "results": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "symbol_id": {"type": "string"},
                        "name": {"type": "string"},
                        "name_path": {"type": "string"},
                        "kind": {"type": "string"},
                        "file_path": {"type": "string"},
                        "module_path": {"type": "string"},
                        "signature": {"type": "string"},
                        "language": {"type": "string"},
                        "line": {"type": "integer"},
                        "score": {"type": "number"},
                        "why_matched": {
                            "type": "array",
                            "items": {"type": "string"}
                        },
                        "flags": {
                            "type": "object",
                            "properties": {
                                "is_test": {"type": "boolean"},
                                "is_generated": {"type": "boolean"},
                                "exported": {"type": "boolean"}
                            }
                        },
                        "provenance": {
                            "type": "object",
                            "properties": {
                                "source": {"type": "string", "enum": ["sparse_bm25f"]},
                                "retrieval_rank": {"type": "integer"}
                            }
                        },
                        "suggested_follow_up": {
                            "type": "array",
                            "items": {"type": "string"}
                        },
                        "confidence": {
                            "type": "string",
                            "enum": ["high", "medium", "low"]
                        }
                    }
                }
            },
            "count": {"type": "integer"},
            "retrieval": {
                "type": "object",
                "properties": {
                    "lane": {"type": "string"},
                    "query_type": {"type": "string"},
                    "recommended": {"type": "boolean"},
                    "lexical_query": {"type": "string"},
                    "semantic_query": {"type": "string"}
                }
            }
        }
    })
}

#[cfg(feature = "semantic")]
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
                                "retrieval_rank": {"type": "integer"},
                                "prior_delta": {"type": "number"},
                                "adjusted_score": {"type": "number"}
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

pub(super) fn watch_status_output_schema() -> serde_json::Value {
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

pub(super) fn tool_metrics_output_schema() -> serde_json::Value {
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

pub(super) fn builder_session_audit_output_schema() -> serde_json::Value {
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

pub(super) fn planner_session_audit_output_schema() -> serde_json::Value {
    builder_session_audit_output_schema()
}

pub(super) fn session_markdown_output_schema() -> serde_json::Value {
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

pub(super) fn workflow_alias_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "workflow": {"type": "string"},
            "delegated_tool": {"type": "string"},
            "deprecated": {"type": "boolean"},
            "replacement_tool": {"type": ["string", "null"]},
            "removal_target": {"type": ["string", "null"]},
            "result": {}
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

pub(super) fn analysis_job_list_output_schema() -> serde_json::Value {
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

pub(super) fn analysis_artifact_list_output_schema() -> serde_json::Value {
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

pub(super) fn replace_content_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "content": {"type": "string", "description": "Full updated file content after replacement"},
            "replacements": {"type": "integer", "description": "Number of replacements performed (text mode only)"}
        }
    })
}

pub(super) fn create_text_file_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "created": {"type": "string", "description": "Relative path of the newly created file"}
        }
    })
}

pub(super) fn add_import_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "success": {"type": "boolean"},
            "file_path": {"type": "string"},
            "content_length": {"type": "integer", "description": "Byte length of the updated file content"}
        }
    })
}

#[cfg(feature = "semantic")]
pub(super) fn find_similar_code_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "query_symbol": {"type": "string"},
            "file": {"type": "string"},
            "min_similarity": {"type": "number"},
            "similar": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "symbol_name": {"type": "string"},
                        "kind": {"type": "string"},
                        "line": {"type": "integer"},
                        "signature": {"type": "string"},
                        "name_path": {"type": "string"},
                        "score": {"type": "number", "description": "Cosine similarity score 0.0-1.0"}
                    }
                }
            },
            "count": {"type": "integer"}
        }
    })
}

#[cfg(feature = "semantic")]
pub(super) fn find_code_duplicates_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "threshold": {"type": "number"},
            "duplicates": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "symbol_a": {"type": "string"},
                        "symbol_b": {"type": "string"},
                        "file_a": {"type": "string"},
                        "file_b": {"type": "string"},
                        "line_a": {"type": "integer"},
                        "line_b": {"type": "integer"},
                        "similarity": {"type": "number"}
                    }
                }
            },
            "count": {"type": "integer"}
        }
    })
}

#[cfg(feature = "semantic")]
pub(super) fn classify_symbol_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "symbol": {"type": "string"},
            "file": {"type": "string"},
            "classifications": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "category": {"type": "string"},
                        "score": {"type": "number", "description": "Zero-shot cosine similarity score"}
                    }
                }
            }
        }
    })
}

#[cfg(feature = "semantic")]
pub(super) fn find_misplaced_code_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "outliers": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "symbol_name": {"type": "string"},
                        "kind": {"type": "string"},
                        "line": {"type": "integer"},
                        "avg_similarity_to_file": {"type": "number", "description": "Lower values indicate stronger semantic outliers"}
                    }
                }
            },
            "count": {"type": "integer"}
        }
    })
}

// ── Group A: Agent high-frequency tools (v1.7 schema expansion) ────

pub(super) fn activate_project_output_schema() -> serde_json::Value {
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

pub(super) fn prepare_harness_session_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "activated": {"type": "boolean"},
            "project": activate_project_output_schema(),
            "active_surface": {"type": "string"},
            "token_budget": {"type": "integer"},
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

pub(super) fn get_capabilities_output_schema() -> serde_json::Value {
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
            }
        }
    })
}

pub(super) fn get_current_config_output_schema() -> serde_json::Value {
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

pub(super) fn search_for_pattern_output_schema() -> serde_json::Value {
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

pub(super) fn find_annotations_output_schema() -> serde_json::Value {
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

pub(super) fn find_tests_output_schema() -> serde_json::Value {
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

pub(super) fn get_project_structure_output_schema() -> serde_json::Value {
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

pub(super) fn get_type_hierarchy_output_schema() -> serde_json::Value {
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
