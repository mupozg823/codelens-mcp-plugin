//! MCP resource definitions and handlers.

use crate::AppState;
use crate::tool_defs::{ToolProfile, preferred_namespaces, tool_namespace, visible_tools};
use codelens_core::{detect_frameworks, detect_workspace_packages};
use serde_json::json;
use std::collections::BTreeMap;

fn profile_guide(profile: ToolProfile) -> serde_json::Value {
    match profile {
        ToolProfile::PlannerReadonly => json!({
            "profile": profile.as_str(),
            "intent": "Use bounded, read-only analysis to plan changes and rank context before implementation.",
            "preferred_tools": ["analyze_change_request", "find_minimal_context_for_change", "impact_report", "module_boundary_report"],
            "preferred_namespaces": ["reports", "symbols", "graph", "filesystem", "session"],
            "avoid": ["rename_symbol", "replace_content", "raw graph expansion unless necessary"]
        }),
        ToolProfile::BuilderMinimal => json!({
            "profile": profile.as_str(),
            "intent": "Keep the visible surface small while implementing changes with only the essential symbol and edit tools.",
            "preferred_tools": ["find_symbol", "get_symbols_overview", "analyze_missing_imports", "add_import"],
            "preferred_namespaces": ["symbols", "filesystem", "session"],
            "avoid": ["dead-code audits", "full-graph exploration", "broad multi-project search"]
        }),
        ToolProfile::ReviewerGraph => json!({
            "profile": profile.as_str(),
            "intent": "Review risky changes with graph-aware, read-only evidence.",
            "preferred_tools": ["impact_report", "diff_aware_references", "module_boundary_report", "dead_code_report"],
            "preferred_namespaces": ["reports", "graph", "symbols", "session"],
            "avoid": ["mutation tools"]
        }),
        ToolProfile::RefactorFull => json!({
            "profile": profile.as_str(),
            "intent": "Run high-safety refactors after planning and review have narrowed the target surface.",
            "preferred_tools": ["refactor_safety_report", "safe_rename_report", "rename_symbol", "replace_symbol_body"],
            "preferred_namespaces": ["reports", "mutation", "symbols", "session"],
            "avoid": ["broad edits without diagnostics or preview"]
        }),
        ToolProfile::CiAudit => json!({
            "profile": profile.as_str(),
            "intent": "Produce machine-friendly review output around diffs, impact, dead code, and structural risk.",
            "preferred_tools": ["get_changed_files", "impact_report", "diff_aware_references", "dead_code_report"],
            "preferred_namespaces": ["reports", "graph", "session"],
            "avoid": ["interactive mutation flows"]
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
    })
}

fn visible_tool_summary(state: &AppState) -> serde_json::Value {
    let surface = *state.surface();
    let tools = visible_tools(surface);
    let mut namespace_counts = BTreeMap::new();
    for tool in &tools {
        *namespace_counts
            .entry(tool_namespace(tool.name).to_owned())
            .or_insert(0usize) += 1;
    }
    let prioritized = tools
        .iter()
        .take(8)
        .map(|tool| {
            json!({
                "name": tool.name,
                "namespace": tool_namespace(tool.name),
                "tier": tool
                    .annotations
                    .as_ref()
                    .and_then(|ann| ann.tier.as_ref())
                    .map(|tier| format!("{tier:?}"))
                    .unwrap_or_default()
            })
        })
        .collect::<Vec<_>>();
    json!({
        "active_surface": surface.as_label(),
        "tool_count": tools.len(),
        "visible_namespaces": namespace_counts,
        "preferred_namespaces": preferred_namespaces(surface),
        "recommended_tools": prioritized,
        "note": "Read `codelens://tools/list/full` only when summary is insufficient."
    })
}

fn visible_tool_details(state: &AppState) -> serde_json::Value {
    let surface = *state.surface();
    let tools = visible_tools(surface)
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "namespace": tool_namespace(tool.name),
                "description": tool.description,
                "tier": tool.annotations.as_ref().and_then(|ann| ann.tier.as_ref()).map(|tier| format!("{tier:?}")).unwrap_or_default()
            })
        })
        .collect::<Vec<_>>();
    json!({
        "active_surface": surface.as_label(),
        "tool_count": tools.len(),
        "tools": tools
    })
}

fn analysis_summary_payload(artifact: &crate::state::AnalysisArtifact) -> serde_json::Value {
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
        "analyze_change_request" | "impact_report" | "refactor_safety_report" | "safe_rename_report"
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

    if available_sections.iter().any(|section| section == "related_tests") {
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

pub(crate) fn read_resource(state: &AppState, uri: &str) -> serde_json::Value {
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
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&visible_tool_summary(state)).unwrap_or_default()
                }]
            })
        }
        "codelens://tools/list/full" => {
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&visible_tool_details(state)).unwrap_or_default()
                }]
            })
        }
        "codelens://stats/token-efficiency" => {
            let session = state.metrics().session_snapshot();
            let handle_reads = session.analysis_summary_reads + session.analysis_section_reads;
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
                "handle_reuse_rate": if handle_reads > 0 {
                    session.handle_reuse_count as f64 / handle_reads as f64
                } else { 0.0 }
            });
            let mut stats = serde_json::Map::new();
            stats.insert("active_http_sessions".to_owned(), json!(state.active_session_count()));
            stats.insert(
                "session_resume_supported".to_owned(),
                json!(state.session_resume_supported()),
            );
            stats.insert(
                "session_timeout_seconds".to_owned(),
                json!(state.session_timeout_seconds()),
            );
            stats.insert("tools_list_tokens".to_owned(), json!(session.tools_list_tokens));
            stats.insert(
                "avg_tool_output_tokens".to_owned(),
                json!(if session.total_calls > 0 {
                    session.total_tokens / session.total_calls as usize
                } else { 0 }),
            );
            stats.insert(
                "p95_tool_latency_ms".to_owned(),
                json!(crate::telemetry::percentile_95(&session.latency_samples)),
            );
            for (key, value) in [
                ("retry_count", json!(session.retry_count)),
                ("analysis_cache_hit_count", json!(session.analysis_cache_hit_count)),
                ("truncated_response_count", json!(session.truncated_response_count)),
                ("truncation_followup_count", json!(session.truncation_followup_count)),
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
                ("quality_focus_reuse_count", json!(session.quality_focus_reuse_count)),
                (
                    "performance_watchpoint_emit_count",
                    json!(session.performance_watchpoint_emit_count),
                ),
                ("composite_calls", json!(session.composite_calls)),
                ("low_level_calls", json!(session.low_level_calls)),
                ("stdio_session_count", json!(session.stdio_session_count)),
                ("http_session_count", json!(session.http_session_count)),
                ("analysis_jobs_enqueued", json!(session.analysis_jobs_enqueued)),
                ("analysis_jobs_started", json!(session.analysis_jobs_started)),
                ("analysis_jobs_completed", json!(session.analysis_jobs_completed)),
                ("analysis_jobs_failed", json!(session.analysis_jobs_failed)),
                ("analysis_jobs_cancelled", json!(session.analysis_jobs_cancelled)),
                ("analysis_queue_depth", json!(session.analysis_queue_depth)),
                ("analysis_queue_max_depth", json!(session.analysis_queue_max_depth)),
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
                ("active_analysis_workers", json!(session.active_analysis_workers)),
                (
                    "peak_active_analysis_workers",
                    json!(session.peak_active_analysis_workers),
                ),
                ("analysis_worker_limit", json!(session.analysis_worker_limit)),
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
            let payload = json!({
                "enabled": state.session_resume_supported(),
                "active_sessions": state.active_session_count(),
                "timeout_seconds": state.session_timeout_seconds(),
                "resume_supported": state.session_resume_supported(),
                "daemon_mode": state.daemon_mode().as_str(),
                "active_surface": state.surface().as_label(),
                "deferred_loading_supported": true,
                "preferred_namespaces": preferred_namespaces(*state.surface()),
                "trusted_client_hook": true,
                "mutation_requires_trusted_client": matches!(
                    state.daemon_mode(),
                    crate::state::RuntimeDaemonMode::MutationEnabled
                )
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
