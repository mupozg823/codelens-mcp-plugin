//! MCP resource definitions and handlers.

use crate::tool_defs::{visible_tools, ToolProfile};
use crate::AppState;
use codelens_core::{detect_frameworks, detect_workspace_packages};
use serde_json::json;

fn profile_guide(profile: ToolProfile) -> serde_json::Value {
    match profile {
        ToolProfile::PlannerReadonly => json!({
            "profile": profile.as_str(),
            "intent": "Use bounded, read-only analysis to plan changes and rank context before implementation.",
            "preferred_tools": ["analyze_change_request", "find_minimal_context_for_change", "impact_report", "module_boundary_report"],
            "avoid": ["rename_symbol", "replace_content", "raw graph expansion unless necessary"]
        }),
        ToolProfile::BuilderMinimal => json!({
            "profile": profile.as_str(),
            "intent": "Keep the visible surface small while implementing changes with only the essential symbol and edit tools.",
            "preferred_tools": ["find_symbol", "get_symbols_overview", "analyze_missing_imports", "add_import"],
            "avoid": ["dead-code audits", "full-graph exploration", "broad multi-project search"]
        }),
        ToolProfile::ReviewerGraph => json!({
            "profile": profile.as_str(),
            "intent": "Review risky changes with graph-aware, read-only evidence.",
            "preferred_tools": ["impact_report", "diff_aware_references", "module_boundary_report", "dead_code_report"],
            "avoid": ["mutation tools"]
        }),
        ToolProfile::RefactorFull => json!({
            "profile": profile.as_str(),
            "intent": "Run high-safety refactors after planning and review have narrowed the target surface.",
            "preferred_tools": ["refactor_safety_report", "safe_rename_report", "rename_symbol", "replace_symbol_body"],
            "avoid": ["broad edits without diagnostics or preview"]
        }),
        ToolProfile::CiAudit => json!({
            "profile": profile.as_str(),
            "intent": "Produce machine-friendly review output around diffs, impact, dead code, and structural risk.",
            "preferred_tools": ["get_changed_files", "impact_report", "diff_aware_references", "dead_code_report"],
            "avoid": ["interactive mutation flows"]
        }),
    }
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
            "description": "Current role-aware tool surface with short descriptions",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://stats/token-efficiency",
            "name": "Token Efficiency Stats",
            "description": "Session-level token, chain, and handle reuse metrics",
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
            "description": "Role profile intent, preferred tools, and anti-patterns",
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
            let surface = *state.surface();
            let tools = visible_tools(surface)
                .into_iter()
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description,
                        "tier": tool.annotations.as_ref().and_then(|ann| ann.tier.as_ref()).map(|tier| format!("{tier:?}")).unwrap_or_default()
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&json!({
                        "active_surface": surface.as_label(),
                        "tool_count": tools.len(),
                        "tools": tools
                    })).unwrap_or_default()
                }]
            })
        }
        "codelens://stats/token-efficiency" => {
            let session = state.metrics().session_snapshot();
            let handle_reads = session.analysis_summary_reads + session.analysis_section_reads;
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&json!({
                        "tools_list_tokens": session.tools_list_tokens,
                        "avg_tool_output_tokens": if session.total_calls > 0 {
                            session.total_tokens / session.total_calls as usize
                        } else { 0 },
                        "p95_tool_latency_ms": crate::telemetry::percentile_95(&session.latency_samples),
                        "retry_count": session.retry_count,
                        "handle_reuse_count": session.handle_reuse_count,
                        "repeated_low_level_chain_count": session.repeated_low_level_chain_count,
                        "composite_calls": session.composite_calls,
                        "low_level_calls": session.low_level_calls,
                        "stdio_session_count": session.stdio_session_count,
                        "http_session_count": session.http_session_count,
                        "analysis_jobs_enqueued": session.analysis_jobs_enqueued,
                        "analysis_jobs_started": session.analysis_jobs_started,
                        "analysis_jobs_completed": session.analysis_jobs_completed,
                        "analysis_jobs_failed": session.analysis_jobs_failed,
                        "analysis_jobs_cancelled": session.analysis_jobs_cancelled,
                        "analysis_queue_depth": session.analysis_queue_depth,
                        "analysis_queue_max_depth": session.analysis_queue_max_depth,
                        "active_analysis_workers": session.active_analysis_workers,
                        "peak_active_analysis_workers": session.peak_active_analysis_workers,
                        "analysis_worker_limit": session.analysis_worker_limit,
                        "analysis_transport_mode": session.analysis_transport_mode.clone(),
                        "handle_reuse_rate": if handle_reads > 0 {
                            session.handle_reuse_count as f64 / handle_reads as f64
                        } else { 0.0 }
                    })).unwrap_or_default()
                }]
            })
        }
        _ if uri.starts_with("codelens://profile/") && uri.ends_with("/guide") => {
            let profile_name = uri
                .trim_start_matches("codelens://profile/")
                .trim_end_matches("/guide");
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
                    json!({
                        "analysis_id": artifact.id,
                        "tool_name": artifact.tool_name,
                        "surface": artifact.surface,
                        "summary": artifact.summary,
                        "top_findings": artifact.top_findings,
                        "confidence": artifact.confidence,
                        "next_actions": artifact.next_actions,
                        "available_sections": artifact.available_sections,
                        "created_at_ms": artifact.created_at_ms,
                    })
                } else {
                    state
                        .get_analysis_section(analysis_id, section)
                        .unwrap_or_else(|_| json!({"error": format!("Unknown section `{section}`")}))
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
