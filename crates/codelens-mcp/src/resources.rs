//! MCP resource definitions and handlers.

use crate::AppState;
use crate::resource_context::{
    ResourceRequestContext, build_agent_activity_payload, build_http_session_payload,
    build_visible_tool_context, filter_default_listed_tools, filter_listed_tools,
};
use crate::session_metrics_payload::build_session_metrics_payload;
use crate::state::AnalysisArtifact;
use crate::surface_manifest::{HARNESS_HOST_COMPAT_RESOURCE_URI, HOST_ADAPTER_HOSTS};
use crate::tool_defs::{
    HostContext, SurfaceCompilerInput, TaskOverlay, ToolProfile, ToolSurface,
    preferred_tier_labels, tool_namespace, tool_preferred_executor_label, tool_tier_label,
    visible_tools,
};
use crate::tools::session::metrics_config::collect_runtime_health_snapshot;
use codelens_engine::{detect_frameworks, detect_workspace_packages};
use serde_json::{Value, json};
use std::collections::BTreeMap;

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

// === Merged from analysis_handles.rs ===

pub(crate) fn analysis_summary_resource(analysis_id: &str) -> Value {
    json!({
        "uri": format!("codelens://analysis/{analysis_id}/summary"),
    })
}

pub(crate) fn analysis_section_handles(analysis_id: &str, sections: &[String]) -> Value {
    json!(
        sections
            .iter()
            .map(|section| json!({
                "section": section,
                "uri": format!("codelens://analysis/{analysis_id}/{section}"),
            }))
            .collect::<Vec<_>>()
    )
}

// === Merged from resource_catalog.rs ===

pub(crate) fn static_resource_entries(project_name: &str) -> Vec<Value> {
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
            "uri": "codelens://surface/manifest",
            "name": "Surface Manifest",
            "description": "Canonical runtime and documentation surface manifest",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://surface/overlay",
            "name": "Surface Overlay Preview",
            "description": "Runtime preview of the (profile × host_context × task_overlay) compiled plan — query with ?host=<id>&task=<id>",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://backend/capabilities",
            "name": "Semantic Backend Capabilities",
            "description": "Passive capability map for the Rust engine, LSP bridge, and SCIP bridge backends — lists which capability each backend claims to fulfil",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://registry/projects",
            "name": "Project Registry",
            "description": "Active project plus registered secondary projects with memory availability, without requiring a tool call",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://registry/memory-scopes",
            "name": "Memory Scope Registry",
            "description": "Declared memory scopes (project + global) with current paths and mutation-wiring status",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://operator/dashboard",
            "name": "Operator Dashboard",
            "description": "Point-in-time operator snapshot — project + surface + index health + job queue + analysis summary + backends + memory scopes, aggregated from existing telemetry",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://harness/modes",
            "name": "Harness Modes",
            "description": "Canonical harness-mode topology and communication policy",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://harness/spec",
            "name": "Harness Spec",
            "description": "Portable harness contract with preflight, coordination, audit, and handoff templates",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://harness/host-adapters",
            "name": "Host Adapter Spec",
            "description": "Portable host-adaptation guidance for Claude Code, Codex, Cursor, Cline, Windsurf, and similar agent hosts",
            "mimeType": "application/json"
        }),
        json!({
            "uri": HARNESS_HOST_COMPAT_RESOURCE_URI,
            "name": "Resolved Harness Host",
            "description": "Compatibility summary for hosts that expect one resolved harness-host contract resource",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://design/agent-experience",
            "name": "Agent Experience Spec",
            "description": "Portable UX, user-flow, agent-flow, tool-flow, and harness-flow contract",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://schemas/handoff-artifact/v1",
            "name": "Handoff Artifact Schema v1",
            "description": "JSON schema for planner -> builder -> reviewer handoff artifacts",
            "mimeType": "application/schema+json"
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
        json!({
            "uri": "codelens://activity/current",
            "name": "Current Agent Activity",
            "description": "Active agent registrations, advisory claims, and recent per-session activity",
            "mimeType": "application/json"
        }),
    ];
    items.extend(HOST_ADAPTER_HOSTS.iter().map(|host| {
        json!({
            "uri": format!("codelens://host-adapters/{host}"),
            "name": format!("Host Adapter: {host}"),
            "description": "Concrete host-native routing and template bundle",
            "mimeType": "application/json"
        })
    }));
    items
}

pub(crate) fn visible_tool_summary(state: &AppState, uri: &str, params: Option<&Value>) -> Value {
    let request = ResourceRequestContext::from_request(uri, params);
    let surface = state.execution_surface(&request.session);
    let context = build_visible_tool_context(state, &request);
    let lean_contract = request.lean_tool_contract();
    let listed_tools = filter_default_listed_tools(
        filter_listed_tools(context.tools.clone(), None, false),
        &request,
        None,
        surface,
    );
    let mut namespace_counts = BTreeMap::new();
    let mut tier_counts = BTreeMap::new();
    let mut executor_counts = BTreeMap::new();
    for tool in &listed_tools {
        *namespace_counts
            .entry(tool_namespace(tool.name).to_owned())
            .or_insert(0usize) += 1;
        *tier_counts
            .entry(tool_tier_label(tool.name).to_owned())
            .or_insert(0usize) += 1;
        *executor_counts
            .entry(tool_preferred_executor_label(tool.name).to_owned())
            .or_insert(0usize) += 1;
    }
    let prioritized = listed_tools
        .iter()
        .take(8)
        .map(|tool| {
            json!({
                "name": tool.name,
                "namespace": tool_namespace(tool.name),
                "tier": tool_tier_label(tool.name),
                "preferred_executor": tool_preferred_executor_label(tool.name)
            })
        })
        .collect::<Vec<_>>();
    let mut payload = serde_json::Map::new();
    payload.insert(
        "client_profile".to_owned(),
        json!(context_request_client_profile(&request)),
    );
    payload.insert("active_surface".to_owned(), json!(surface.as_label()));
    payload.insert(
        "default_contract_mode".to_owned(),
        json!(request.tool_contract_mode()),
    );
    payload.insert("tool_count".to_owned(), json!(listed_tools.len()));
    payload.insert(
        "tool_count_total".to_owned(),
        json!(context.total_tool_count),
    );
    payload.insert(
        "preferred_namespaces".to_owned(),
        json!(context.preferred_namespaces),
    );
    payload.insert("preferred_tiers".to_owned(), json!(context.preferred_tiers));
    payload.insert(
        "loaded_namespaces".to_owned(),
        json!(context.loaded_namespaces),
    );
    payload.insert("loaded_tiers".to_owned(), json!(context.loaded_tiers));
    payload.insert(
        "effective_namespaces".to_owned(),
        json!(context.effective_namespaces),
    );
    payload.insert("effective_tiers".to_owned(), json!(context.effective_tiers));
    payload.insert(
        "deferred_loading_active".to_owned(),
        json!(context.deferred_loading_active),
    );
    payload.insert("preferred_executors".to_owned(), json!(executor_counts));
    payload.insert("recommended_tools".to_owned(), json!(prioritized));
    payload.insert(
        "note".to_owned(),
        json!("Read `codelens://tools/list/full` only when summary is insufficient."),
    );
    if !lean_contract {
        payload.insert("visible_namespaces".to_owned(), json!(namespace_counts));
        payload.insert("visible_tiers".to_owned(), json!(tier_counts));
        payload.insert("all_namespaces".to_owned(), json!(context.all_namespaces));
        payload.insert("all_tiers".to_owned(), json!(context.all_tiers));
        payload.insert(
            "full_tool_exposure".to_owned(),
            json!(context.full_tool_exposure),
        );
    }
    if let Some(namespace) = context.selected_namespace {
        payload.insert("selected_namespace".to_owned(), json!(namespace));
    }
    if let Some(tier) = context.selected_tier {
        payload.insert("selected_tier".to_owned(), json!(tier));
    }
    Value::Object(payload)
}

pub(crate) fn visible_tool_details(state: &AppState, uri: &str, params: Option<&Value>) -> Value {
    let request = ResourceRequestContext::from_request(uri, params);
    let surface = state.execution_surface(&request.session);
    let context = build_visible_tool_context(state, &request);
    let tools = filter_default_listed_tools(
        filter_listed_tools(context.tools, None, false),
        &request,
        None,
        surface,
    )
    .into_iter()
    .map(|tool| {
        json!({
            "name": tool.name,
            "namespace": tool_namespace(tool.name),
            "description": tool.description,
            "tier": tool_tier_label(tool.name),
            "preferred_executor": tool_preferred_executor_label(tool.name)
        })
    })
    .collect::<Vec<_>>();
    json!({
        "client_profile": context_request_client_profile(&request),
        "active_surface": surface.as_label(),
        "default_contract_mode": request.client_profile.default_tool_contract_mode(),
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

fn context_request_client_profile(
    request: &crate::resource_context::ResourceRequestContext,
) -> &'static str {
    request.client_profile.as_str()
}

// === Merged from resource_profiles.rs ===

pub(crate) const PROFILE_GUIDE_PROFILES: [ToolProfile; 7] = [
    ToolProfile::PlannerReadonly,
    ToolProfile::BuilderMinimal,
    ToolProfile::ReviewerGraph,
    ToolProfile::EvaluatorCompact,
    ToolProfile::RefactorFull,
    ToolProfile::CiAudit,
    ToolProfile::WorkflowFirst,
];

pub(crate) fn profile_guide(profile: ToolProfile) -> Value {
    match profile {
        ToolProfile::PlannerReadonly => json!({
            "profile": profile.as_str(),
            "intent": "Use bounded, read-only analysis to plan changes and rank context before implementation.",
            "preferred_tools": ["explore_codebase", "review_architecture", "review_changes", "plan_safe_refactor"],
            "preferred_namespaces": ["reports", "symbols", "graph", "filesystem", "session"],
            "avoid": ["rename_symbol", "replace_content", "raw graph expansion unless necessary"]
        }),
        ToolProfile::BuilderMinimal => json!({
            "profile": profile.as_str(),
            "intent": "Keep the visible surface small while implementing changes with only the essential symbol and edit tools.",
            "preferred_tools": ["explore_codebase", "trace_request_path", "plan_safe_refactor", "review_changes"],
            "preferred_namespaces": ["reports", "symbols", "filesystem", "session"],
            "avoid": ["dead-code audits", "full-graph exploration", "broad multi-project search"]
        }),
        ToolProfile::ReviewerGraph => json!({
            "profile": profile.as_str(),
            "intent": "Review risky changes with graph-aware, read-only evidence.",
            "preferred_tools": ["review_architecture", "review_changes", "cleanup_duplicate_logic", "diagnose_issues"],
            "preferred_namespaces": ["reports", "graph", "symbols", "session"],
            "avoid": ["mutation tools"]
        }),
        ToolProfile::RefactorFull => json!({
            "profile": profile.as_str(),
            "intent": "Run high-safety refactors only after a fresh preflight has narrowed the target surface and cleared blockers.",
            "preferred_tools": ["plan_safe_refactor", "review_changes", "trace_request_path", "review_architecture"],
            "preferred_namespaces": ["reports", "session"],
            "avoid": ["mutation before preflight", "broad edits without diagnostics or preview"]
        }),
        ToolProfile::CiAudit => json!({
            "profile": profile.as_str(),
            "intent": "Produce machine-friendly review output around diffs, impact, dead code, and structural risk.",
            "preferred_tools": ["review_changes", "cleanup_duplicate_logic", "review_architecture", "diagnose_issues"],
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
        ToolProfile::WorkflowFirst => json!({
            "profile": profile.as_str(),
            "description": "Problem-first workflow surface. Agents see 12 high-level workflow tools; low-level tools are deferred.",
            "surface_size": "workflow",
            "mutation": false,
            "preferred_tiers": preferred_tier_labels(ToolSurface::Profile(profile)),
        }),
    }
}

pub(crate) fn profile_guide_summary(profile: ToolProfile) -> Value {
    let guide = profile_guide(profile);
    json!({
        "profile": guide.get("profile").cloned().unwrap_or(json!(profile.as_str())),
        "intent": guide.get("intent").cloned().unwrap_or(json!("")),
        "preferred_tools": guide.get("preferred_tools").cloned().unwrap_or(json!([])),
        "preferred_namespaces": guide.get("preferred_namespaces").cloned().unwrap_or(json!([])),
        "preferred_tiers": preferred_tier_labels(ToolSurface::Profile(profile)),
    })
}

pub(crate) fn profile_resource_entries() -> Vec<Value> {
    PROFILE_GUIDE_PROFILES
        .iter()
        .flat_map(|profile| {
            [
                json!({
                    "uri": format!("codelens://profile/{}/guide", profile.as_str()),
                    "name": format!("Profile Guide: {}", profile.as_str()),
                    "description": "Compressed role profile guide",
                    "mimeType": "application/json"
                }),
                json!({
                    "uri": format!("codelens://profile/{}/guide/full", profile.as_str()),
                    "name": format!("Profile Guide (Full): {}", profile.as_str()),
                    "description": "Expanded role profile guide with anti-patterns",
                    "mimeType": "application/json"
                }),
            ]
        })
        .collect()
}

// === Merged from resource_analysis.rs ===

pub(crate) fn analysis_resource_entries(state: &AppState) -> Vec<Value> {
    let mut items = vec![
        json!({
            "uri": "codelens://analysis/recent",
            "name": "Recent Analyses",
            "description": "Recent stored analyses with summary resource handles",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://analysis/jobs",
            "name": "Analysis Jobs",
            "description": "Queued and completed analysis jobs with jump handles",
            "mimeType": "application/json"
        }),
    ];
    items.extend(
        state
            .list_analysis_summaries()
            .into_iter()
            .map(|artifact| {
                json!({
                    "uri": format!("codelens://analysis/{}/summary", artifact.id),
                    "name": format!("Analysis: {}", artifact.tool_name),
                    "description": format!("{} ({})", artifact.summary, artifact.surface),
                    "mimeType": "application/json"
                })
            })
            .collect::<Vec<_>>(),
    );
    items
}

pub(crate) fn recent_analysis_payload(state: &AppState) -> Value {
    let mut summaries = state.list_analysis_summaries();
    summaries.sort_by_key(|b| std::cmp::Reverse(b.created_at_ms));
    let mut tool_counts = BTreeMap::new();
    for summary in &summaries {
        *tool_counts
            .entry(summary.tool_name.clone())
            .or_insert(0usize) += 1;
    }
    let latest_created_at_ms = summaries
        .iter()
        .map(|summary| summary.created_at_ms)
        .max()
        .unwrap_or_default();
    let items = summaries
        .iter()
        .take(8)
        .map(|summary| {
            json!({
                "analysis_id": summary.id,
                "tool_name": summary.tool_name,
                "summary": summary.summary,
                "surface": summary.surface,
                "created_at_ms": summary.created_at_ms,
                "summary_resource": analysis_summary_resource(&summary.id),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "artifacts": items,
        "count": summaries.len(),
        "latest_created_at_ms": latest_created_at_ms,
        "tool_counts": tool_counts,
    })
}

pub(crate) fn recent_analysis_jobs_payload(state: &AppState) -> Value {
    let scope = state.current_project_scope();
    let mut jobs = state.list_analysis_jobs_for_scope(&scope, None);
    jobs.sort_by_key(|b| std::cmp::Reverse(b.updated_at_ms));
    let mut status_counts = BTreeMap::new();
    for job in &jobs {
        *status_counts
            .entry(job.status.as_str().to_owned())
            .or_insert(0usize) += 1;
    }
    let items = jobs
        .iter()
        .take(8)
        .map(|job| {
            let section_handles = job
                .analysis_id
                .as_deref()
                .map(|analysis_id| analysis_section_handles(analysis_id, &job.estimated_sections))
                .unwrap_or_else(|| json!([]));
            let summary_resource = job
                .analysis_id
                .as_deref()
                .map(analysis_summary_resource)
                .unwrap_or(Value::Null);
            json!({
                "job_id": job.id,
                "kind": job.kind,
                "status": job.status,
                "progress": job.progress,
                "current_step": job.current_step,
                "analysis_id": job.analysis_id,
                "estimated_sections": job.estimated_sections,
                "summary_resource": summary_resource,
                "section_handles": section_handles,
                "updated_at_ms": job.updated_at_ms,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "jobs": items,
        "count": jobs.len(),
        "active_count": jobs.iter().filter(|job| matches!(job.status, crate::runtime_types::JobLifecycle::Queued | crate::runtime_types::JobLifecycle::Running)).count(),
        "status_counts": status_counts,
    })
}

pub(crate) fn analysis_summary_payload(artifact: &AnalysisArtifact) -> Value {
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
    let summary_resource = analysis_summary_resource(&artifact.id);
    let section_handles = analysis_section_handles(&artifact.id, &artifact.available_sections);
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
        "summary_resource": summary_resource,
        "section_handles": section_handles,
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
        payload["evidence_handles"] = payload["section_handles"].clone();
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
        "explore_codebase"
            | "trace_request_path"
            | "review_architecture"
            | "plan_safe_refactor"
            | "audit_security_context"
            | "analyze_change_impact"
            | "cleanup_duplicate_logic"
            | "onboard_project"
            | "analyze_change_request"
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
