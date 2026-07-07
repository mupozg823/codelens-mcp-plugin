mod build;
mod render;
#[cfg(test)]
mod tests;

use codelens_engine::ProjectRoot;
use serde_json::{Value, json};

pub(super) const WORKSPACE_MODULE_GRANULARITY: &str = "workspace_modules";

pub(crate) struct WorkspaceModuleGraphReport {
    pub(crate) mermaid: String,
    pub(crate) module_graph: Value,
    pub(crate) stats: Value,
    pub(crate) top_finding: String,
}

#[derive(Clone)]
pub(super) struct WorkspaceModuleNode {
    pub(super) path: String,
    pub(super) workspace_member: bool,
    pub(super) file_count: usize,
}

pub(super) struct WorkspaceModuleEdge {
    pub(super) source: String,
    pub(super) target: String,
    pub(super) file_pair_count: usize,
}

pub(super) struct WorkspaceModuleGraph {
    workspace_member_count: usize,
    modules: Vec<WorkspaceModuleNode>,
    edges: Vec<WorkspaceModuleEdge>,
}

pub(crate) fn build_workspace_module_graph_report(
    project: &ProjectRoot,
    requested_path: &str,
    impact: &Value,
    max_nodes: usize,
) -> Option<WorkspaceModuleGraphReport> {
    if impact.get("scope_kind").and_then(Value::as_str) != Some("directory") {
        return None;
    }
    if project.resolve(requested_path).ok()? != project.as_path() {
        return None;
    }

    let workspace_members = build::read_workspace_members(project.as_path())?;
    let graph = WorkspaceModuleGraph::from_impact(workspace_members, impact);
    Some(graph.into_report(requested_path, impact, max_nodes))
}

impl WorkspaceModuleGraph {
    fn new(
        workspace_member_count: usize,
        modules: Vec<WorkspaceModuleNode>,
        edges: Vec<WorkspaceModuleEdge>,
    ) -> Self {
        Self {
            workspace_member_count,
            modules,
            edges,
        }
    }

    fn into_report(
        self,
        requested_path: &str,
        impact: &Value,
        max_nodes: usize,
    ) -> WorkspaceModuleGraphReport {
        let mermaid = self.render_mermaid(max_nodes);
        let module_graph = self.as_json();
        let stats = json!({
            "target": requested_path,
            "scope_kind": impact.get("scope_kind").and_then(Value::as_str).unwrap_or("directory"),
            "granularity": WORKSPACE_MODULE_GRANULARITY,
            "workspace_member_count": self.workspace_member_count,
            "module_count": self.modules.len(),
            "module_edge_count": self.edges.len(),
            "in_scope_file_count": impact.get("in_scope_file_count").cloned().unwrap_or(Value::Null),
            "in_scope_file_limit_hit": impact.get("in_scope_file_limit_hit").cloned().unwrap_or(Value::Null),
            "upstream_total": impact.get("direct_importers").and_then(Value::as_array).map_or(0, Vec::len),
            "downstream_total": impact.get("blast_radius").and_then(Value::as_array).map_or(0, Vec::len),
            "max_nodes_rendered": max_nodes,
        });
        let top_finding = format!(
            "{} workspace members, {} module nodes, {} cross-module edges",
            self.workspace_member_count,
            self.modules.len(),
            self.edges.len()
        );

        WorkspaceModuleGraphReport {
            mermaid,
            module_graph,
            stats,
            top_finding,
        }
    }
}
