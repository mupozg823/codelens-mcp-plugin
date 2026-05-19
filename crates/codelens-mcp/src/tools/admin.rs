//! ADR-0009 §2 + §6: Admin tools.
//!
//! Exposes two admin-tier tools:
//! - `audit_log_query` — read-only window into the `audit_log.sqlite`
//!   rows written by every mutation call.
//! - `audit_tool_surface_consistency` — cross-layer drift detector
//!   (P1-4 Sprint A, 2026-05-18). Originally shipped in v1.13.9
//!   (#155), dropped in the v1.13.27 surface trim (#292/dce98b7a),
//!   resurrected here as a runtime check (no source-file regex).
//!
//! Both require `Admin` role (see `crate::principals::required_role_for`).

use super::{AppState, ToolResult, optional_string, optional_usize, success_meta};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Query the durable audit log.
///
/// Filters:
/// - `transaction_id` — narrow to one mutation call
/// - `since_ms` — earliest `timestamp_ms` to include (epoch millis)
/// - `limit` — max rows to return (default 100)
///
/// When the audit sink is unavailable (e.g. SQLite open failed at
/// startup) the tool returns an empty `rows` array and a
/// `sink_available=false` flag rather than `Err` — operators inspecting
/// the audit trail need to be able to ask the question even when the
/// sink itself is the broken thing.
pub fn audit_log_query(state: &AppState, arguments: &Value) -> ToolResult {
    let transaction_id = optional_string(arguments, "transaction_id");
    let since_ms = arguments.get("since_ms").and_then(|v| v.as_i64());
    let limit = optional_usize(arguments, "limit", 100).clamp(1, 1000);

    let Some(sink) = state.audit_sink() else {
        return Ok((
            json!({
                "sink_available": false,
                "rows": [],
                "filters": {
                    "transaction_id": transaction_id,
                    "since_ms": since_ms,
                    "limit": limit,
                },
            }),
            success_meta(BackendKind::Config, 1.0),
        ));
    };

    let rows = sink
        .query(transaction_id, since_ms, limit)
        .map_err(|error| CodeLensError::Internal(error.context("audit_log_query")))?;

    let serialised: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "transaction_id": r.transaction_id,
                "timestamp_ms": r.timestamp_ms,
                "principal": r.principal,
                "tool": r.tool,
                "args_hash": r.args_hash,
                "apply_status": r.apply_status,
                "state_from": r.state_from,
                "state_to": r.state_to,
                "evidence_hash": r.evidence_hash,
                "rollback_restored": r.rollback_restored,
                "error_message": r.error_message,
            })
        })
        .collect();

    Ok((
        json!({
            "sink_available": true,
            "rows": serialised,
            "filters": {
                "transaction_id": transaction_id,
                "since_ms": since_ms,
                "limit": limit,
            },
        }),
        success_meta(BackendKind::Config, 1.0),
    ))
}

/// Cross-layer drift detector for the tool surface (P1-4 Sprint A).
///
/// Compares three runtime sources of truth plus the deprecation
/// allowlist to surface only the violations that matter:
///
/// - **Layer 1: `tools.toml`** — surfaced via the `Tool` registry that
///   `scripts/regen-tool-defs.py` emits into
///   `tool_defs/generated/build_generated.rs`. Reached here through
///   `tool_defs::visible_tools(ToolSurface::Preset(ToolPreset::Full))`,
///   which is the closest thing to "every registered tool" in-process.
/// - **Layer 3: `dispatch_table()`** — the runtime HashMap built by
///   `tools::dispatch_table()`. If a name appears here but not in
///   Layer 1, the handler exists but the JSON-RPC schema validator
///   will reject the call (the missing schema makes it look unknown).
///   If it appears in Layer 1 but not here, the schema validator will
///   accept it, then dispatch will 404 with `Unknown tool`.
/// - **Layer 4: `presets.{MINIMAL,PLANNER_READONLY,BUILDER_MINIMAL,REVIEWER_GRAPH}_TOOLS`**
///   — the static preset whitelists in `tool_defs/presets.rs`.
///   A name listed here but missing from Layer 1 is an orphan: the
///   preset references a tool that no longer exists, and
///   `regen-tool-defs.py --check` warns about the same drift on the
///   build side. `BALANCED_EXCLUDES` is intentionally not folded in
///   because it's an exclusion list, not a membership list.
/// - **Layer 5: `tool_deprecation` allowlist** (Sprint B-2) — tools on
///   the v1.13.27 deprecation list still appear in dispatch/preset
///   for backward compat but are intentionally missing from
///   tools.toml. Splitting them into the `intentional_deprecation`
///   bucket prevents the 27-entry deprecation cycle from showing up
///   as 27 false-positive violations.
///
/// Three violation classes are reported plus the `intentional_deprecation`
/// surface. Violations should be empty in a clean tree; the deprecation
/// bucket is informational. The CI tolerance is `all_clean = true`.
///
/// Earlier revisions (v1.13.9..v1.13.27) parsed `tools.toml` and the
/// `dispatch_table()` macro body with regexes, which broke whenever
/// a doc comment inside the macro contained a placeholder arm. This
/// version asks the runtime instead, so it cannot drift from the
/// thing it audits.
pub fn audit_tool_surface_consistency(_state: &AppState, _arguments: &Value) -> ToolResult {
    use crate::tool_defs::{
        ToolPreset, ToolSurface, tool_deprecation, visible_tools,
        whitelist_preset_member_union,
    };

    let toml_tools: BTreeSet<String> = visible_tools(ToolSurface::Preset(ToolPreset::Full))
        .into_iter()
        .map(|t| t.name.to_owned())
        .collect();

    let dispatched: BTreeSet<String> = crate::tools::dispatch_table()
        .keys()
        .map(|k| (*k).to_owned())
        .collect();

    let preset_members: BTreeSet<String> = whitelist_preset_member_union()
        .into_iter()
        .map(str::to_owned)
        .collect();

    let is_intentional = |name: &String| tool_deprecation(name).is_some();

    // `missing_in_dispatch` is NOT filtered through the deprecation list —
    // if a tool is registered in tools.toml (schema visible) but not in
    // dispatch (no handler), that's a real bug regardless of deprecation
    // status. Schema visibility implies callable.
    let missing_in_dispatch: Vec<String> = toml_tools.difference(&dispatched).cloned().collect();

    let raw_missing_in_toml: Vec<String> =
        dispatched.difference(&toml_tools).cloned().collect();
    let (intentional_missing_toml, missing_in_toml): (Vec<String>, Vec<String>) =
        raw_missing_in_toml
            .into_iter()
            .partition(is_intentional);

    let raw_orphan_in_preset: Vec<String> =
        preset_members.difference(&toml_tools).cloned().collect();
    let (intentional_orphan_preset, orphan_in_preset): (Vec<String>, Vec<String>) =
        raw_orphan_in_preset
            .into_iter()
            .partition(is_intentional);

    let intentional_deprecation: Vec<String> = intentional_missing_toml
        .into_iter()
        .chain(intentional_orphan_preset)
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect();

    let violation_count =
        missing_in_dispatch.len() + missing_in_toml.len() + orphan_in_preset.len();
    let all_clean = violation_count == 0;

    let mut next_actions: Vec<String> = Vec::new();
    if !missing_in_dispatch.is_empty() {
        next_actions.push(format!(
            "{} tool(s) registered in tools.toml have no dispatch arm — add them to `crate::tools::dispatch_table` or remove the toml entries.",
            missing_in_dispatch.len()
        ));
    }
    if !missing_in_toml.is_empty() {
        next_actions.push(format!(
            "{} tool(s) wired into dispatch_table have no tools.toml schema (and are not on the deprecation allowlist) — add the schema (and re-run `scripts/regen-tool-defs.py --write`) or drop the dispatch arm.",
            missing_in_toml.len()
        ));
    }
    if !orphan_in_preset.is_empty() {
        next_actions.push(format!(
            "{} tool(s) listed in a preset whitelist no longer exist in tools.toml (and are not on the deprecation allowlist) — prune the preset entries or restore the tool.",
            orphan_in_preset.len()
        ));
    }
    if !intentional_deprecation.is_empty() {
        next_actions.push(format!(
            "{} tool(s) are on the v1.13.27 deprecation allowlist (kept in dispatch/preset for backward compat) — surfaced for visibility, not counted as violations.",
            intentional_deprecation.len()
        ));
    }
    if all_clean {
        next_actions.push("Surface is consistent across all checked layers.".to_owned());
    }

    Ok((
        json!({
            "all_clean": all_clean,
            "violation_count": violation_count,
            "layers_checked": [
                "tools.toml (via tool_defs::visible_tools(Preset::Full))",
                "dispatch_table (runtime HashMap)",
                "presets.{MINIMAL,PLANNER_READONLY,BUILDER_MINIMAL,REVIEWER_GRAPH}_TOOLS",
                "tool_deprecation (v1.13.27 allowlist, surfaced separately)",
            ],
            "summary": {
                "toml_tool_count": toml_tools.len(),
                "dispatch_count": dispatched.len(),
                "preset_member_count": preset_members.len(),
                "intentional_deprecation_count": intentional_deprecation.len(),
            },
            "violations": {
                "missing_in_dispatch": missing_in_dispatch,
                "missing_in_toml": missing_in_toml,
                "orphan_in_preset": orphan_in_preset,
            },
            "intentional_deprecation": intentional_deprecation,
            "next_actions": next_actions,
        }),
        success_meta(BackendKind::Config, 1.0),
    ))
}

#[cfg(test)]
mod surface_audit_tests {
    use super::*;
    use crate::state::AppState;
    use crate::test_helpers::fixtures::temp_project_root;
    use crate::tool_defs::ToolPreset;

    fn make_state() -> AppState {
        // The audit reads only the static tool registry and the dispatch
        // table; the project root is essentially unused. Use the lightweight
        // minimal constructor so we don't spin up watchers or index workers
        // for every test invocation.
        AppState::new_minimal(temp_project_root("audit_v2"), ToolPreset::Full)
    }

    #[test]
    fn audit_returns_three_violation_buckets() {
        let state = make_state();
        let (payload, _meta) =
            audit_tool_surface_consistency(&state, &json!({})).expect("audit succeeds");
        let data = payload.as_object().expect("object response");
        let violations = data["violations"].as_object().expect("violations object");
        assert!(violations.contains_key("missing_in_dispatch"));
        assert!(violations.contains_key("missing_in_toml"));
        assert!(violations.contains_key("orphan_in_preset"));
        assert!(data.contains_key("all_clean"));
        assert!(data.contains_key("violation_count"));
        assert!(data.contains_key("layers_checked"));
    }

    #[test]
    fn audit_self_includes_itself_in_dispatch() {
        // Once the dispatch_table arm + tools.toml entry land in the
        // same PR, calling the audit must see itself on both sides:
        // its own name must NOT appear in `missing_in_dispatch` or
        // `missing_in_toml`. This pins the registration to keep the
        // audit self-discoverable in subsequent PRs.
        let state = make_state();
        let (payload, _meta) =
            audit_tool_surface_consistency(&state, &json!({})).expect("audit succeeds");
        let violations = &payload["violations"];
        let missing_dispatch = violations["missing_in_dispatch"]
            .as_array()
            .map(|arr| arr.iter().any(|v| v == "audit_tool_surface_consistency"))
            .unwrap_or(false);
        let missing_toml = violations["missing_in_toml"]
            .as_array()
            .map(|arr| arr.iter().any(|v| v == "audit_tool_surface_consistency"))
            .unwrap_or(false);
        assert!(
            !missing_dispatch,
            "audit_tool_surface_consistency must be in dispatch_table (regression guard)"
        );
        assert!(
            !missing_toml,
            "audit_tool_surface_consistency must be in tools.toml (regression guard)"
        );
    }
}
