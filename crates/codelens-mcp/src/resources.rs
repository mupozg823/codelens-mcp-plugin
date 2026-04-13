//! MCP resource definitions and handlers.

use crate::AppState;
use crate::resource_analysis::{
    analysis_resource_entries, analysis_summary_payload, recent_analysis_jobs_payload,
    recent_analysis_payload,
};
use crate::resource_catalog::{
    static_resource_entries, visible_tool_details, visible_tool_summary,
};
use crate::resource_context::{ResourceRequestContext, build_http_session_payload};
use crate::resource_profiles::{profile_guide, profile_guide_summary, profile_resource_entries};
use crate::session_metrics_payload::build_session_metrics_payload;
use crate::tool_defs::{ToolProfile, visible_tools};
use crate::tools::session::metrics_config::collect_runtime_health_snapshot;
use codelens_engine::{detect_frameworks, detect_workspace_packages};
use serde_json::{Value, json};

pub(crate) fn resources(state: &AppState) -> Vec<Value> {
    let project_name = state
        .project()
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut items = static_resource_entries(&project_name);
    items.extend(profile_resource_entries());
    items.extend(analysis_resource_entries(state));
    items
}

fn json_resource(uri: &str, payload: Value) -> Value {
    json!({
        "contents": [{
            "uri": uri,
            "mimeType": "application/json",
            "text": serde_json::to_string_pretty(&payload).unwrap_or_default()
        }]
    })
}

fn text_resource(uri: &str, text: String) -> Value {
    json!({
        "contents": [{
            "uri": uri,
            "mimeType": "text/plain",
            "text": text
        }]
    })
}

pub(crate) fn read_resource(state: &AppState, uri: &str, params: Option<&Value>) -> Value {
    let request = ResourceRequestContext::from_request(uri, params);
    let _session_project_guard = state
        .ensure_session_project(&request.session)
        .ok()
        .flatten();
    match uri {
        "codelens://project/overview" => {
            let surface = state.execution_surface(&request.session);
            let visible = visible_tools(surface);
            let runtime_health = collect_runtime_health_snapshot(state, surface);
            json_resource(
                uri,
                json!({
                    "project_root": state.project().as_path().to_string_lossy(),
                    "active_surface": surface.as_label(),
                    "daemon_mode": state.daemon_mode().as_str(),
                    "visible_tool_count": visible.len(),
                    "symbol_index": runtime_health.index_stats,
                    "health_summary": runtime_health.health_summary,
                    "memories_dir": state.memories_dir().to_string_lossy(),
                }),
            )
        }
        "codelens://project/architecture" => {
            let stats = state.symbol_index().stats().ok();
            let frameworks = detect_frameworks(state.project().as_path());
            let workspace_packages = detect_workspace_packages(state.project().as_path());
            let surface = state.execution_surface(&request.session);
            json_resource(
                uri,
                json!({
                    "active_surface": surface.as_label(),
                    "daemon_mode": state.daemon_mode().as_str(),
                    "frameworks": frameworks,
                    "workspace_packages": workspace_packages,
                    "indexed_files": stats.as_ref().map(|s| s.indexed_files).unwrap_or(0),
                    "stale_files": stats.as_ref().map(|s| s.stale_files).unwrap_or(0),
                    "notes": [
                        "Use workflow-first entrypoints such as explore_codebase, review_architecture, and analyze_change_impact before low-level expansion.",
                        "Prefer HTTP + role profiles for multi-agent harnesses."
                    ]
                }),
            )
        }
        "codelens://tools/list" => {
            if request.deferred_loading_requested
                && (request.requested_namespace.is_some() || request.requested_tier.is_some())
            {
                state.metrics().record_deferred_namespace_expansion();
            }
            json_resource(uri, visible_tool_summary(state, uri, params))
        }
        "codelens://tools/list/full" => {
            json_resource(uri, visible_tool_details(state, uri, params))
        }
        "codelens://stats/token-efficiency" => {
            let metrics_payload = build_session_metrics_payload(state);
            let mut stats = metrics_payload.session;
            stats.insert("derived_kpis".to_owned(), metrics_payload.derived_kpis);
            json_resource(uri, Value::Object(stats))
        }
        "codelens://session/http" => {
            json_resource(uri, build_http_session_payload(state, &request))
        }
        "codelens://analysis/recent" => json_resource(uri, recent_analysis_payload(state)),
        "codelens://analysis/jobs" => json_resource(uri, recent_analysis_jobs_payload(state)),
        _ if uri.starts_with("codelens://profile/") && uri.ends_with("/guide") => {
            let profile_name = uri
                .trim_start_matches("codelens://profile/")
                .trim_end_matches("/guide");
            let profile = ToolProfile::from_str(profile_name);
            let body = profile
                .map(profile_guide_summary)
                .unwrap_or_else(|| json!({"error": format!("Unknown profile `{profile_name}`")}));
            json_resource(uri, body)
        }
        _ if uri.starts_with("codelens://profile/") && uri.ends_with("/guide/full") => {
            let profile_name = uri
                .trim_start_matches("codelens://profile/")
                .trim_end_matches("/guide/full");
            let profile = ToolProfile::from_str(profile_name);
            let body = profile
                .map(profile_guide)
                .unwrap_or_else(|| json!({"error": format!("Unknown profile `{profile_name}`")}));
            json_resource(uri, body)
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
                json_resource(uri, content)
            } else {
                json_resource(
                    uri,
                    json!({"error": format!("Unknown analysis `{analysis_id}`")}),
                )
            }
        }
        _ => text_resource(uri, format!("Unknown resource: {uri}")),
    }
}
