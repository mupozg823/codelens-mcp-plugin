//! MCP resource definitions and handlers.

mod analysis;
mod analysis_handles;
mod catalog;
mod format;
mod profiles;
mod tool_listing;
mod uri_aliases;

use crate::AppState;
use crate::resource_context::{
    ResourceRequestContext, build_agent_activity_payload, build_http_session_payload,
};
use crate::session_metrics_payload::build_session_metrics_payload;
use crate::tool_defs::{
    HostContext, SurfaceCompilerInput, TaskOverlay, ToolProfile, visible_tools,
};
use crate::tools::session::metrics_config::collect_runtime_health_snapshot;
use codelens_engine::{detect_frameworks, detect_workspace_packages};
use serde_json::{Value, json};

use analysis::{
    analysis_resource_entries, analysis_summary_payload, recent_analysis_jobs_payload,
    recent_analysis_payload,
};
pub(crate) use analysis_handles::{analysis_section_handles, analysis_summary_resource};
use catalog::static_resource_entries;
use format::{json_resource, schema_resource, text_resource};
use profiles::{profile_guide, profile_guide_summary, profile_resource_entries};
use tool_listing::{visible_tool_details, visible_tool_summary};
use uri_aliases::{normalize_resource_uri, symbiote_alias_entries};

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
        "codelens://operator/dashboard" => {
            let dashboard = crate::operator::build_operator_dashboard(state);
            json_resource(uri, serde_json::to_value(&dashboard).unwrap_or(Value::Null))
        }
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
                    "note": "write_memory / delete_memory / read_memory / list_memories accept a `scope` parameter and resolve to either tier (write/delete default `project`, read defaults `auto`, list defaults `project`; read also accepts `auto`, list also accepts `both`). rename_memory / archive_memory / restore_memory / list_archived are project-scoped only. The global tier resolves to $HOME/.codelens/memories/.",
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
                    "note": "Passive scaffold (P2). Backend reports separate declared capability from active runtime availability. Retrieval and semantic_edit_backend are intentionally separate; dispatch does not yet route through the SemanticBackend trait.",
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
            let body = crate::surface_manifest::harness_host_compat_bundle_for_project(
                requested_host,
                selection_source,
                Some(state.project().as_path()),
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
        "codelens://host-instructions/audit" => json_resource(
            uri,
            crate::instruction_audit::instruction_manifest_audit(state.project().as_path()),
        ),
        "codelens://benchmarks/host-plugin-stack" => json_resource(
            uri,
            crate::instruction_audit::host_plugin_stack_benchmark(state.project().as_path()),
        ),
        "codelens://design/agent-experience" => json_resource(
            uri,
            crate::surface_manifest::build_surface_manifest_for_state(state)["agent_experience"]
                .clone(),
        ),
        _ if uri.starts_with("codelens://host-adapters/") => {
            let host = uri.trim_start_matches("codelens://host-adapters/");
            let body = crate::surface_manifest::host_adapter_bundle_for_project(
                host,
                Some(state.project().as_path()),
            )
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
            stats.insert("token_bill".to_owned(), metrics_payload.token_bill);
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
