//! Output schemas for the ADR-0016 default-surface tools that the
//! `tools.toml` codegen path does not yet bind (verb facades + the
//! reviewer-graph / ci-audit profile tools + `refresh_symbol_index`).
//!
//! These are attached at registry-build time in `tool_defs::build` (the
//! post-build pass), keeping the `tools.toml` source untouched while the
//! parallel surface-listing restructure is in flight. Each schema mirrors
//! the concrete `json!` payload its handler returns — cross-checked against
//! the handler source, not inferred:
//!
//! - verb facades (`search`/`overview`/`graph`/`diagnose`/`review`):
//!   `tools/verbs.rs::run_verb` returns the resolved target tool's payload
//!   verbatim, so the shape varies by `mode`; the schema is a permissive
//!   object documenting the mode-routing contract (ADR-0016 decision 2).
//! - `refresh_symbol_index`: `tools/symbols/inventory.rs` (`IndexStats` +
//!   optional freshness/warning, or the queued-job envelope).
//! - `get_complexity`: `tools/symbols/inventory.rs`.
//! - `get_symbol_importance`: `tools/graph.rs`.
//! - `audit_log_query` / `audit_tool_surface_consistency` /
//!   `find_phantom_modules` / `find_redundant_definitions` /
//!   `find_over_visible_apis`: `tools/admin/mod.rs`.
//! - `classify_symbol` (semantic feature only): `dispatch/semantic/analysis.rs`.

use serde_json::json;

/// Shared schema for the mode-routed verb facades. The concrete payload is
/// whatever the resolved target tool returns for the requested `mode`
/// (see `tools/verbs.rs` mode→target tables), so the shape varies by mode;
/// the schema stays a permissive structured object per ADR-0016 decision 2.
pub(crate) fn verb_facade_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "description": "Mode-routed verb facade (ADR-0016 decision 2). The payload matches the resolved target tool's output schema for the requested `mode` and therefore varies by mode; see crates/codelens-mcp/src/tools/verbs.rs for the mode→target mapping."
    })
}

pub(crate) fn refresh_symbol_index_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "indexed_files": {"type": "integer"},
            "supported_files": {"type": "integer"},
            "stale_files": {"type": "integer"},
            "embedding_freshness": {"type": "object"},
            "warning": {
                "type": "object",
                "properties": {
                    "code": {"type": "string"},
                    "message": {"type": "string"}
                }
            },
            "background": {"type": "boolean"},
            "status": {"type": "string"},
            "job": {"type": "object"},
            "poll": {
                "type": "object",
                "properties": {
                    "tool": {"type": "string"},
                    "arguments": {"type": "object"}
                }
            }
        }
    })
}

pub(crate) fn get_complexity_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string"},
            "functions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "kind": {"type": "string"},
                        "file": {"type": "string"},
                        "line": {"type": "integer"},
                        "branches": {"type": "integer"},
                        "complexity": {"type": "integer"}
                    }
                }
            },
            "count": {"type": "integer"},
            "avg_complexity": {"type": "number"}
        }
    })
}

pub(crate) fn get_symbol_importance_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "ranking": {"type": "array", "items": {"type": "object"}},
            "count": {"type": "integer"}
        }
    })
}

pub(crate) fn audit_log_query_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "sink_available": {"type": "boolean"},
            "rows": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "operation_id": {"type": ["string", "null"]},
                        "transaction_id": {"type": ["string", "null"]},
                        "timestamp_ms": {"type": "integer"},
                        "principal": {"type": ["string", "null"]},
                        "tool": {"type": "string"},
                        "args_hash": {"type": ["string", "null"]},
                        "apply_status": {"type": ["string", "null"]},
                        "state_from": {"type": ["string", "null"]},
                        "state_to": {"type": ["string", "null"]},
                        "evidence_hash": {"type": ["string", "null"]},
                        "rollback_restored": {"type": ["boolean", "null"]},
                        "error_message": {"type": ["string", "null"]}
                    }
                }
            },
            "filters": {
                "type": "object",
                "properties": {
                    "operation_id": {"type": ["string", "null"]},
                    "since_ms": {"type": ["integer", "null"]},
                    "limit": {"type": "integer"}
                }
            }
        }
    })
}

pub(crate) fn audit_tool_surface_consistency_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "all_clean": {"type": "boolean"},
            "violation_count": {"type": "integer"},
            "layers_checked": {"type": "array", "items": {"type": "string"}},
            "summary": {"type": "object"},
            "violations": {
                "type": "object",
                "properties": {
                    "missing_in_dispatch": {"type": "array", "items": {"type": "string"}},
                    "missing_in_toml": {"type": "array", "items": {"type": "string"}},
                    "orphan_in_preset": {"type": "array", "items": {"type": "string"}},
                    "tombstone_reintroduced": {"type": "array", "items": {"type": "string"}}
                }
            },
            "surface_drift": {"type": "object"},
            "intentional_deprecation": {"type": "array", "items": {"type": "string"}},
            "intentional_feature_gated": {"type": "array", "items": {"type": "string"}},
            "pending_d3_allowlisted": {"type": "array", "items": {"type": "string"}},
            "pending_d3_symbolic_edit_core": {"type": "array", "items": {"type": "string"}},
            "pending_d3_refactor_substrate": {"type": "array", "items": {"type": "string"}},
            "next_actions": {"type": "array", "items": {"type": "string"}}
        }
    })
}

pub(crate) fn find_phantom_modules_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "phantom_modules": {"type": "array", "items": {"type": "object"}},
            "count": {"type": "integer"},
            "max_results": {"type": "integer"},
            "truncated": {"type": "boolean"},
            "next_actions": {"type": "array", "items": {"type": "string"}}
        }
    })
}

pub(crate) fn find_redundant_definitions_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "redundant_definitions": {"type": "array", "items": {"type": "object"}},
            "count": {"type": "integer"},
            "max_results": {"type": "integer"},
            "truncated": {"type": "boolean"},
            "next_actions": {"type": "array", "items": {"type": "string"}}
        }
    })
}

pub(crate) fn find_over_visible_apis_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "violations": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "surface": {"type": "string"},
                        "tool": {"type": "string"},
                        "reasons": {"type": "array", "items": {"type": "string"}},
                        "destructive_hint": {"type": ["boolean", "null"]},
                        "approval_required": {"type": ["boolean", "null"]},
                        "audit_category": {"type": ["string", "null"]}
                    }
                }
            },
            "violation_count": {"type": "integer"},
            "all_clean": {"type": "boolean"},
            "readonly_surfaces_checked": {"type": "array", "items": {"type": "string"}},
            "policy": {"type": "object"},
            "next_actions": {"type": "array", "items": {"type": "string"}}
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
            "classifications": {}
        }
    })
}

/// Registry-build attachment map: the ADR-0016 default-surface tools whose
/// `outputSchema` is supplied here (not via `tools.toml` codegen). Consumed
/// by `tool_defs::build` when a generated tool has no schema of its own.
/// Returns `None` for every other tool so the codegen-sourced schemas win.
pub(crate) fn supplemental_output_schema(name: &str) -> Option<serde_json::Value> {
    let schema = match name {
        "search" | "overview" | "graph" | "diagnose" | "review" => verb_facade_output_schema(),
        "refresh_symbol_index" => refresh_symbol_index_output_schema(),
        "get_complexity" => get_complexity_output_schema(),
        "get_symbol_importance" => get_symbol_importance_output_schema(),
        "audit_log_query" => audit_log_query_output_schema(),
        "audit_tool_surface_consistency" => audit_tool_surface_consistency_output_schema(),
        "find_phantom_modules" => find_phantom_modules_output_schema(),
        "find_redundant_definitions" => find_redundant_definitions_output_schema(),
        "find_over_visible_apis" => find_over_visible_apis_output_schema(),
        #[cfg(feature = "semantic")]
        "classify_symbol" => classify_symbol_output_schema(),
        _ => return None,
    };
    Some(schema)
}
