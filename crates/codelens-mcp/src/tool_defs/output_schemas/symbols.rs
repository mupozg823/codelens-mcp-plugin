use serde_json::json;

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
            "body_delivery": {
                "type": "object",
                "description": "Per-call body transparency. `status` is disabled|full|partial|dropped|truncated so the harness knows whether a follow-up read is needed before trusting the symbol body field.",
                "properties": {
                    "requested": {"type": "boolean"},
                    "status": {"type": "string", "enum": ["disabled","full","partial","dropped","truncated"]},
                    "bodies_full": {"type": "integer"},
                    "bodies_truncated": {"type": "integer"},
                    "bodies_omitted_over_cap": {"type": "integer"},
                    "max_symbols_with_body": {"type": "integer"},
                    "line_limit": {"type": "integer"},
                    "char_limit": {"type": "integer"},
                    "hint": {"type": "string"}
                }
            }
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
            "backend": {"type": "string"}
        }
    })
}

pub(crate) fn impact_output_schema() -> serde_json::Value {
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

pub(crate) fn diagnostics_output_schema() -> serde_json::Value {
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

pub(crate) fn rename_output_schema() -> serde_json::Value {
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

#[cfg(feature = "semantic")]
pub(crate) fn find_similar_code_output_schema() -> serde_json::Value {
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
pub(crate) fn find_code_duplicates_output_schema() -> serde_json::Value {
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
pub(crate) fn classify_symbol_output_schema() -> serde_json::Value {
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
pub(crate) fn find_misplaced_code_output_schema() -> serde_json::Value {
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
