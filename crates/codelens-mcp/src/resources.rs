//! MCP resource definitions and handlers.

use crate::AppState;
use crate::resource_context::{
    ResourceRequestContext, build_http_session_payload, build_visible_tool_context,
};
use crate::session_metrics_payload::build_session_metrics_payload;
use crate::tool_defs::{
    ToolProfile, preferred_tier_labels, tool_namespace, tool_tier_label, visible_tools,
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

fn visible_tool_summary(
    state: &AppState,
    uri: &str,
    params: Option<&serde_json::Value>,
) -> serde_json::Value {
    let surface = *state.surface();
    let request = ResourceRequestContext::from_request(uri, params);
    let context = build_visible_tool_context(state, &request);
    let mut namespace_counts = BTreeMap::new();
    let mut tier_counts = BTreeMap::new();
    for tool in &context.tools {
        *namespace_counts
            .entry(tool_namespace(tool.name).to_owned())
            .or_insert(0usize) += 1;
        *tier_counts
            .entry(tool_tier_label(tool.name).to_owned())
            .or_insert(0usize) += 1;
    }
    let prioritized = context
        .tools
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
    json!({
        "active_surface": surface.as_label(),
        "tool_count": context.tools.len(),
        "tool_count_total": context.total_tool_count,
        "visible_namespaces": namespace_counts,
        "visible_tiers": tier_counts,
        "all_namespaces": context.all_namespaces,
        "all_tiers": context.all_tiers,
        "preferred_namespaces": context.preferred_namespaces,
        "preferred_tiers": context.preferred_tiers,
        "loaded_namespaces": context.loaded_namespaces,
        "loaded_tiers": context.loaded_tiers,
        "effective_namespaces": context.effective_namespaces,
        "effective_tiers": context.effective_tiers,
        "selected_namespace": context.selected_namespace,
        "selected_tier": context.selected_tier,
        "deferred_loading_active": context.deferred_loading_active,
        "full_tool_exposure": context.full_tool_exposure,
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
    let request = ResourceRequestContext::from_request(uri, params);
    let context = build_visible_tool_context(state, &request);
    let tools = context
        .tools
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
    json!({
        "active_surface": surface.as_label(),
        "tool_count": tools.len(),
        "tool_count_total": context.total_tool_count,
        "all_namespaces": context.all_namespaces,
        "all_tiers": context.all_tiers,
        "preferred_namespaces": context.preferred_namespaces,
        "preferred_tiers": context.preferred_tiers,
        "loaded_namespaces": context.loaded_namespaces,
        "loaded_tiers": context.loaded_tiers,
        "effective_namespaces": context.effective_namespaces,
        "effective_tiers": context.effective_tiers,
        "selected_namespace": context.selected_namespace,
        "selected_tier": context.selected_tier,
        "deferred_loading_active": context.deferred_loading_active,
        "full_tool_exposure": context.full_tool_exposure,
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
            let request = ResourceRequestContext::from_request(uri, params);
            if request.deferred_loading_requested
                && (request.requested_namespace.is_some() || request.requested_tier.is_some())
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
            let metrics_payload = build_session_metrics_payload(state);
            let mut stats = metrics_payload.session;
            stats.insert("derived_kpis".to_owned(), metrics_payload.derived_kpis);
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&stats).unwrap_or_default()
                }]
            })
        }
        "codelens://session/http" => {
            let request = ResourceRequestContext::from_request(uri, params);
            let payload = build_http_session_payload(state, &request);
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
