use crate::AppState;
use crate::resource_context::ResourceRequestContext;
use crate::tool_defs::visible_tools;
use crate::tools::session::metrics_config::collect_runtime_health_snapshot;
use codelens_engine::{detect_frameworks, detect_workspace_packages};
use serde_json::{Value, json};

use super::format::json_resource;

pub(super) fn project_overview_resource(
    state: &AppState,
    uri: &str,
    request: &ResourceRequestContext,
) -> Value {
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

pub(super) fn project_architecture_resource(
    state: &AppState,
    uri: &str,
    request: &ResourceRequestContext,
) -> Value {
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

pub(super) fn operator_dashboard_resource(state: &AppState, uri: &str) -> Value {
    let dashboard = crate::operator::build_operator_dashboard(state);
    json_resource(uri, serde_json::to_value(&dashboard).unwrap_or(Value::Null))
}

pub(super) fn registry_projects_resource(state: &AppState, uri: &str) -> Value {
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

pub(super) fn registry_memory_scopes_resource(state: &AppState, uri: &str) -> Value {
    let scopes = crate::registry::enumerate_memory_scopes(state);
    json_resource(
        uri,
        json!({
            "scopes": scopes,
            "note": "write_memory / delete_memory / read_memory / list_memories accept a `scope` parameter and resolve to either tier (write/delete default `project`, read defaults `auto`, list defaults `project`; read also accepts `auto`, list also accepts `both`). rename_memory / archive_memory / restore_memory / list_archived are project-scoped only. The global tier resolves to $HOME/.codelens/memories/.",
        }),
    )
}

pub(super) fn backend_capabilities_resource(state: &AppState, uri: &str) -> Value {
    let reports = crate::backend::enumerate_backends(state);
    let coverage_payload = crate::backend::capability_coverage()
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
