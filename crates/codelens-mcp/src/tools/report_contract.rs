use super::report_payload::{build_handle_payload, infer_risk_level};
use super::report_verifier::build_verifier_contract;
use super::{AppState, ToolResult, success_meta};
use crate::protocol::BackendKind;
use crate::session_context::SessionRequestContext;
use crate::tool_defs::{ToolProfile, ToolSurface};
use serde_json::Value;
use std::collections::BTreeMap;

/// Phase P4-b: infer a `cargo test -p <crate>` command from a
/// `crates/<crate>/...` touched path. Returns `None` when no path
/// matches the pattern, which lets callers emit the structured entry
/// with `command: null` — preserving the backward-compatible
/// string-only `next_actions` contract.
fn crate_from_touched_paths(touched_files: &[String]) -> Option<String> {
    for file in touched_files {
        let normalized = file.replace('\\', "/");
        let mut components = normalized.split('/');
        while let Some(segment) = components.next() {
            if segment == "crates"
                && let Some(candidate) = components.next()
                && !candidate.is_empty()
                && !candidate.starts_with('.')
            {
                return Some(candidate.to_owned());
            }
        }
    }
    None
}

/// Phase P4-b: promote each `next_actions` string to a structured
/// `{text, command?}` entry. When a cargo crate can be inferred from
/// touched files, prepend a `cargo test -p <crate>` entry so the
/// harness can run verification without re-deriving the crate name.
fn synthesize_next_actions_detailed(
    next_actions: &[String],
    touched_files: &[String],
) -> Vec<Value> {
    let mut detailed: Vec<Value> = Vec::with_capacity(next_actions.len() + 1);
    if let Some(crate_name) = crate_from_touched_paths(touched_files) {
        detailed.push(serde_json::json!({
            "text": format!(
                "Run crate-scoped tests after editing {crate_name} to catch regressions early"
            ),
            "command": format!("cargo test -p {crate_name}"),
            "kind": "verification",
        }));
    }
    for action in next_actions {
        detailed.push(serde_json::json!({ "text": action, "command": null }));
    }
    detailed
}

fn overlapping_claims_from_section(value: &Value) -> Vec<Value> {
    value
        .get("claims")
        .and_then(|claims| claims.as_array())
        .cloned()
        .unwrap_or_default()
}

fn overlapping_claims_from_sections(sections: &BTreeMap<String, Value>) -> Vec<Value> {
    sections
        .get("coordination_overlaps")
        .map(overlapping_claims_from_section)
        .unwrap_or_default()
}

fn overlapping_claims_from_artifact(state: &AppState, analysis_id: &str) -> Vec<Value> {
    state
        .peek_analysis_section(analysis_id, "coordination_overlaps")
        .ok()
        .map(|value| overlapping_claims_from_section(&value))
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn make_handle_response(
    state: &AppState,
    tool_name: &str,
    cache_key: Option<String>,
    summary: String,
    top_findings: Vec<String>,
    confidence: f64,
    next_actions: Vec<String>,
    mut sections: BTreeMap<String, Value>,
    touched_files: Vec<String>,
    symbol_hint: Option<String>,
    arguments: Option<&Value>,
) -> ToolResult {
    let logical_session_id = arguments
        .map(SessionRequestContext::from_json)
        .map(|session| session.session_id);
    let logical_session_id = logical_session_id.as_deref();
    let risk_level = infer_risk_level(&summary, &top_findings, &next_actions);
    let ci_audit = matches!(*state.surface(), ToolSurface::Profile(ToolProfile::CiAudit));
    let inline_overlapping_claims = overlapping_claims_from_sections(&sections);
    let verifier = build_verifier_contract(
        state,
        tool_name,
        &summary,
        &top_findings,
        &next_actions,
        &mut sections,
        &touched_files,
        symbol_hint.as_deref(),
    );
    if let Some(cache_key) = cache_key.as_deref()
        && let Some(artifact) = state.find_reusable_analysis_for_current_scope(tool_name, cache_key)
    {
        state
            .metrics()
            .record_analysis_cache_hit_for_session(logical_session_id);
        let mut data = build_handle_payload(
            tool_name,
            &artifact.id,
            &artifact.summary,
            &artifact.top_findings,
            &artifact.risk_level,
            artifact.confidence,
            &artifact.next_actions,
            &artifact.blockers,
            &artifact.readiness,
            &artifact.verifier_checks,
            &artifact.available_sections,
            true,
            ci_audit,
        );
        let overlapping_claims = overlapping_claims_from_artifact(state, &artifact.id);
        if !overlapping_claims.is_empty() {
            data["overlapping_claims"] = serde_json::json!(overlapping_claims);
        }
        // Phase P4-b: structured next actions with optional `command`
        // hints. Emit alongside the legacy string array so older
        // consumers keep working.
        data["next_actions_detailed"] = serde_json::json!(
            synthesize_next_actions_detailed(&artifact.next_actions, &touched_files)
        );
        state.metrics().record_quality_contract_emitted_for_session(
            data["quality_focus"]
                .as_array()
                .map(|v| v.len())
                .unwrap_or(0),
            data["recommended_checks"]
                .as_array()
                .map(|v| v.len())
                .unwrap_or(0),
            data["performance_watchpoints"]
                .as_array()
                .map(|v| v.len())
                .unwrap_or(0),
            logical_session_id,
        );
        state
            .metrics()
            .record_verifier_contract_emitted_for_session(
                data["blockers"].as_array().map(|v| v.len()).unwrap_or(0),
                data["verifier_checks"]
                    .as_array()
                    .map(|v| v.len())
                    .unwrap_or(0),
                logical_session_id,
            );
        return Ok((data, success_meta(BackendKind::Hybrid, artifact.confidence)));
    }
    let artifact = state.store_analysis_for_current_scope(
        tool_name,
        cache_key,
        summary.clone(),
        top_findings.clone(),
        risk_level.to_owned(),
        confidence,
        next_actions.clone(),
        verifier.blockers.clone(),
        verifier.readiness.clone(),
        verifier.verifier_checks.clone(),
        sections,
    )?;
    let mut data = build_handle_payload(
        tool_name,
        &artifact.id,
        &artifact.summary,
        &artifact.top_findings,
        &artifact.risk_level,
        artifact.confidence,
        &artifact.next_actions,
        &artifact.blockers,
        &artifact.readiness,
        &artifact.verifier_checks,
        &artifact.available_sections,
        false,
        ci_audit,
    );
    if !inline_overlapping_claims.is_empty() {
        data["overlapping_claims"] = serde_json::json!(inline_overlapping_claims);
    }
    // Phase P4-b: structured next actions (see cache-hit branch above).
    data["next_actions_detailed"] = serde_json::json!(
        synthesize_next_actions_detailed(&artifact.next_actions, &touched_files)
    );
    state.metrics().record_quality_contract_emitted_for_session(
        data["quality_focus"]
            .as_array()
            .map(|v| v.len())
            .unwrap_or(0),
        data["recommended_checks"]
            .as_array()
            .map(|v| v.len())
            .unwrap_or(0),
        data["performance_watchpoints"]
            .as_array()
            .map(|v| v.len())
            .unwrap_or(0),
        logical_session_id,
    );
    state
        .metrics()
        .record_verifier_contract_emitted_for_session(
            data["blockers"].as_array().map(|v| v.len()).unwrap_or(0),
            data["verifier_checks"]
                .as_array()
                .map(|v| v.len())
                .unwrap_or(0),
            logical_session_id,
        );
    // Cross-phase: inject recent analysis IDs so agents can reference prior results.
    let prior_ids = state.recent_analysis_ids();
    if prior_ids.len() > 1 {
        // Exclude current analysis from the list.
        let prior: Vec<_> = prior_ids
            .iter()
            .filter(|id| id.as_str() != artifact.id)
            .cloned()
            .collect();
        if !prior.is_empty() {
            data["prior_analyses"] = serde_json::json!(prior);
        }
    }
    Ok((data, success_meta(BackendKind::Hybrid, confidence)))
}
