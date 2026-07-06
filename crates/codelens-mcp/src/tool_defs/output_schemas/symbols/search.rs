use super::evidence::evidence_output_schema;
use serde_json::json;

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
            "cache_hit_tier": {"type": "string", "enum": ["disabled", "cold", "exact"]},
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
                    "semantic_query": {"type": "string"},
                    "cache_hit_tier": {"type": ["string", "null"], "enum": ["disabled", "cold", "exact", null]},
                    "query_cache": {
                        "type": "object",
                        "properties": {
                            "enabled": {"type": "boolean"},
                            "used": {"type": "boolean"},
                            "cache_hit_tier": {"type": ["string", "null"], "enum": ["disabled", "cold", "exact", null]},
                            "source": {"type": "string"}
                        }
                    }
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
