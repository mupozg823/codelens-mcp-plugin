//! MCP resource definitions and handlers.

mod analysis;
mod analysis_handles;
mod analysis_reader;
mod catalog;
mod format;
mod profiles;
mod project_resources;
mod session_resources;
mod surface_resources;
mod tool_listing;
mod uri_aliases;

use crate::AppState;
use crate::resource_context::ResourceRequestContext;
use serde_json::Value;

use analysis::analysis_resource_entries;
pub(crate) use analysis_handles::{analysis_section_handles, analysis_summary_resource};
use catalog::static_resource_entries;
use format::{json_resource, text_resource};
use profiles::profile_resource_entries;
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
            project_resources::project_overview_resource(state, uri, &request)
        }
        "codelens://project/architecture" => {
            project_resources::project_architecture_resource(state, uri, &request)
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
        "codelens://surface/manifest" => surface_resources::surface_manifest_resource(state, uri),
        "codelens://operator/dashboard" => {
            project_resources::operator_dashboard_resource(state, uri)
        }
        "codelens://registry/projects" => project_resources::registry_projects_resource(state, uri),
        "codelens://registry/memory-scopes" => {
            project_resources::registry_memory_scopes_resource(state, uri)
        }
        "codelens://backend/capabilities" => {
            project_resources::backend_capabilities_resource(state, uri)
        }
        "codelens://surface/overlay" => {
            surface_resources::surface_overlay_resource(state, uri, params, &request)
        }
        "codelens://harness/modes" => surface_resources::harness_modes_resource(state, uri),
        "codelens://harness/spec" => surface_resources::harness_spec_resource(state, uri),
        "codelens://harness/host-adapters" => {
            surface_resources::harness_host_adapters_resource(state, uri)
        }
        "codelens://harness/host" => surface_resources::harness_host_resource(state, uri, params),
        "codelens://host-instructions/audit" => {
            surface_resources::host_instructions_audit_resource(state, uri)
        }
        "codelens://benchmarks/host-plugin-stack" => {
            surface_resources::host_plugin_stack_benchmark_resource(state, uri)
        }
        "codelens://design/agent-experience" => {
            surface_resources::agent_experience_resource(state, uri)
        }
        crate::skill_catalog::CODEX_SKILL_CATALOG_RESOURCE_URI => {
            surface_resources::codex_skill_catalog_resource(uri)
        }
        _ if uri.starts_with("codelens://host-adapters/") => {
            surface_resources::host_adapter_bundle_resource(state, uri)
        }
        "codelens://schemas/handoff-artifact/v1" => surface_resources::handoff_schema_resource(uri),
        "codelens://stats/token-efficiency" => {
            session_resources::token_efficiency_resource(state, uri, &request)
        }
        "codelens://session/http" => session_resources::http_session_resource(state, uri, &request),
        "codelens://activity/current" => {
            session_resources::agent_activity_resource(state, uri, &request)
        }
        "codelens://analysis/recent" => analysis_reader::recent_analysis_resource(state, uri),
        "codelens://analysis/jobs" => analysis_reader::analysis_jobs_resource(state, uri),
        _ if uri.starts_with("codelens://profile/") && uri.ends_with("/guide") => {
            surface_resources::profile_guide_summary_resource(uri)
        }
        _ if uri.starts_with("codelens://profile/") && uri.ends_with("/guide/full") => {
            surface_resources::profile_guide_full_resource(uri)
        }
        _ if uri.starts_with("codelens://analysis/") => {
            analysis_reader::analysis_artifact_resource(state, uri, &request)
        }
        _ => text_resource(uri, format!("Unknown resource: {uri}")),
    }
}
