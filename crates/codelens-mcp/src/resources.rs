//! MCP resource definitions and handlers.

use crate::AppState;
use crate::tool_defs::{
    ToolProfile, preferred_namespaces, preferred_tier_labels, tool_namespace, tool_tier_label,
    visible_namespaces, visible_tiers, visible_tools,
};
use codelens_core::{detect_frameworks, detect_workspace_packages};
use serde_json::json;
use std::collections::BTreeMap;

fn profile_guide(profile: ToolProfile) -> serde_json::Value {
    match profile {
        ToolProfile::PlannerReadonly => json!({
            "profile": profile.as_str(),
            "intent": "Use bounded, read-only analysis to plan changes and rank context before implementation.",
            "preferred_tools": ["verify_change_readiness", "analyze_change_request", "find_minimal_context_for_change", "impact_report"],
            "preferred_namespaces": ["reports", "symbols", "graph", "filesystem", "session"],
            "avoid": ["rename_symbol", "replace_content", "raw graph expansion unless necessary"]
        }),
        ToolProfile::BuilderMinimal => json!({
            "profile": profile.as_str(),
            "intent": "Keep the visible surface small while implementing changes with only the essential symbol and edit tools.",
            "preferred_tools": ["verify_change_readiness", "find_symbol", "get_symbols_overview", "add_import"],
            "preferred_namespaces": ["symbols", "filesystem", "session"],
            "avoid": ["dead-code audits", "full-graph exploration", "broad multi-project search"]
        }),
        ToolProfile::ReviewerGraph => json!({
            "profile": profile.as_str(),
            "intent": "Review risky changes with graph-aware, read-only evidence.",
            "preferred_tools": ["verify_change_readiness", "impact_report", "diff_aware_references", "dead_code_report"],
            "preferred_namespaces": ["reports", "graph", "symbols", "session"],
            "avoid": ["mutation tools"]
        }),
        ToolProfile::RefactorFull => json!({
            "profile": profile.as_str(),
            "intent": "Run high-safety refactors only after a fresh preflight has narrowed the target surface and cleared blockers.",
            "preferred_tools": ["verify_change_readiness", "safe_rename_report", "unresolved_reference_check", "rename_symbol"],
            "preferred_namespaces": ["reports", "mutation", "symbols", "session"],
            "avoid": ["mutation before preflight", "broad edits without diagnostics or preview"]
        }),
        ToolProfile::CiAudit => json!({
            "profile": profile.as_str(),
            "intent": "Produce machine-friendly review output around diffs, impact, dead code, and structural risk.",
            "preferred_tools": ["verify_change_readiness", "impact_report", "diff_aware_references", "dead_code_report"],
            "preferred_namespaces": ["reports", "graph", "session"],
            "avoid": ["interactive mutation flows"]
        }),
        ToolProfile::EvaluatorCompact => json!({
            "profile": profile.as_str(),
            "intent": "Minimal read-only profile for scoring harnesses — diagnostics, test discovery, and symbol lookup only.",
            "preferred_tools": ["verify_change_readiness", "get_file_diagnostics", "find_tests", "find_symbol"],
            "preferred_namespaces": ["reports", "symbols", "lsp", "session"],
            "avoid": ["mutation tools", "graph expansion", "broad analysis reports"]
        }),
    }
}

fn profile_guide_summary(profile: ToolProfile) -> serde_json::Value {
    let guide = profile_guide(profile);
    json!({
        "profile": guide.get("profile").cloned().unwrap_or(json!(profile.as_str())),
        "intent": guide.get("intent").cloned().unwrap_or(json!("")),
        "preferred_tools": guide.get("preferred_tools").cloned().unwrap_or(json!([])),
        "preferred_namespaces": guide.get("preferred_namespaces").cloned().unwrap_or(json!([])),
        "preferred_tiers": preferred_tier_labels(crate::tool_defs::ToolSurface::Profile(profile)),
    })
}

fn deferred_loading_requested(params: Option<&serde_json::Value>) -> bool {
    params
        .and_then(|params| params.get("_session_deferred_tool_loading"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn loaded_namespaces(params: Option<&serde_json::Value>) -> Vec<String> {
    params
        .and_then(|params| params.get("_session_loaded_namespaces"))
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn loaded_tiers(params: Option<&serde_json::Value>) -> Vec<String> {
    params
        .and_then(|params| params.get("_session_loaded_tiers"))
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn full_tool_exposure(params: Option<&serde_json::Value>) -> bool {
    params
        .and_then(|params| params.get("_session_full_tool_exposure"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn resource_requested_namespace(params: Option<&serde_json::Value>) -> Option<String> {
    params
        .and_then(|params| params.get("namespace"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}

fn resource_requested_tier(params: Option<&serde_json::Value>) -> Option<String> {
    params
        .and_then(|params| params.get("tier"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}

fn resource_full_listing(uri: &str, params: Option<&serde_json::Value>) -> bool {
    uri == "codelens://tools/list/full"
        || params
            .and_then(|params| params.get("full"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
}

fn effective_visible_tools(
    state: &AppState,
    uri: &str,
    params: Option<&serde_json::Value>,
) -> (
    Vec<&'static crate::protocol::Tool>,
    Vec<&'static str>,
    Vec<&'static str>,
    Vec<&'static str>,
    Vec<&'static str>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Option<String>,
    Option<String>,
    bool,
    bool,
) {
    let surface = *state.surface();
    let all_tools = visible_tools(surface);
    let all_namespaces = visible_namespaces(surface);
    let all_tiers = visible_tiers(surface);
    let preferred = preferred_namespaces(surface);
    let preferred_tiers = preferred_tier_labels(surface);
    let loaded = loaded_namespaces(params);
    let loaded_tier_values = loaded_tiers(params);
    let selected_namespace = resource_requested_namespace(params);
    let selected_tier = resource_requested_tier(params);
    let full_listing = resource_full_listing(uri, params);
    let full_exposure = full_tool_exposure(params);
    let deferred = deferred_loading_requested(params);
    let filtered = all_tools
        .iter()
        .copied()
        .filter(|tool| match selected_namespace.as_deref() {
            Some(namespace) => tool_namespace(tool.name) == namespace,
            None if deferred && !full_listing && !full_exposure => {
                let namespace = tool_namespace(tool.name);
                preferred.contains(&namespace) || loaded.iter().any(|value| value == namespace)
            }
            None => true,
        })
        .filter(|tool| match selected_tier.as_deref() {
            Some(tier) => tool_tier_label(tool.name) == tier,
            None if deferred && !full_listing && !full_exposure => {
                let tier = tool_tier_label(tool.name);
                preferred_tiers.contains(&tier)
                    || loaded_tier_values.iter().any(|value| value == tier)
            }
            None => true,
        })
        .collect::<Vec<_>>();
    let mut effective = preferred
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    for namespace in &loaded {
        if !effective.iter().any(|value| value == namespace) {
            effective.push(namespace.clone());
        }
    }
    effective.sort();
    let mut effective_tiers = preferred_tiers
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    for tier in &loaded_tier_values {
        if !effective_tiers.iter().any(|value| value == tier) {
            effective_tiers.push(tier.clone());
        }
    }
    effective_tiers.sort();
    (
        filtered,
        all_namespaces,
        all_tiers,
        preferred,
        preferred_tiers,
        loaded,
        loaded_tier_values,
        effective,
        effective_tiers,
        selected_namespace,
        selected_tier,
        deferred && !full_listing && !full_exposure,
        full_exposure,
    )
}

fn visible_tool_summary(
    state: &AppState,
    uri: &str,
    params: Option<&serde_json::Value>,
) -> serde_json::Value {
    let surface = *state.surface();
    let (
        tools,
        all_namespaces,
        all_tiers,
        preferred,
        preferred_tiers,
        loaded,
        loaded_tiers,
        effective,
        effective_tiers,
        selected_namespace,
        selected_tier,
        deferred_active,
        full_exposure,
    ) = effective_visible_tools(state, uri, params);
    let mut namespace_counts = BTreeMap::new();
    let mut tier_counts = BTreeMap::new();
    for tool in &tools {
        *namespace_counts
            .entry(tool_namespace(tool.name).to_owned())
            .or_insert(0usize) += 1;
        *tier_counts
            .entry(tool_tier_label(tool.name).to_owned())
            .or_insert(0usize) += 1;
    }
    let prioritized = tools
        .iter()
        .take(8)
        .map(|tool| {
            json!({
                "name": tool.name,
                "namespace": tool_namespace(tool.name),
                "tier": tool_tier_label(tool.name)
            })
        })
        .collect::<Vec<_>>();
    let deferred_loading_active =
        deferred_active && selected_namespace.is_none() && selected_tier.is_none();
    json!({
        "active_surface": surface.as_label(),
        "tool_count": tools.len(),
        "tool_count_total": visible_tools(surface).len(),
        "visible_namespaces": namespace_counts,
        "visible_tiers": tier_counts,
        "all_namespaces": all_namespaces,
        "all_tiers": all_tiers,
        "preferred_namespaces": preferred,
        "preferred_tiers": preferred_tiers,
        "loaded_namespaces": loaded,
        "loaded_tiers": loaded_tiers,
        "effective_namespaces": effective,
        "effective_tiers": effective_tiers,
        "selected_namespace": selected_namespace,
        "selected_tier": selected_tier,
        "deferred_loading_active": deferred_loading_active,
        "full_tool_exposure": full_exposure,
        "recommended_tools": prioritized,
        "note": "Read `codelens://tools/list/full` only when summary is insufficient."
    })
}

fn visible_tool_details(
    state: &AppState,
    uri: &str,
    params: Option<&serde_json::Value>,
) -> serde_json::Value {
    let surface = *state.surface();
    let (
        tools,
        all_namespaces,
        all_tiers,
        preferred,
        preferred_tiers,
        loaded,
        loaded_tiers,
        effective,
        effective_tiers,
        selected_namespace,
        selected_tier,
        deferred_active,
        full_exposure,
    ) = effective_visible_tools(state, uri, params);
    let tools = tools
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "namespace": tool_namespace(tool.name),
                "description": tool.description,
                "tier": tool_tier_label(tool.name)
            })
        })
        .collect::<Vec<_>>();
    let deferred_loading_active =
        deferred_active && selected_namespace.is_none() && selected_tier.is_none();
    json!({
        "active_surface": surface.as_label(),
        "tool_count": tools.len(),
        "tool_count_total": visible_tools(surface).len(),
        "all_namespaces": all_namespaces,
        "all_tiers": all_tiers,
        "preferred_namespaces": preferred,
        "preferred_tiers": preferred_tiers,
        "loaded_namespaces": loaded,
        "loaded_tiers": loaded_tiers,
        "effective_namespaces": effective,
        "effective_tiers": effective_tiers,
        "selected_namespace": selected_namespace,
        "selected_tier": selected_tier,
        "deferred_loading_active": deferred_loading_active,
        "full_tool_exposure": full_exposure,
        "tools": tools
    })
}

fn analysis_summary_payload(artifact: &crate::state::AnalysisArtifact) -> serde_json::Value {
    let verifier_checks = if artifact.verifier_checks.is_empty() {
        vec![
            json!({
                "check": "diagnostic_verifier",
                "status": artifact.readiness.diagnostics_ready,
                "summary": "Refresh diagnostics evidence before trusting a reused artifact.",
                "evidence_section": null,
            }),
            json!({
                "check": "reference_verifier",
                "status": artifact.readiness.reference_safety,
                "summary": "Refresh reference evidence before mutating reused analysis targets.",
                "evidence_section": null,
            }),
            json!({
                "check": "test_readiness_verifier",
                "status": artifact.readiness.test_readiness,
                "summary": "Refresh test-readiness evidence before relying on a reused artifact.",
                "evidence_section": null,
            }),
            json!({
                "check": "mutation_readiness_verifier",
                "status": artifact.readiness.mutation_ready,
                "summary": if artifact.blockers.is_empty() {
                    "Reused artifact needs fresh verifier evidence before mutation."
                } else {
                    "Blockers remain on the reused artifact; refresh evidence before mutation."
                },
                "evidence_section": null,
            }),
        ]
    } else {
        artifact
            .verifier_checks
            .iter()
            .map(|check| {
                json!({
                    "check": check.check,
                    "status": check.status,
                    "summary": check.summary,
                    "evidence_section": check.evidence_section,
                })
            })
            .collect::<Vec<_>>()
    };
    let quality_focus = infer_summary_quality_focus(
        &artifact.tool_name,
        &artifact.summary,
        &artifact.top_findings,
    );
    let recommended_checks = infer_summary_recommended_checks(
        &artifact.tool_name,
        &artifact.summary,
        &artifact.top_findings,
        &artifact.next_actions,
        &artifact.available_sections,
    );
    let performance_watchpoints = infer_summary_performance_watchpoints(
        &artifact.summary,
        &artifact.top_findings,
        &artifact.next_actions,
    );
    let mut payload = json!({
        "analysis_id": artifact.id,
        "tool_name": artifact.tool_name,
        "surface": artifact.surface,
        "summary": artifact.summary,
        "top_findings": artifact.top_findings,
        "risk_level": artifact.risk_level,
        "confidence": artifact.confidence,
        "next_actions": artifact.next_actions,
        "blockers": artifact.blockers,
        "blocker_count": artifact.blockers.len(),
        "readiness": artifact.readiness,
        "verifier_checks": verifier_checks,
        "quality_focus": quality_focus,
        "recommended_checks": recommended_checks,
        "performance_watchpoints": performance_watchpoints,
        "available_sections": artifact.available_sections,
        "created_at_ms": artifact.created_at_ms,
    });
    if artifact.surface == "ci-audit" {
        payload["schema_version"] = json!("codelens-ci-audit-v1");
        payload["report_kind"] = json!(artifact.tool_name);
        payload["profile"] = json!("ci-audit");
        payload["machine_summary"] = json!({
            "finding_count": artifact.top_findings.len(),
            "next_action_count": artifact.next_actions.len(),
            "section_count": artifact.available_sections.len(),
            "blocker_count": artifact.blockers.len(),
            "verifier_check_count": payload["verifier_checks"].as_array().map(|v| v.len()).unwrap_or(0),
            "ready_check_count": payload["verifier_checks"].as_array().map(|checks| checks.iter().filter(|check| check.get("status").and_then(|value| value.as_str()) == Some("ready")).count()).unwrap_or(0),
            "blocked_check_count": payload["verifier_checks"].as_array().map(|checks| checks.iter().filter(|check| check.get("status").and_then(|value| value.as_str()) == Some("blocked")).count()).unwrap_or(0),
            "quality_focus_count": payload["quality_focus"].as_array().map(|v| v.len()).unwrap_or(0),
            "recommended_check_count": payload["recommended_checks"].as_array().map(|v| v.len()).unwrap_or(0),
            "performance_watchpoint_count": payload["performance_watchpoints"].as_array().map(|v| v.len()).unwrap_or(0),
        });
        payload["evidence_handles"] = json!(
            artifact
                .available_sections
                .iter()
                .map(|section| json!({
                    "section": section,
                    "uri": format!("codelens://analysis/{}/{section}", artifact.id),
                }))
                .collect::<Vec<_>>()
        );
    }
    payload
}

fn infer_summary_quality_focus(
    tool_name: &str,
    summary: &str,
    top_findings: &[String],
) -> Vec<String> {
    let combined = format!("{} {}", summary, top_findings.join(" ")).to_ascii_lowercase();
    let mut focus = Vec::new();
    let mut push_unique = |value: &str| {
        if !focus.iter().any(|existing| existing == value) {
            focus.push(value.to_owned());
        }
    };

    push_unique("correctness");
    if matches!(
        tool_name,
        "analyze_change_request"
            | "verify_change_readiness"
            | "impact_report"
            | "refactor_safety_report"
            | "safe_rename_report"
            | "unresolved_reference_check"
    ) {
        push_unique("regression_safety");
    }
    if combined.contains("http")
        || combined.contains("browser")
        || combined.contains("ui")
        || combined.contains("render")
        || combined.contains("frontend")
        || combined.contains("layout")
    {
        push_unique("user_experience");
    }
    if combined.contains("coupling")
        || combined.contains("circular")
        || combined.contains("refactor")
        || combined.contains("boundary")
    {
        push_unique("maintainability");
    }
    if combined.contains("search")
        || combined.contains("embedding")
        || combined.contains("watch")
        || combined.contains("latency")
        || combined.contains("performance")
    {
        push_unique("performance");
    }
    focus
}

fn infer_summary_recommended_checks(
    tool_name: &str,
    summary: &str,
    top_findings: &[String],
    next_actions: &[String],
    available_sections: &[String],
) -> Vec<String> {
    let combined = format!(
        "{} {} {} {}",
        tool_name,
        summary,
        top_findings.join(" "),
        next_actions.join(" ")
    )
    .to_ascii_lowercase();
    let mut checks = Vec::new();
    let mut push_unique = |value: &str| {
        if !checks.iter().any(|existing| existing == value) {
            checks.push(value.to_owned());
        }
    };

    push_unique("run targeted tests for affected files or symbols");
    push_unique("run diagnostics or lint on touched files before finalizing");

    if available_sections
        .iter()
        .any(|section| section == "related_tests")
    {
        push_unique("expand related_tests and execute the highest-signal subset");
    }
    if combined.contains("rename") || combined.contains("refactor") {
        push_unique("verify references and call sites after the refactor preview");
    }
    if combined.contains("http")
        || combined.contains("browser")
        || combined.contains("ui")
        || combined.contains("frontend")
        || combined.contains("layout")
        || combined.contains("render")
    {
        push_unique("exercise the user-facing flow in a browser or UI harness");
    }
    if combined.contains("search")
        || combined.contains("embedding")
        || combined.contains("latency")
        || combined.contains("performance")
    {
        push_unique("compare hot-path latency or throughput before and after the change");
    }
    if combined.contains("dead code") || combined.contains("delete") {
        push_unique("confirm the candidate is unused in tests, runtime paths, and CI scripts");
    }
    checks
}

fn infer_summary_performance_watchpoints(
    summary: &str,
    top_findings: &[String],
    next_actions: &[String],
) -> Vec<String> {
    let combined = format!(
        "{} {} {}",
        summary,
        top_findings.join(" "),
        next_actions.join(" ")
    )
    .to_ascii_lowercase();
    let mut watchpoints = Vec::new();
    let mut push_unique = |value: &str| {
        if !watchpoints.iter().any(|existing| existing == value) {
            watchpoints.push(value.to_owned());
        }
    };

    if combined.contains("search") || combined.contains("embedding") || combined.contains("query") {
        push_unique("watch ranking quality, latency, and cache-hit behavior on search paths");
    }
    if combined.contains("http") || combined.contains("server") || combined.contains("route") {
        push_unique("watch request latency, concurrency, and error-rate changes on hot routes");
    }
    if combined.contains("watch") || combined.contains("filesystem") {
        push_unique("watch background work, queue depth, and repeated invalidation behavior");
    }
    if combined.contains("ui")
        || combined.contains("frontend")
        || combined.contains("layout")
        || combined.contains("render")
        || combined.contains("browser")
    {
        push_unique("watch rendering smoothness, layout stability, and unnecessary re-renders");
    }
    watchpoints
}

pub(crate) fn resources(state: &AppState) -> Vec<serde_json::Value> {
    let project_name = state
        .project()
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut items = vec![
        json!({
            "uri": "codelens://project/overview",
            "name": format!("Project: {project_name}"),
            "description": "Compressed project overview with active surface and index status",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://project/architecture",
            "name": "Project Architecture",
            "description": "High-level architecture summary for harness planning",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://tools/list",
            "name": "Visible Tool Surface",
            "description": "Compressed role-aware tool surface summary",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://tools/list/full",
            "name": "Visible Tool Surface (Full)",
            "description": "Expanded role-aware tool surface with descriptions",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://stats/token-efficiency",
            "name": "Token Efficiency Stats",
            "description": "Session-level token, chain, and handle reuse metrics",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://session/http",
            "name": "HTTP Session Runtime",
            "description": "Shared daemon session counts, timeout, and resume support",
            "mimeType": "application/json"
        }),
    ];

    for profile in [
        ToolProfile::PlannerReadonly,
        ToolProfile::BuilderMinimal,
        ToolProfile::ReviewerGraph,
        ToolProfile::EvaluatorCompact,
        ToolProfile::RefactorFull,
        ToolProfile::CiAudit,
    ] {
        items.push(json!({
            "uri": format!("codelens://profile/{}/guide", profile.as_str()),
            "name": format!("Profile Guide: {}", profile.as_str()),
            "description": "Compressed role profile guide",
            "mimeType": "application/json"
        }));
        items.push(json!({
            "uri": format!("codelens://profile/{}/guide/full", profile.as_str()),
            "name": format!("Profile Guide (Full): {}", profile.as_str()),
            "description": "Expanded role profile guide with anti-patterns",
            "mimeType": "application/json"
        }));
    }

    for artifact in state.list_analysis_summaries() {
        items.push(json!({
            "uri": format!("codelens://analysis/{}/summary", artifact.id),
            "name": format!("Analysis: {}", artifact.tool_name),
            "description": format!("{} ({})", artifact.summary, artifact.surface),
            "mimeType": "application/json"
        }));
    }

    items
}

pub(crate) fn read_resource(
    state: &AppState,
    uri: &str,
    params: Option<&serde_json::Value>,
) -> serde_json::Value {
    match uri {
        "codelens://project/overview" => {
            let stats = state.symbol_index().stats().ok();
            let surface = *state.surface();
            let visible = visible_tools(surface);
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&json!({
                        "project_root": state.project().as_path().to_string_lossy(),
                        "active_surface": surface.as_label(),
                        "daemon_mode": state.daemon_mode().as_str(),
                        "visible_tool_count": visible.len(),
                        "symbol_index": stats,
                        "memories_dir": state.memories_dir().to_string_lossy(),
                    })).unwrap_or_default()
                }]
            })
        }
        "codelens://project/architecture" => {
            let stats = state.symbol_index().stats().ok();
            let frameworks = detect_frameworks(state.project().as_path());
            let workspace_packages = detect_workspace_packages(state.project().as_path());
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&json!({
                        "active_surface": state.surface().as_label(),
                        "daemon_mode": state.daemon_mode().as_str(),
                        "frameworks": frameworks,
                        "workspace_packages": workspace_packages,
                        "indexed_files": stats.as_ref().map(|s| s.indexed_files).unwrap_or(0),
                        "stale_files": stats.as_ref().map(|s| s.stale_files).unwrap_or(0),
                        "notes": [
                            "Use composite workflow tools first to keep model-visible context bounded.",
                            "Prefer HTTP + role profiles for multi-agent harnesses."
                        ]
                    })).unwrap_or_default()
                }]
            })
        }
        "codelens://tools/list" => {
            if deferred_loading_requested(params)
                && (resource_requested_namespace(params).is_some()
                    || resource_requested_tier(params).is_some())
            {
                state.metrics().record_deferred_namespace_expansion();
            }
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&visible_tool_summary(state, uri, params)).unwrap_or_default()
                }]
            })
        }
        "codelens://tools/list/full" => {
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&visible_tool_details(state, uri, params)).unwrap_or_default()
                }]
            })
        }
        "codelens://stats/token-efficiency" => {
            let session = state.metrics().session_snapshot();
            let handle_reads = session.analysis_summary_reads + session.analysis_section_reads;
            let watcher_stats = state.watcher.as_ref().map(|watcher| watcher.stats());
            let watcher_failure_health = state.watcher_failure_health();
            let derived = json!({
                "truncation_followup_rate": if session.truncated_response_count > 0 {
                    session.truncation_followup_count as f64 / session.truncated_response_count as f64
                } else { 0.0 },
                "composite_guidance_followthrough_rate": if session.composite_guidance_emitted_count > 0 {
                    session.composite_guidance_followed_count as f64 / session.composite_guidance_emitted_count as f64
                } else { 0.0 },
                "analysis_cache_hit_rate": if session.composite_calls > 0 {
                    session.analysis_cache_hit_count as f64 / session.composite_calls as f64
                } else { 0.0 },
                "quality_contract_present_rate": if session.composite_calls > 0 {
                    session.quality_contract_emitted_count as f64 / session.composite_calls as f64
                } else { 0.0 },
                "recommended_check_followthrough_rate": if session.quality_contract_emitted_count > 0 {
                    session.recommended_check_followthrough_count as f64 / session.quality_contract_emitted_count as f64
                } else { 0.0 },
                "quality_focus_reuse_rate": if session.handle_reuse_count > 0 {
                    session.quality_focus_reuse_count as f64 / session.handle_reuse_count as f64
                } else { 0.0 },
                "performance_watchpoint_emit_rate": if session.quality_contract_emitted_count > 0 {
                    session.performance_watchpoint_emit_count as f64 / session.quality_contract_emitted_count as f64
                } else { 0.0 },
                "verifier_contract_present_rate": if session.composite_calls > 0 {
                    session.verifier_contract_emitted_count as f64 / session.composite_calls as f64
                } else { 0.0 },
                "blocker_emit_rate": if session.verifier_contract_emitted_count > 0 {
                    session.blocker_emit_count as f64 / session.verifier_contract_emitted_count as f64
                } else { 0.0 },
                "verifier_followthrough_rate": if session.verifier_contract_emitted_count > 0 {
                    session.verifier_followthrough_count as f64 / session.verifier_contract_emitted_count as f64
                } else { 0.0 },
                "mutation_preflight_gate_deny_rate": if session.mutation_preflight_checked_count > 0 {
                    session.mutation_preflight_gate_denied_count as f64
                        / session.mutation_preflight_checked_count as f64
                } else { 0.0 },
                "deferred_hidden_tool_call_deny_rate": if session.deferred_namespace_expansion_count > 0 {
                    session.deferred_hidden_tool_call_denied_count as f64
                        / session.deferred_namespace_expansion_count as f64
                } else { 0.0 },
                "watcher_lock_contention_rate": if watcher_stats
                    .as_ref()
                    .map(|stats| stats.events_processed)
                    .unwrap_or(0)
                    > 0
                {
                    watcher_stats
                        .as_ref()
                        .map(|stats| stats.lock_contention_batches as f64 / stats.events_processed as f64)
                        .unwrap_or(0.0)
                } else { 0.0 },
                "watcher_recent_failure_share": if watcher_failure_health.total_failures > 0 {
                    watcher_failure_health.recent_failures as f64 / watcher_failure_health.total_failures as f64
                } else { 0.0 },
                "handle_reuse_rate": if handle_reads > 0 {
                    session.handle_reuse_count as f64 / handle_reads as f64
                } else { 0.0 }
            });
            let mut stats = serde_json::Map::new();
            stats.insert(
                "active_http_sessions".to_owned(),
                json!(state.active_session_count()),
            );
            stats.insert(
                "session_resume_supported".to_owned(),
                json!(state.session_resume_supported()),
            );
            stats.insert(
                "session_timeout_seconds".to_owned(),
                json!(state.session_timeout_seconds()),
            );
            stats.insert(
                "watcher_running".to_owned(),
                json!(
                    watcher_stats
                        .as_ref()
                        .map(|stats| stats.running)
                        .unwrap_or(false)
                ),
            );
            stats.insert(
                "watcher_events_processed".to_owned(),
                json!(
                    watcher_stats
                        .as_ref()
                        .map(|stats| stats.events_processed)
                        .unwrap_or(0)
                ),
            );
            stats.insert(
                "watcher_files_reindexed".to_owned(),
                json!(
                    watcher_stats
                        .as_ref()
                        .map(|stats| stats.files_reindexed)
                        .unwrap_or(0)
                ),
            );
            stats.insert(
                "watcher_lock_contention_batches".to_owned(),
                json!(
                    watcher_stats
                        .as_ref()
                        .map(|stats| stats.lock_contention_batches)
                        .unwrap_or(0)
                ),
            );
            stats.insert(
                "watcher_index_failures".to_owned(),
                json!(watcher_failure_health.recent_failures),
            );
            stats.insert(
                "watcher_index_failures_total".to_owned(),
                json!(watcher_failure_health.total_failures),
            );
            stats.insert(
                "watcher_stale_index_failures".to_owned(),
                json!(watcher_failure_health.stale_failures),
            );
            stats.insert(
                "watcher_persistent_index_failures".to_owned(),
                json!(watcher_failure_health.persistent_failures),
            );
            stats.insert(
                "watcher_pruned_missing_failures".to_owned(),
                json!(watcher_failure_health.pruned_missing_failures),
            );
            stats.insert(
                "watcher_recent_failure_window_seconds".to_owned(),
                json!(watcher_failure_health.recent_window_seconds),
            );
            stats.insert(
                "tools_list_tokens".to_owned(),
                json!(session.tools_list_tokens),
            );
            stats.insert(
                "avg_tool_output_tokens".to_owned(),
                json!(if session.total_calls > 0 {
                    session.total_tokens / session.total_calls as usize
                } else {
                    0
                }),
            );
            stats.insert(
                "p95_tool_latency_ms".to_owned(),
                json!(crate::telemetry::percentile_95(&session.latency_samples)),
            );
            for (key, value) in [
                ("retry_count", json!(session.retry_count)),
                (
                    "analysis_cache_hit_count",
                    json!(session.analysis_cache_hit_count),
                ),
                (
                    "truncated_response_count",
                    json!(session.truncated_response_count),
                ),
                (
                    "truncation_followup_count",
                    json!(session.truncation_followup_count),
                ),
                (
                    "truncation_same_tool_retry_count",
                    json!(session.truncation_same_tool_retry_count),
                ),
                (
                    "truncation_handle_followup_count",
                    json!(session.truncation_handle_followup_count),
                ),
                ("handle_reuse_count", json!(session.handle_reuse_count)),
                (
                    "repeated_low_level_chain_count",
                    json!(session.repeated_low_level_chain_count),
                ),
                (
                    "composite_guidance_emitted_count",
                    json!(session.composite_guidance_emitted_count),
                ),
                (
                    "composite_guidance_followed_count",
                    json!(session.composite_guidance_followed_count),
                ),
                (
                    "quality_contract_emitted_count",
                    json!(session.quality_contract_emitted_count),
                ),
                (
                    "recommended_checks_emitted_count",
                    json!(session.recommended_checks_emitted_count),
                ),
                (
                    "recommended_check_followthrough_count",
                    json!(session.recommended_check_followthrough_count),
                ),
                (
                    "quality_focus_reuse_count",
                    json!(session.quality_focus_reuse_count),
                ),
                (
                    "performance_watchpoint_emit_count",
                    json!(session.performance_watchpoint_emit_count),
                ),
                (
                    "verifier_contract_emitted_count",
                    json!(session.verifier_contract_emitted_count),
                ),
                ("blocker_emit_count", json!(session.blocker_emit_count)),
                (
                    "verifier_followthrough_count",
                    json!(session.verifier_followthrough_count),
                ),
                (
                    "mutation_preflight_checked_count",
                    json!(session.mutation_preflight_checked_count),
                ),
                (
                    "mutation_without_preflight_count",
                    json!(session.mutation_without_preflight_count),
                ),
                (
                    "mutation_preflight_gate_denied_count",
                    json!(session.mutation_preflight_gate_denied_count),
                ),
                (
                    "stale_preflight_reject_count",
                    json!(session.stale_preflight_reject_count),
                ),
                (
                    "mutation_with_caution_count",
                    json!(session.mutation_with_caution_count),
                ),
                (
                    "rename_without_symbol_preflight_count",
                    json!(session.rename_without_symbol_preflight_count),
                ),
                (
                    "deferred_namespace_expansion_count",
                    json!(session.deferred_namespace_expansion_count),
                ),
                (
                    "deferred_hidden_tool_call_denied_count",
                    json!(session.deferred_hidden_tool_call_denied_count),
                ),
                ("composite_calls", json!(session.composite_calls)),
                ("low_level_calls", json!(session.low_level_calls)),
                ("stdio_session_count", json!(session.stdio_session_count)),
                ("http_session_count", json!(session.http_session_count)),
                (
                    "analysis_jobs_enqueued",
                    json!(session.analysis_jobs_enqueued),
                ),
                (
                    "analysis_jobs_started",
                    json!(session.analysis_jobs_started),
                ),
                (
                    "analysis_jobs_completed",
                    json!(session.analysis_jobs_completed),
                ),
                ("analysis_jobs_failed", json!(session.analysis_jobs_failed)),
                (
                    "analysis_jobs_cancelled",
                    json!(session.analysis_jobs_cancelled),
                ),
                ("analysis_queue_depth", json!(session.analysis_queue_depth)),
                (
                    "analysis_queue_max_depth",
                    json!(session.analysis_queue_max_depth),
                ),
                (
                    "analysis_queue_weighted_depth",
                    json!(session.analysis_queue_weighted_depth),
                ),
                (
                    "analysis_queue_max_weighted_depth",
                    json!(session.analysis_queue_max_weighted_depth),
                ),
                (
                    "analysis_queue_priority_promotions",
                    json!(session.analysis_queue_priority_promotions),
                ),
                (
                    "active_analysis_workers",
                    json!(session.active_analysis_workers),
                ),
                (
                    "peak_active_analysis_workers",
                    json!(session.peak_active_analysis_workers),
                ),
                (
                    "analysis_worker_limit",
                    json!(session.analysis_worker_limit),
                ),
                ("analysis_cost_budget", json!(session.analysis_cost_budget)),
                (
                    "analysis_transport_mode",
                    json!(session.analysis_transport_mode.clone()),
                ),
                ("daemon_mode", json!(state.daemon_mode().as_str())),
            ] {
                stats.insert(key.to_owned(), value);
            }
            stats.insert("derived_kpis".to_owned(), derived);
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&stats).unwrap_or_default()
                }]
            })
        }
        "codelens://session/http" => {
            let loaded_namespaces = loaded_namespaces(params);
            let loaded_tiers = loaded_tiers(params);
            let full_tool_exposure = full_tool_exposure(params);
            let payload = json!({
                "enabled": state.session_resume_supported(),
                "active_sessions": state.active_session_count(),
                "timeout_seconds": state.session_timeout_seconds(),
                "resume_supported": state.session_resume_supported(),
                "daemon_mode": state.daemon_mode().as_str(),
                "active_surface": state.surface().as_label(),
                "deferred_loading_supported": true,
                "loaded_namespaces": loaded_namespaces,
                "loaded_tiers": loaded_tiers,
                "full_tool_exposure": full_tool_exposure,
                "deferred_namespace_gate": true,
                "deferred_tier_gate": true,
                "preferred_namespaces": preferred_namespaces(*state.surface()),
                "preferred_tiers": preferred_tier_labels(*state.surface()),
                "trusted_client_hook": true,
                "mutation_requires_trusted_client": matches!(
                    state.daemon_mode(),
                    crate::state::RuntimeDaemonMode::MutationEnabled
                ),
                "mutation_preflight_required": matches!(
                    *state.surface(),
                    crate::tool_defs::ToolSurface::Profile(ToolProfile::RefactorFull)
                ),
                "preflight_ttl_seconds": state.preflight_ttl_seconds(),
                "rename_requires_symbol_preflight": true,
                "requires_namespace_listing_before_tool_call": true,
                "requires_tier_listing_before_tool_call": true
            });
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&payload).unwrap_or_default()
                }]
            })
        }
        _ if uri.starts_with("codelens://profile/") && uri.ends_with("/guide") => {
            let profile_name = uri
                .trim_start_matches("codelens://profile/")
                .trim_end_matches("/guide");
            let profile = ToolProfile::from_str(profile_name);
            let body = profile
                .map(profile_guide_summary)
                .unwrap_or_else(|| json!({"error": format!("Unknown profile `{profile_name}`")}));
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&body).unwrap_or_default()
                }]
            })
        }
        _ if uri.starts_with("codelens://profile/") && uri.ends_with("/guide/full") => {
            let profile_name = uri
                .trim_start_matches("codelens://profile/")
                .trim_end_matches("/guide/full");
            let profile = ToolProfile::from_str(profile_name);
            let body = profile
                .map(profile_guide)
                .unwrap_or_else(|| json!({"error": format!("Unknown profile `{profile_name}`")}));
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&body).unwrap_or_default()
                }]
            })
        }
        _ if uri.starts_with("codelens://analysis/") => {
            let trimmed = uri.trim_start_matches("codelens://analysis/");
            let mut parts = trimmed.splitn(2, '/');
            let analysis_id = parts.next().unwrap_or_default();
            let section = parts.next().unwrap_or("summary");
            if let Some(artifact) = state.get_analysis(analysis_id) {
                let content = if section == "summary" {
                    state.metrics().record_analysis_read(false);
                    analysis_summary_payload(&artifact)
                } else {
                    state
                        .get_analysis_section(analysis_id, section)
                        .unwrap_or_else(
                            |_| json!({"error": format!("Unknown section `{section}`")}),
                        )
                };
                json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string_pretty(&content).unwrap_or_default()
                    }]
                })
            } else {
                json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string_pretty(&json!({"error": format!("Unknown analysis `{analysis_id}`")})).unwrap_or_default()
                    }]
                })
            }
        }
        _ => json!({
            "contents": [{
                "uri": uri,
                "mimeType": "text/plain",
                "text": format!("Unknown resource: {uri}")
            }]
        }),
    }
}
