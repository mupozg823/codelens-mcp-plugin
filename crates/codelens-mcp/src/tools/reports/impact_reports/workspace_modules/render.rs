use serde_json::{Value, json};
use std::collections::BTreeMap;

use super::{WORKSPACE_MODULE_GRANULARITY, WorkspaceModuleGraph};
use crate::tools::reports::impact_reports::mermaid_escape_label;

impl WorkspaceModuleGraph {
    pub(super) fn render_mermaid(&self, max_nodes: usize) -> String {
        let mut out = String::from("flowchart LR\n");
        out.push_str("    classDef workspace fill:#e8f4ff,stroke:#2c5f8a,stroke-width:2px\n");
        out.push_str("    classDef module fill:#f6f6f6,stroke:#777\n");
        out.push_str("    classDef note fill:#ffffcc,stroke:#999,stroke-dasharray:4\n");

        let mut node_ids = BTreeMap::new();
        for (index, module) in self.modules.iter().take(max_nodes).enumerate() {
            let node_id = format!("m{index}");
            let class_name = if module.workspace_member {
                "workspace"
            } else {
                "module"
            };
            out.push_str(&format!(
                "    {node_id}[\"{}\"]:::{class_name}\n",
                mermaid_escape_label(&module.path)
            ));
            node_ids.insert(module.path.as_str(), node_id);
        }

        for edge in &self.edges {
            let Some(source_id) = node_ids.get(edge.source.as_str()) else {
                continue;
            };
            let Some(target_id) = node_ids.get(edge.target.as_str()) else {
                continue;
            };
            out.push_str(&format!(
                "    {source_id} -->|\"{} files\"| {target_id}\n",
                edge.file_pair_count
            ));
        }

        if self.edges.is_empty() {
            out.push_str("    no_edges[\"no cross-module edges in scoped impact\"]:::note\n");
        }
        if self.modules.len() > max_nodes {
            let extra = self.modules.len() - max_nodes;
            out.push_str(&format!(
                "    modules_more[\"... +{extra} more modules\"]:::note\n"
            ));
        }

        out
    }

    pub(super) fn as_json(&self) -> Value {
        json!({
            "granularity": WORKSPACE_MODULE_GRANULARITY,
            "workspace_member_count": self.workspace_member_count,
            "nodes": self.modules.iter().map(|module| {
                json!({
                    "path": module.path,
                    "workspace_member": module.workspace_member,
                    "file_count": module.file_count,
                })
            }).collect::<Vec<_>>(),
            "edges": self.edges.iter().map(|edge| {
                json!({
                    "source": edge.source,
                    "target": edge.target,
                    "file_pair_count": edge.file_pair_count,
                })
            }).collect::<Vec<_>>(),
        })
    }
}
