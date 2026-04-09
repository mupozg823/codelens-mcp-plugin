use super::report_payload::{build_handle_payload, infer_risk_level};
use super::report_verifier::build_verifier_contract;
use super::{AppState, ToolResult, success_meta};
use crate::protocol::BackendKind;
use crate::tool_defs::{ToolProfile, ToolSurface};
use serde_json::Value;
use std::collections::BTreeMap;

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
) -> ToolResult {
    let risk_level = infer_risk_level(&summary, &top_findings, &next_actions);
    let ci_audit = matches!(*state.surface(), ToolSurface::Profile(ToolProfile::CiAudit));
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
        && let Some(artifact) = state.find_reusable_analysis(tool_name, cache_key)
    {
        state.metrics().record_analysis_cache_hit();
        let data = build_handle_payload(
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
        state.metrics().record_quality_contract_emitted(
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
        );
        state.metrics().record_verifier_contract_emitted(
            data["blockers"].as_array().map(|v| v.len()).unwrap_or(0),
            data["verifier_checks"]
                .as_array()
                .map(|v| v.len())
                .unwrap_or(0),
        );
        return Ok((data, success_meta(BackendKind::Hybrid, artifact.confidence)));
    }
    let artifact = state.store_analysis(
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
    state.metrics().record_quality_contract_emitted(
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
    );
    state.metrics().record_verifier_contract_emitted(
        data["blockers"].as_array().map(|v| v.len()).unwrap_or(0),
        data["verifier_checks"]
            .as_array()
            .map(|v| v.len())
            .unwrap_or(0),
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
