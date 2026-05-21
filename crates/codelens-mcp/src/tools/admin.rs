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
        ToolPreset, ToolSurface, tool_deprecation, visible_tools, whitelist_preset_member_union,
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

    let raw_missing_in_toml: Vec<String> = dispatched.difference(&toml_tools).cloned().collect();
    let (intentional_missing_toml, missing_in_toml): (Vec<String>, Vec<String>) =
        raw_missing_in_toml.into_iter().partition(is_intentional);

    let raw_orphan_in_preset: Vec<String> =
        preset_members.difference(&toml_tools).cloned().collect();
    let (intentional_orphan_preset, orphan_in_preset): (Vec<String>, Vec<String>) =
        raw_orphan_in_preset.into_iter().partition(is_intentional);

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

/// Surface project memory files (`.codelens/memories/*.md`) whose
/// modification time exceeds the staleness threshold. Memories are
/// frozen-in-time observations — without a freshness check they
/// silently drift from the codebase they describe (e.g. cited
/// file paths get renamed, cited symbols disappear, cited
/// architectural claims stop matching the code). This is the
/// self-auditability complement to the four detectors that audit
/// the tool surface; the same pattern (runtime query, admin-only,
/// preset_tags=[]) applies.
///
/// Threshold is configurable via `threshold_days` argument
/// (default 30, clamped 1..=3650). Each entry reports the file
/// path (relative to project root), age in days, and modification
/// timestamp (epoch millis) so callers can fold the output back
/// into a freshness ratchet.
pub fn audit_memory_consistency(state: &AppState, arguments: &Value) -> ToolResult {
    let threshold_days = optional_usize(arguments, "threshold_days", 30).clamp(1, 3650);
    let threshold_secs = (threshold_days as u64) * 24 * 60 * 60;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let memories_dir = state.memories_dir();
    let project_root = state.project().as_path().to_path_buf();

    let mut total_files = 0u64;
    let mut stale: Vec<Value> = Vec::new();

    if memories_dir.exists() {
        let entries = match std::fs::read_dir(&memories_dir) {
            Ok(rd) => rd,
            Err(error) => {
                return Err(CodeLensError::Io(error));
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            total_files += 1;
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            let Ok(mtime_dur) = modified.duration_since(std::time::UNIX_EPOCH) else {
                continue;
            };
            let mtime_secs = mtime_dur.as_secs();
            let age_secs = now_secs.saturating_sub(mtime_secs);
            if age_secs <= threshold_secs {
                continue;
            }
            let relative = path
                .strip_prefix(&project_root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| path.to_string_lossy().to_string());
            stale.push(json!({
                "file": relative,
                "age_days": age_secs / (24 * 60 * 60),
                "mtime_epoch_secs": mtime_secs,
            }));
        }
    }

    // Sort stale entries deterministically — oldest first surfaces
    // the most egregious drift to readers / dashboards.
    stale.sort_by(|a, b| {
        b["age_days"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["age_days"].as_u64().unwrap_or(0))
            .then_with(|| {
                a["file"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["file"].as_str().unwrap_or(""))
            })
    });

    let stale_count = stale.len();
    let all_clean = stale_count == 0;
    let mut next_actions: Vec<String> = Vec::new();
    if all_clean {
        next_actions.push(format!(
            "No stale memories ({total_files} file(s) scanned, all newer than {threshold_days} days)."
        ));
    } else {
        next_actions.push(format!(
            "{stale_count} memory file(s) older than {threshold_days} days. Re-verify against current code, update mtime by re-saving, or delete if obsolete.",
        ));
    }

    Ok((
        json!({
            "memories_dir": memories_dir.to_string_lossy(),
            "total_files": total_files,
            "stale_count": stale_count,
            "stale_entries": stale,
            "threshold_days": threshold_days,
            "all_clean": all_clean,
            "next_actions": next_actions,
        }),
        success_meta(BackendKind::Filesystem, 1.0),
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

    #[test]
    fn resurrected_detectors_registered_on_both_sides() {
        // Regression guard for the v1.13.27 surface trim follow-up: the two
        // detectors must stay in dispatch_table + tools.toml together. If
        // one drifts, the audit's `missing_in_*` buckets surface it.
        let state = make_state();
        let (payload, _meta) =
            audit_tool_surface_consistency(&state, &json!({})).expect("audit succeeds");
        let violations = &payload["violations"];
        for tool in ["find_phantom_modules", "find_redundant_definitions"] {
            let missing_dispatch = violations["missing_in_dispatch"]
                .as_array()
                .map(|arr| arr.iter().any(|v| v == tool))
                .unwrap_or(false);
            let missing_toml = violations["missing_in_toml"]
                .as_array()
                .map(|arr| arr.iter().any(|v| v == tool))
                .unwrap_or(false);
            assert!(!missing_dispatch, "{tool} must be in dispatch_table");
            assert!(!missing_toml, "{tool} must be in tools.toml");
        }
    }

    #[test]
    fn find_phantom_modules_on_empty_project_returns_zero() {
        let state = make_state();
        let (payload, _meta) = find_phantom_modules(&state, &json!({})).expect("call succeeds");
        assert_eq!(payload["count"].as_u64().unwrap_or(99), 0);
        assert!(payload["phantom_modules"].as_array().unwrap().is_empty());
        assert_eq!(payload["truncated"].as_bool().unwrap_or(true), false);
        assert_eq!(payload["max_results"].as_u64().unwrap_or(0), 50);
    }

    #[test]
    fn audit_memory_consistency_registered_on_both_sides() {
        let state = make_state();
        let (payload, _meta) =
            audit_tool_surface_consistency(&state, &json!({})).expect("audit succeeds");
        let violations = &payload["violations"];
        let tool = "audit_memory_consistency";
        let missing_dispatch = violations["missing_in_dispatch"]
            .as_array()
            .map(|arr| arr.iter().any(|v| v == tool))
            .unwrap_or(false);
        let missing_toml = violations["missing_in_toml"]
            .as_array()
            .map(|arr| arr.iter().any(|v| v == tool))
            .unwrap_or(false);
        assert!(!missing_dispatch, "{tool} must be in dispatch_table");
        assert!(!missing_toml, "{tool} must be in tools.toml");
    }

    #[test]
    fn audit_memory_consistency_shape_and_threshold_keys() {
        let state = make_state();
        let (payload, _meta) = audit_memory_consistency(&state, &json!({})).expect("call succeeds");
        assert!(payload["total_files"].is_number());
        assert!(payload["stale_count"].is_number());
        assert!(payload["all_clean"].is_boolean());
        assert_eq!(payload["threshold_days"].as_u64().unwrap_or(0), 30);
        assert!(payload["memories_dir"].is_string());
        assert!(payload["stale_entries"].is_array());
        assert!(payload["next_actions"].as_array().unwrap().len() >= 1);
    }

    #[test]
    fn audit_memory_consistency_clamps_threshold_days() {
        let state = make_state();
        let (low, _) = audit_memory_consistency(&state, &json!({"threshold_days": 0}))
            .expect("call succeeds with 0");
        assert_eq!(low["threshold_days"].as_u64().unwrap_or(0), 1);
        let (high, _) = audit_memory_consistency(&state, &json!({"threshold_days": 10000}))
            .expect("call succeeds with 10000");
        assert_eq!(high["threshold_days"].as_u64().unwrap_or(0), 3650);
    }

    #[test]
    fn find_over_visible_apis_registered_on_both_sides() {
        // Regression guard: the detector must stay in dispatch_table +
        // tools.toml together. Same pattern as `audit_self_includes_itself_in_dispatch`.
        let state = make_state();
        let (payload, _meta) =
            audit_tool_surface_consistency(&state, &json!({})).expect("audit succeeds");
        let violations = &payload["violations"];
        let missing_dispatch = violations["missing_in_dispatch"]
            .as_array()
            .map(|arr| arr.iter().any(|v| v == "find_over_visible_apis"))
            .unwrap_or(false);
        let missing_toml = violations["missing_in_toml"]
            .as_array()
            .map(|arr| arr.iter().any(|v| v == "find_over_visible_apis"))
            .unwrap_or(false);
        assert!(
            !missing_dispatch,
            "find_over_visible_apis must be in dispatch_table"
        );
        assert!(
            !missing_toml,
            "find_over_visible_apis must be in tools.toml"
        );
    }

    #[test]
    fn find_over_visible_apis_shape_and_policy_keys() {
        // Smoke-test the shape: should return JSON with violations,
        // violation_count, all_clean, readonly_surfaces_checked,
        // policy, next_actions. Content of violations depends on the
        // current preset whitelists, so we don't pin specific tool names.
        let state = make_state();
        let (payload, _meta) = find_over_visible_apis(&state, &json!({})).expect("call succeeds");
        assert!(payload["violation_count"].is_number());
        assert!(payload["all_clean"].is_boolean());
        let surfaces = payload["readonly_surfaces_checked"]
            .as_array()
            .expect("readonly_surfaces_checked is an array");
        assert_eq!(surfaces.len(), 3, "checks 3 readonly-intent surfaces");
        assert!(
            payload["policy"]["destructive_hint_true"].is_string(),
            "policy.destructive_hint_true documented",
        );
        assert!(
            payload["policy"]["approval_required_true"].is_string(),
            "policy.approval_required_true documented",
        );
        assert!(
            payload["next_actions"].as_array().unwrap().len() >= 1,
            "at least one next_action surfaced",
        );
        // Every violation entry must carry tool + surface + reasons.
        for v in payload["violations"].as_array().unwrap() {
            assert!(v["tool"].is_string());
            assert!(v["surface"].is_string());
            let reasons = v["reasons"].as_array().expect("reasons is an array");
            assert!(
                !reasons.is_empty(),
                "violation must cite at least one reason"
            );
        }
    }

    #[test]
    fn find_redundant_definitions_on_empty_project_returns_zero() {
        let state = make_state();
        let (payload, _meta) =
            find_redundant_definitions(&state, &json!({})).expect("call succeeds");
        assert_eq!(payload["count"].as_u64().unwrap_or(99), 0);
        assert!(
            payload["redundant_definitions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(payload["truncated"].as_bool().unwrap_or(true), false);
        assert_eq!(payload["max_results"].as_u64().unwrap_or(0), 50);
    }
}
