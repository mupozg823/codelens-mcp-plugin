//! Output schemas for symbol navigation and code-graph tools.

use serde_json::json;

fn evidence_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "schema_version": {"type": "string", "enum": ["codelens-evidence-v1"]},
            "domain": {"type": "string", "enum": ["call_graph", "retrieval", "symbol", "references"]},
            "active_backend": {
                "type": "string",
                "enum": ["tree-sitter", "hybrid", "semantic", "sqlite", "scip", "lsp"]
            },
            "confidence": {"type": "number"},
            "confidence_basis": {"type": "string"},
            "degraded_reason": {"type": ["string", "null"]},
            "signals": {"type": "object"}
        }
    })
}

pub(crate) fn symbol_output_schema() -> serde_json::Value {
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
            "body_preview": {"type": "boolean"},
            "evidence": evidence_output_schema()
        }
    })
}

pub(crate) fn ranked_context_output_schema() -> serde_json::Value {
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
                                    "enum": ["structural", "semantic_boosted", "semantic_added", "sparse_boosted", "sparse_added"]
                                },
                                "structural_candidate": {"type": "boolean"},
                                "semantic_score": {"type": ["number", "null"]},
                                "sparse_score": {"type": ["number", "null"]}
                            }
                        }
                    }
                }
            },
            "count": {"type": "integer"},
            "token_budget": {"type": "integer"},
            "chars_used": {"type": "integer"},
            "evidence": evidence_output_schema(),
            "retrieval": {
                "type": "object",
                "properties": {
                "semantic_enabled": {"type": "boolean"},
                "semantic_used_in_core": {"type": "boolean"},
                "sparse_used_in_core": {"type": "boolean"},
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
            },
            "sparse_evidence": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "symbol": {"type": "string"},
                        "file": {"type": "string"},
                        "score": {"type": "number"},
                        "matched_terms": {
                            "type": "array",
                            "items": {"type": "string"}
                        },
                        "selected": {"type": "boolean"},
                        "final_rank": {"type": ["integer", "null"]}
                    }
                }
            }
        }
    })
}

pub(crate) fn bm25_symbol_search_output_schema() -> serde_json::Value {
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
            "evidence": evidence_output_schema(),
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
pub(crate) fn semantic_search_output_schema() -> serde_json::Value {
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

pub(crate) fn references_output_schema() -> serde_json::Value {
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
            "backend": {"type": "string"},
            "evidence": evidence_output_schema()
        }
    })
}

pub(crate) fn resolve_symbol_target_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "success": {"type": "boolean"},
            "semantic_backend": {"type": "string"},
            "edit_authority": {"type": "object"},
            "targets": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "column": {"type": "integer"},
                        "end_line": {"type": "integer"},
                        "end_column": {"type": "integer"},
                        "target": {"type": "string"},
                        "method": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"}
        }
    })
}

fn resolution_summary_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "same_file": {"type": "integer"},
            "import_map": {"type": "integer"},
            "import_suffix": {"type": "integer"},
            "unique_name": {"type": "integer"},
            "path_proximity": {"type": "integer"},
            "unresolved": {"type": "integer"}
        }
    })
}

pub(crate) fn get_callers_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "function": {"type": "string"},
            "callers": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file": {"type": "string"},
                        "function": {"type": "string"},
                        "line": {"type": "integer"},
                        "confidence": {"type": "number"},
                        "resolution": {"type": ["string", "null"]}
                    }
                }
            },
            "count": {"type": "integer"},
            "confidence_basis": {
                "type": "string",
                "enum": [
                    "import_evidence",
                    "same_file_only",
                    "name_only_unique",
                    "mixed_with_fallback",
                    "fallback_only",
                    "unresolved_only"
                ]
            },
            "resolution_summary": resolution_summary_output_schema(),
            "evidence": evidence_output_schema()
        }
    })
}

pub(crate) fn get_callees_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "function": {"type": "string"},
            "callees": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "line": {"type": "integer"},
                        "resolved_file": {"type": ["string", "null"]},
                        "confidence": {"type": "number"},
                        "resolution": {"type": ["string", "null"]}
                    }
                }
            },
            "count": {"type": "integer"},
            "confidence_basis": {
                "type": "string",
                "enum": [
                    "import_evidence",
                    "same_file_only",
                    "name_only_unique",
                    "mixed_with_fallback",
                    "fallback_only",
                    "unresolved_only"
                ]
            },
            "resolution_summary": resolution_summary_output_schema(),
            "evidence": evidence_output_schema()
        }
    })
}

pub(crate) fn diagnostics_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "diagnostics": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "column": {"type": "integer"},
                        "end_line": {"type": "integer"},
                        "end_column": {"type": "integer"},
                        "severity": {"type": ["integer", "null"]},
                        "severity_label": {"type": ["string", "null"]},
                        "code": {"type": ["string", "null"]},
                        "source": {"type": ["string", "null"]},
                        "message": {"type": "string"},
                        "classification": {"type": "string"},
                        "actionability": {"type": "string"},
                        "recommended_action": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"},
            "backend": {"type": "string", "enum": ["lsp", "scip"]},
            "suppressed_diagnostics_count": {"type": "integer"},
            "suppressed_diagnostics": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "column": {"type": "integer"},
                        "code": {"type": ["string", "null"]},
                        "source": {"type": ["string", "null"]},
                        "message": {"type": "string"},
                        "suppression": {"type": "string"}
                    }
                }
            }
        }
    })
}

/// D1 (#346 Phase 4): shared shape for `find_declaration` /
/// `find_implementations` — LSP-resolved locations plus the graceful
/// degradation contract (`degraded_reason` + `fallback_hint`).
pub(crate) fn lsp_navigation_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "success": {"type": "boolean"},
            "operation": {"type": "string", "enum": ["declaration", "implementation"]},
            "backend": {"type": "string", "enum": ["lsp"]},
            "symbol_name": {"type": ["string", "null"]},
            "language": {"type": "string"},
            "position": {
                "type": "object",
                "properties": {
                    "file_path": {"type": "string"},
                    "line": {"type": "integer"},
                    "column": {"type": "integer"}
                }
            },
            "targets": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "column": {"type": "integer"},
                        "end_line": {"type": "integer"},
                        "end_column": {"type": "integer"},
                        "target": {"type": "string"},
                        "method": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"},
            "degraded_reason": {"type": "string"},
            "fallback_hint": {"type": "array", "items": {"type": "string"}}
        }
    })
}

/// D1 (#346 Phase 4): `get_diagnostics_for_symbol` — the file
/// diagnostics shape filtered to one symbol's span.
pub(crate) fn symbol_diagnostics_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "success": {"type": "boolean"},
            "symbol": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "kind": {"type": "string"},
                    "file_path": {"type": "string"},
                    "span": {
                        "type": "object",
                        "properties": {
                            "start_line": {"type": "integer"},
                            "end_line": {"type": "integer"}
                        }
                    }
                }
            },
            "diagnostics": {"type": "array", "items": {"type": "object"}},
            "count": {"type": "integer"},
            "file_diagnostics_count": {"type": "integer"},
            "backend": {"type": ["string", "null"]},
            "degraded_reason": {"type": "string"},
            "fallback_hint": {"type": "array", "items": {"type": "string"}}
        }
    })
}
