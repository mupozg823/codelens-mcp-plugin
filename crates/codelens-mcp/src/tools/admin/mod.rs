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

mod memory_consistency;
pub use memory_consistency::audit_memory_consistency;

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
        ToolPreset, ToolSurface, tool_deprecation, tool_feature_gate, visible_tools,
        whitelist_preset_member_union,
    };

    let toml_tools: BTreeSet<String> = visible_tools(ToolSurface::Preset(ToolPreset::Full))
        .into_iter()
        .map(|t| t.name.to_owned())
        .collect();

    let dispatched: BTreeSet<String> = crate::dispatch::registered_tool_names();

    let preset_members: BTreeSet<String> = whitelist_preset_member_union()
        .into_iter()
        .map(str::to_owned)
        .collect();

    let is_deprecated = |name: &String| tool_deprecation(name).is_some();
    let is_pending_d3 = |name: &String| crate::tools::PENDING_D3_ALLOWLIST.contains(&name.as_str());
    let is_feature_gated_out = |name: &String| {
        matches!(tool_feature_gate(name), Some("semantic")) && !cfg!(feature = "semantic")
    };

    // `missing_in_dispatch` is NOT filtered through the deprecation list —
    // if a tool is registered in tools.toml (schema visible) but not in
    // dispatch (no handler), that's a real bug regardless of deprecation
    // status. Schema visibility implies callable.
    let missing_in_dispatch: Vec<String> = toml_tools.difference(&dispatched).cloned().collect();

    let raw_missing_in_toml: Vec<String> = dispatched.difference(&toml_tools).cloned().collect();
    let (pending_d3_missing_toml, raw_missing_in_toml): (Vec<String>, Vec<String>) =
        raw_missing_in_toml.into_iter().partition(is_pending_d3);
    let (intentional_missing_toml, maybe_missing_in_toml): (Vec<String>, Vec<String>) =
        raw_missing_in_toml.into_iter().partition(is_deprecated);
    let (feature_gated_missing_toml, missing_in_toml): (Vec<String>, Vec<String>) =
        maybe_missing_in_toml
            .into_iter()
            .partition(is_feature_gated_out);

    let raw_orphan_in_preset: Vec<String> =
        preset_members.difference(&toml_tools).cloned().collect();
    let (pending_d3_orphan_preset, raw_orphan_in_preset): (Vec<String>, Vec<String>) =
        raw_orphan_in_preset.into_iter().partition(is_pending_d3);
    let (intentional_orphan_preset, maybe_orphan_in_preset): (Vec<String>, Vec<String>) =
        raw_orphan_in_preset.into_iter().partition(is_deprecated);
    let (feature_gated_orphan_preset, orphan_in_preset): (Vec<String>, Vec<String>) =
        maybe_orphan_in_preset
            .into_iter()
            .partition(is_feature_gated_out);

    let pending_d3_allowlisted: Vec<String> = pending_d3_missing_toml
        .into_iter()
        .chain(pending_d3_orphan_preset)
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect();

    let intentional_deprecation: Vec<String> = intentional_missing_toml
        .into_iter()
        .chain(intentional_orphan_preset)
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect();
    let intentional_feature_gated: Vec<String> = feature_gated_missing_toml
        .into_iter()
        .chain(feature_gated_orphan_preset)
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
    if !pending_d3_allowlisted.is_empty() {
        next_actions.push(format!(
            "{} tool(s) are on the pending-D3 allowlist (#346): dispatch-only symbolic edit core awaiting the ADR-0009/D3 re-listing decision — surfaced for visibility, not counted as violations.",
            pending_d3_allowlisted.len()
        ));
    }
    if !intentional_deprecation.is_empty() {
        next_actions.push(format!(
            "{} tool(s) are on the v1.13.27 deprecation allowlist (kept in dispatch/preset for backward compat) — surfaced for visibility, not counted as violations.",
            intentional_deprecation.len()
        ));
    }
    if !intentional_feature_gated.is_empty() {
        next_actions.push(format!(
            "{} feature-gated tool reference(s) are hidden in this build and not counted as violations.",
            intentional_feature_gated.len()
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
                "PENDING_D3_ALLOWLIST (#346 dispatch-only carve-out, surfaced separately)",
            ],
            "summary": {
                "toml_tool_count": toml_tools.len(),
                "dispatch_count": dispatched.len(),
                "preset_member_count": preset_members.len(),
                "intentional_deprecation_count": intentional_deprecation.len(),
                "intentional_feature_gated_count": intentional_feature_gated.len(),
                "pending_d3_allowlisted_count": pending_d3_allowlisted.len(),
            },
            "violations": {
                "missing_in_dispatch": missing_in_dispatch,
                "missing_in_toml": missing_in_toml,
                "orphan_in_preset": orphan_in_preset,
            },
            "intentional_deprecation": intentional_deprecation,
            "intentional_feature_gated": intentional_feature_gated,
            "pending_d3_allowlisted": pending_d3_allowlisted,
            "next_actions": next_actions,
        }),
        success_meta(BackendKind::Config, 1.0),
    ))
}

/// Surface phantom `mod NAME;` declarations whose target name is never
/// `use`d anywhere else in the workspace. Resurrected from the v1.13.27
/// surface trim alongside `audit_tool_surface_consistency`. Engine impl
/// in `codelens_engine::phantom_modules` is unchanged — this is the
/// missing MCP wrapper. Heuristic: `pub mod` is reported but may be
/// intentional for re-export patterns, so visibility is included in
/// each entry for callers to filter.
pub fn find_phantom_modules(state: &AppState, arguments: &Value) -> ToolResult {
    let max_results = optional_usize(arguments, "max_results", 50).clamp(1, 500);
    let project = state.project();

    let entries = codelens_engine::phantom_modules::find_phantom_modules(&project, max_results)
        .map_err(|err| CodeLensError::Internal(err.context("find_phantom_modules")))?;

    let count = entries.len();
    let truncated = count >= max_results;
    let mut next_actions = Vec::new();
    if count == 0 {
        next_actions.push("No phantom modules detected.".to_owned());
    } else {
        next_actions.push(format!(
            "{} phantom mod declaration(s) found — review before removal (re-export patterns can keep `pub mod` useful).",
            count,
        ));
        if truncated {
            next_actions.push(
                "Result truncated. Raise `max_results` (max 500) for the full list.".to_owned(),
            );
        }
    }

    Ok((
        json!({
            "phantom_modules": entries,
            "count": count,
            "max_results": max_results,
            "truncated": truncated,
            "next_actions": next_actions,
        }),
        success_meta(BackendKind::TreeSitter, 0.85),
    ))
}

/// Surface Rust one-line wrappers whose entire body forwards to another
/// function with a literal default argument. Resurrected from the v1.13.27
/// surface trim. Engine impl in `codelens_engine::redundant_definitions`
/// unchanged. Group results by `target` to find substrates with multiple
/// wrappers — the highest cleanup leverage per Phase 1-A's findings.
pub fn find_redundant_definitions(state: &AppState, arguments: &Value) -> ToolResult {
    let max_results = optional_usize(arguments, "max_results", 50).clamp(1, 500);
    let project = state.project();

    let entries =
        codelens_engine::redundant_definitions::find_redundant_definitions(&project, max_results)
            .map_err(|err| CodeLensError::Internal(err.context("find_redundant_definitions")))?;

    let count = entries.len();
    let truncated = count >= max_results;
    let mut next_actions = Vec::new();
    if count == 0 {
        next_actions.push("No one-line wrapper redundancies detected.".to_owned());
    } else {
        next_actions.push(format!(
            "{} one-line wrapper(s) found. Group by `target` to find substrates with multiple wrappers — highest cleanup leverage.",
            count,
        ));
        if truncated {
            next_actions.push(
                "Result truncated. Raise `max_results` (max 500) for the full list.".to_owned(),
            );
        }
    }

    Ok((
        json!({
            "redundant_definitions": entries,
            "count": count,
            "max_results": max_results,
            "truncated": truncated,
            "next_actions": next_actions,
        }),
        success_meta(BackendKind::TreeSitter, 0.85),
    ))
}

/// Surface tools whose annotations contradict the readonly-intent of the
/// surface they appear in. A `destructive_hint=true` or
/// `approval_required=true` tool listed on a readonly preset/profile
/// (`Minimal`, `PlannerReadonly`, `ReviewerGraph`) is leakage — the
/// surface promises read-only safety, but the tool reserves write or
/// approval semantics. This is the runtime detector that the 2026-05-18
/// dogfood memo referred to as "495 over-visible cleanup".
///
/// Resurrected from the v1.13.27 surface trim. Runtime query only — no
/// engine impl needed, since the data lives entirely in the `Tool`
/// registry (compiled from tools.toml) and the preset whitelists. Sits
/// alongside `audit_tool_surface_consistency` and the resurrected
/// `find_phantom_modules` / `find_redundant_definitions` in the
/// self-auditability detector family.
pub fn find_over_visible_apis(_state: &AppState, _arguments: &Value) -> ToolResult {
    use crate::tool_defs::{ToolPreset, ToolProfile, ToolSurface, visible_tools};

    let readonly_surfaces: &[(&str, ToolSurface)] = &[
        ("preset:minimal", ToolSurface::Preset(ToolPreset::Minimal)),
        (
            "profile:planner-readonly",
            ToolSurface::Profile(ToolProfile::PlannerReadonly),
        ),
        (
            "profile:reviewer-graph",
            ToolSurface::Profile(ToolProfile::ReviewerGraph),
        ),
    ];

    let mut violations: Vec<Value> = Vec::new();
    for (label, surface) in readonly_surfaces {
        for tool in visible_tools(*surface) {
            let Some(ann) = tool.annotations.as_ref() else {
                continue;
            };
            let mut reasons: Vec<&'static str> = Vec::new();
            if ann.destructive_hint == Some(true) {
                reasons.push("destructive_hint=true");
            }
            if ann.approval_required == Some(true) {
                reasons.push("approval_required=true");
            }
            if !reasons.is_empty() {
                violations.push(json!({
                    "surface": label,
                    "tool": tool.name,
                    "reasons": reasons,
                    "destructive_hint": ann.destructive_hint,
                    "approval_required": ann.approval_required,
                    "audit_category": ann.audit_category,
                }));
            }
        }
    }

    let violation_count = violations.len();
    let all_clean = violation_count == 0;
    let mut next_actions: Vec<String> = Vec::new();
    if all_clean {
        next_actions.push(
            "No over-visible API leakage: every readonly-intent surface is free of destructive or approval-required tools."
                .to_owned(),
        );
    } else {
        next_actions.push(format!(
            "{violation_count} over-visible exposure(s) across readonly surfaces. Either tighten the preset/profile member list, or relax the tool annotation (only if the previous tag was wrong).",
        ));
    }

    Ok((
        json!({
            "violations": violations,
            "violation_count": violation_count,
            "all_clean": all_clean,
            "readonly_surfaces_checked": readonly_surfaces
                .iter()
                .map(|(label, _)| (*label).to_owned())
                .collect::<Vec<_>>(),
            "policy": {
                "destructive_hint_true": "over-visible in any readonly-intent surface",
                "approval_required_true": "over-visible in any readonly-intent surface",
            },
            "next_actions": next_actions,
        }),
        success_meta(BackendKind::Config, 1.0),
    ))
}
