//! MCP resource definitions and handlers.

use crate::resource_analysis::{
    analysis_resource_entries, analysis_summary_payload, recent_analysis_jobs_payload,
    recent_analysis_payload,
};
use crate::resource_catalog::{
    static_resource_entries, visible_tool_details, visible_tool_summary,
};
use crate::resource_context::{
    build_agent_activity_payload, build_http_session_payload, ResourceRequestContext,
};
use crate::resource_profiles::{profile_guide, profile_guide_summary, profile_resource_entries};
use crate::session_metrics_payload::build_session_metrics_payload;
use crate::tool_defs::{
    visible_tools, HostContext, SurfaceCompilerInput, TaskOverlay, ToolProfile,
};
use crate::tools::session::metrics_config::collect_runtime_health_snapshot;
use crate::AppState;
use codelens_engine::{detect_frameworks, detect_workspace_packages};
use serde_json::{json, Value};

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
    let symbiote_aliases = symbiote_alias_entries(&items);
    items.extend(symbiote_aliases);
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

fn schema_resource(uri: &str, payload: Value) -> Value {
    json!({
        "contents": [{
            "uri": uri,
            "mimeType": "application/schema+json",
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

fn symbiote_alias_entries(entries: &[Value]) -> Vec<Value> {
    entries
        .iter()
        .filter_map(|entry| {
            let uri = entry.get("uri").and_then(|value| value.as_str())?;
            let rest = uri.strip_prefix("codelens://")?;
            let mut alias = entry.clone();
            let object = alias.as_object_mut()?;
            object.insert("uri".to_owned(), json!(format!("symbiote://{rest}")));

            let alias_name = object
                .get("name")
                .and_then(|value| value.as_str())
                .map(|name| format!("{name} (Symbiote Alias)"))
                .unwrap_or_else(|| format!("symbiote://{rest}"));
            object.insert("name".to_owned(), json!(alias_name));

            let alias_description = object
                .get("description")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(|description| format!("{description} [Symbiote URI alias for `{uri}`]"))
                .unwrap_or_else(|| format!("Symbiote URI alias for `{uri}`"));
            object.insert("description".to_owned(), json!(alias_description));
            Some(alias)
        })
        .collect()
}

/// ADR-0007 Phase 2: accept `symbiote://<rest>` as an alias of
/// `codelens://<rest>`. Dispatch logic below remains pinned to the
/// canonical `codelens://` form; this normalizer is the single rewrite
/// site so we don't have to dual-match every arm.
fn normalize_resource_uri(uri: &str) -> std::borrow::Cow<'_, str> {
    if let Some(rest) = uri.strip_prefix("symbiote://") {
        std::borrow::Cow::Owned(format!("codelens://{}", rest))
    } else {
        std::borrow::Cow::Borrowed(uri)
    }
}

pub(crate) fn read_resource(state: &AppState, uri: &str, params: Option<&Value>) -> Value {
    let normalized = normalize_resource_uri(uri);
    let uri = normalized.as_ref();
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
                        "Use workflow-first entrypoints such as explore_codebase, review_architecture, and review_changes before low-level expansion.",
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
        "codelens://surface/manifest" => json_resource(
            uri,
            crate::surface_manifest::build_surface_manifest_for_state(state),
        ),
        "codelens://registry/projects" => {
            let entries = crate::registry::enumerate_projects(state);
            json_resource(
                uri,
                json!({
                    "projects": entries,
                    "count_active": 1,
                    "count_secondary": state.list_secondary_projects().len(),
                }),
            )
        }
        "codelens://registry/memory-scopes" => {
            let scopes = crate::registry::enumerate_memory_scopes(state);
            json_resource(
                uri,
                json!({
                    "scopes": scopes,
                    "note": "Passive scaffold (P3). Mutation tools (write_memory / read_memory) currently operate on project scope only. Global scope path is declared so the contract is visible before wiring the active half of P3.",
                }),
            )
        }
        "codelens://backend/capabilities" => {
            let reports = crate::backend::enumerate_backends(state);
            let coverage = crate::backend::capability_coverage();
            let coverage_payload = coverage
                .into_iter()
                .map(|(cap, fulfillers)| {
                    json!({
                        "capability": cap.as_str(),
                        "fulfilled_by": fulfillers,
                    })
                })
                .collect::<Vec<_>>();
            json_resource(
                uri,
                json!({
                    "backends": reports,
                    "capability_coverage": coverage_payload,
                    "note": "Passive scaffold (P2). Dispatch does not yet route through the SemanticBackend trait; this resource reports declared capability rather than actual backend selection per call.",
                }),
            )
        }
        "codelens://surface/overlay" => {
            let surface = state.execution_surface(&request.session);
            let requested_host = params
                .and_then(|value| value.get("host"))
                .and_then(|value| value.as_str());
            let requested_task = params
                .and_then(|value| value.get("task"))
                .and_then(|value| value.as_str());
            let mut input = SurfaceCompilerInput::new(surface);
            if let Some(host) = requested_host.and_then(HostContext::from_str) {
                input = input.with_host(host);
            }
            if let Some(task) = requested_task.and_then(TaskOverlay::from_str) {
                input = input.with_task(task);
            }
            let plan = input.compile();
            let unknown_host = requested_host
                .filter(|name| HostContext::from_str(name).is_none())
                .map(str::to_owned);
            let unknown_task = requested_task
                .filter(|name| TaskOverlay::from_str(name).is_none())
                .map(str::to_owned);
            json_resource(
                uri,
                json!({
                    "surface": surface.as_label(),
                    "host_context": plan.host_context.map(|value| value.as_str()),
                    "task_overlay": plan.task_overlay.map(|value| value.as_str()),
                    "applied": plan.applied(),
                    "preferred_executor_bias": plan.preferred_executor_bias,
                    "preferred_entrypoints": plan.preferred_entrypoints,
                    "emphasized_tools": plan.emphasized_tools,
                    "avoid_tools": plan.avoid_tools,
                    "routing_notes": plan.routing_notes,
                    "requested_host": requested_host,
                    "requested_task": requested_task,
                    "unknown_host": unknown_host,
                    "unknown_task": unknown_task,
                }),
            )
        }
        "codelens://harness/modes" => json_resource(
            uri,
            crate::surface_manifest::build_surface_manifest_for_state(state)["harness_modes"]
                .clone(),
        ),
        "codelens://harness/spec" => json_resource(
            uri,
            crate::surface_manifest::build_surface_manifest_for_state(state)["harness_spec"]
                .clone(),
        ),
        "codelens://harness/host-adapters" => json_resource(
            uri,
            crate::surface_manifest::build_surface_manifest_for_state(state)["host_adapters"]
                .clone(),
        ),
        "codelens://harness/host" => {
            let requested_host = params
                .and_then(|value| value.get("host"))
                .and_then(|value| value.as_str())
                .unwrap_or("claude-code");
            let selection_source = if params
                .and_then(|value| value.get("host"))
                .and_then(|value| value.as_str())
                .is_some()
            {
                "request_param"
            } else {
                "default"
            };
            let body = crate::surface_manifest::harness_host_compat_bundle(
                requested_host,
                selection_source,
            )
            .unwrap_or_else(|| {
                json!({
                    "error": format!("Unknown host `{requested_host}`"),
                    "requested_host": requested_host,
                    "selection_source": selection_source
                })
            });
            json_resource(uri, body)
        }
        "codelens://design/agent-experience" => json_resource(
            uri,
            crate::surface_manifest::build_surface_manifest_for_state(state)["agent_experience"]
                .clone(),
        ),
        _ if uri.starts_with("codelens://host-adapters/") => {
            let host = uri.trim_start_matches("codelens://host-adapters/");
            let body = crate::surface_manifest::host_adapter_bundle(host)
                .unwrap_or_else(|| json!({"error": format!("Unknown host adapter `{host}`")}));
            json_resource(uri, body)
        }
        "codelens://schemas/handoff-artifact/v1" => {
            schema_resource(uri, crate::surface_manifest::handoff_artifact_schema_json())
        }
        "codelens://stats/token-efficiency" => {
            let metrics_payload = build_session_metrics_payload(
                state,
                if request.session.is_local() {
                    None
                } else {
                    Some(request.session.session_id.as_str())
                },
                request.session.project_path.as_deref(),
            );
            let mut stats = metrics_payload.session;
            stats.insert("derived_kpis".to_owned(), metrics_payload.derived_kpis);
            json_resource(uri, Value::Object(stats))
        }
        "codelens://session/http" => {
            json_resource(uri, build_http_session_payload(state, &request))
        }
        "codelens://activity/current" => {
            json_resource(uri, build_agent_activity_payload(state, &request))
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
                    state.metrics().record_analysis_read_for_session(
                        false,
                        if request.session.is_local() {
                            None
                        } else {
                            Some(request.session.session_id.as_str())
                        },
                    );
                    analysis_summary_payload(&artifact)
                } else {
                    state.metrics().record_analysis_read_for_session(
                        true,
                        if request.session.is_local() {
                            None
                        } else {
                            Some(request.session.session_id.as_str())
                        },
                    );
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
