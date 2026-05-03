use crate::AppState;
use crate::tool_runtime::{ToolResult, required_string};
use crate::tools::report_contract::make_handle_response;
use crate::tools::report_utils::stable_cache_key;
use serde_json::{Value, json};
use std::collections::BTreeMap;

use super::{
    ScopeBoundarySummary, collect_scope_boundary_summary, file_name, impact_entry_file,
    mermaid_escape_label, parent_dir,
};

/// Render a Mermaid `flowchart LR` diagram summarising direct importers
/// (upstream) and blast-radius dependencies (downstream) of a target file.
///
/// Nodes are clustered into `subgraph` blocks by parent directory.  Each node
/// label shows only the filename; the full path lives in the subgraph title.
/// `classDef` blocks style target / upstream / downstream nodes distinctly.
/// When either side exceeds `max_nodes`, a `...+N more` note node is appended.
/// Downstream edges carry a label when the entry contains `symbols_affected`
/// or `depth`.
pub(crate) fn render_module_mermaid(
    target: &str,
    importers: &[Value],
    downstream: &[Value],
    max_nodes: usize,
) -> String {
    let mut out = String::from("flowchart LR\n");

    // ── classDef styling ─────────────────────────────────────────────────────
    out.push_str("    classDef target fill:#f9f,stroke:#333,stroke-width:2px\n");
    out.push_str("    classDef upstream fill:#bbf,stroke:#333\n");
    out.push_str("    classDef downstream fill:#fbb,stroke:#333\n");
    out.push_str("    classDef note fill:#ffffcc,stroke:#999,stroke-dasharray:4\n");

    // ── target node ──────────────────────────────────────────────────────────
    out.push_str(&format!(
        "    target0[\"{}\"]:::target\n",
        mermaid_escape_label(file_name(target))
    ));

    // ── helper: collect (node_id, file_path) pairs for a side ────────────────
    let capped_importers: Vec<(String, &str)> = importers
        .iter()
        .take(max_nodes)
        .enumerate()
        .map(|(i, e)| {
            (
                format!("up{i}"),
                impact_entry_file(e).unwrap_or("<unknown>"),
            )
        })
        .collect();

    let capped_downstream: Vec<(String, &Value)> = downstream
        .iter()
        .take(max_nodes)
        .enumerate()
        .map(|(i, e)| (format!("down{i}"), e))
        .collect();

    // ── upstream subgraphs ───────────────────────────────────────────────────
    // Group by parent dir, preserving insertion order via BTreeMap for stable output.
    let mut up_by_dir: std::collections::BTreeMap<&str, Vec<(&str, &str)>> =
        std::collections::BTreeMap::new();
    for (node_id, file) in &capped_importers {
        up_by_dir
            .entry(parent_dir(file))
            .or_default()
            .push((node_id, file));
    }

    for (dir, nodes) in &up_by_dir {
        out.push_str(&format!("    subgraph {}\n", mermaid_escape_label(dir)));
        for (node_id, file) in nodes {
            out.push_str(&format!(
                "        {}[\"{}\"]:::upstream\n",
                node_id,
                mermaid_escape_label(file_name(file))
            ));
        }
        out.push_str("    end\n");
    }

    // ── downstream subgraphs ─────────────────────────────────────────────────
    let mut down_by_dir: std::collections::BTreeMap<&str, Vec<(&str, &Value)>> =
        std::collections::BTreeMap::new();
    for (node_id, entry) in &capped_downstream {
        let file = impact_entry_file(entry).unwrap_or("<unknown>");
        down_by_dir
            .entry(parent_dir(file))
            .or_default()
            .push((node_id, entry));
    }

    for (dir, nodes) in &down_by_dir {
        out.push_str(&format!("    subgraph {}\n", mermaid_escape_label(dir)));
        for (node_id, entry) in nodes {
            let file = impact_entry_file(entry).unwrap_or("<unknown>");
            out.push_str(&format!(
                "        {}[\"{}\"]:::downstream\n",
                node_id,
                mermaid_escape_label(file_name(file))
            ));
        }
        out.push_str("    end\n");
    }

    // ── upstream edges ───────────────────────────────────────────────────────
    for (node_id, _file) in &capped_importers {
        out.push_str(&format!("    {node_id} --> target0\n"));
    }

    // ── truncation note for upstream ─────────────────────────────────────────
    if importers.len() > max_nodes {
        let extra = importers.len() - max_nodes;
        out.push_str(&format!("    up_more[\"... +{extra} more\"]:::note\n"));
        out.push_str("    up_more --> target0\n");
    }

    // ── downstream edges (with optional labels) ───────────────────────────────
    for (node_id, entry) in &capped_downstream {
        let label = entry
            .get("symbols_affected")
            .and_then(Value::as_u64)
            .map(|n| format!("{n} symbols"))
            .or_else(|| {
                entry
                    .get("depth")
                    .and_then(Value::as_u64)
                    .map(|d| format!("depth {d}"))
            });

        if let Some(lbl) = label {
            out.push_str(&format!(
                "    target0 -->|\"{}\"|{node_id}\n",
                mermaid_escape_label(&lbl)
            ));
        } else {
            out.push_str(&format!("    target0 --> {node_id}\n"));
        }
    }

    // ── truncation note for downstream ───────────────────────────────────────
    if downstream.len() > max_nodes {
        let extra = downstream.len() - max_nodes;
        out.push_str(&format!("    down_more[\"... +{extra} more\"]:::note\n"));
        out.push_str("    target0 --> down_more\n");
    }

    out
}

#[allow(deprecated)]
pub fn mermaid_module_graph(state: &AppState, arguments: &Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    let max_nodes = arguments
        .get("max_nodes")
        .and_then(Value::as_u64)
        .unwrap_or(10) as usize;
    if let Some(scope) = collect_scope_boundary_summary(state, path)? {
        return mermaid_scope_graph(state, arguments, path, scope, max_nodes);
    }

    let impact = crate::tools::graph::get_impact_analysis(
        state,
        &json!({"file_path": path, "max_depth": 2}),
    )
    .map(|out| out.0)
    .unwrap_or_else(|_| json!({"blast_radius": [], "direct_importers": []}));

    let importers: Vec<Value> = impact
        .get("direct_importers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let downstream: Vec<Value> = impact
        .get("blast_radius")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mermaid = render_module_mermaid(path, &importers, &downstream, max_nodes);
    let importer_count = importers.len();
    let downstream_count = downstream.len();

    let top_findings = vec![format!(
        "{} upstream, {} downstream (rendered up to {} per side)",
        importer_count, downstream_count, max_nodes
    )];

    let mut sections = BTreeMap::new();
    sections.insert(
        "diagram".to_owned(),
        json!({
            "format": "mermaid",
            "syntax": "flowchart",
            "content": mermaid,
            "hint": "Embed the `content` field in a fenced ```mermaid block to render in GitHub / GitLab / VS Code Markdown.",
        }),
    );
    sections.insert(
        "stats".to_owned(),
        json!({
            "target": path,
            "upstream_total": importer_count,
            "downstream_total": downstream_count,
            "max_nodes_rendered": max_nodes,
        }),
    );
    sections.insert("raw_impact".to_owned(), impact);

    make_handle_response(
        state,
        "mermaid_module_graph",
        stable_cache_key("mermaid_module_graph", arguments, &["path", "max_nodes"]),
        format!("Mermaid flowchart of module dependency boundaries for `{path}`."),
        top_findings,
        0.90,
        vec![
            "Embed the diagram in a PR body to visualise module risk".to_owned(),
            "Call module_boundary_report for structural coupling + cycle evidence".to_owned(),
        ],
        sections,
        vec![path.to_owned()],
        None,
        Some(arguments),
    )
}

fn mermaid_scope_graph(
    state: &AppState,
    arguments: &Value,
    path: &str,
    scope: ScopeBoundarySummary,
    max_nodes: usize,
) -> ToolResult {
    let mermaid = render_scope_mermaid(&scope, max_nodes);
    let top_findings = vec![format!(
        "{} files, {} internal edges, {} external importers, {} external dependencies",
        scope.file_count,
        scope.internal_edge_count,
        scope.inbound_external_count,
        scope.outbound_external_count
    )];

    let mut sections = BTreeMap::new();
    sections.insert(
        "diagram".to_owned(),
        json!({
            "format": "mermaid",
            "syntax": "flowchart",
            "content": mermaid,
            "hint": "Embed the `content` field in a fenced ```mermaid block to render in GitHub / GitLab / VS Code Markdown.",
        }),
    );
    sections.insert(
        "stats".to_owned(),
        json!({
            "target": path,
            "scope": scope.scope,
            "files_total": scope.file_count,
            "internal_edges_total": scope.internal_edge_count,
            "internal_edges_rendered": scope.internal_edges.len(),
            "external_importers_total": scope.inbound_external_count,
            "external_importers_rendered": scope.inbound_external.len(),
            "external_dependencies_total": scope.outbound_external_count,
            "external_dependencies_rendered": scope.outbound_external.len(),
            "external_affected_total": scope.affected_external_count,
            "external_affected_rendered": scope.affected_external.len(),
            "max_nodes_rendered": max_nodes,
            "truncated": scope.truncated,
        }),
    );
    sections.insert("top_files".to_owned(), json!(scope.top_files));
    sections.insert(
        "internal_edges".to_owned(),
        json!(
            scope
                .internal_edges
                .iter()
                .map(|(source, target)| json!({"source": source, "target": target}))
                .collect::<Vec<_>>()
        ),
    );

    make_handle_response(
        state,
        "mermaid_module_graph",
        stable_cache_key(
            "mermaid_module_graph",
            &json!({"path": path, "max_nodes": max_nodes, "scope_kind": "directory"}),
            &["path", "max_nodes", "scope_kind"],
        ),
        format!("Mermaid flowchart of directory dependency boundaries for `{path}`."),
        top_findings,
        0.88,
        vec![
            "Use module_boundary_report for the tabular scope evidence".to_owned(),
            "Inspect top_files before changing ownership boundaries".to_owned(),
        ],
        sections,
        vec![path.to_owned()],
        None,
        Some(arguments),
    )
}

fn render_scope_mermaid(scope: &ScopeBoundarySummary, max_nodes: usize) -> String {
    let mut out = String::from("flowchart LR\n");
    out.push_str("    classDef scope fill:#e8f4ff,stroke:#2563eb,stroke-width:2px\n");
    out.push_str("    classDef file fill:#eefbf3,stroke:#15803d\n");
    out.push_str("    classDef external fill:#fff7ed,stroke:#c2410c\n");
    out.push_str("    scope0[\"");
    out.push_str(&mermaid_escape_label(&scope.scope));
    out.push_str("\"]:::scope\n");

    if !scope.inbound_external.is_empty() {
        out.push_str(&format!(
            "    external_in[\"{} external importer(s)\"]:::external\n",
            scope.inbound_external.len()
        ));
        out.push_str("    external_in --> scope0\n");
    }
    if !scope.outbound_external.is_empty() {
        out.push_str(&format!(
            "    external_out[\"{} external dependency file(s)\"]:::external\n",
            scope.outbound_external.len()
        ));
        out.push_str("    scope0 --> external_out\n");
    }
    if !scope.affected_external.is_empty() {
        out.push_str(&format!(
            "    external_affected[\"{} externally affected file(s)\"]:::external\n",
            scope.affected_external.len()
        ));
        out.push_str("    scope0 --> external_affected\n");
    }

    let files: Vec<&str> = scope
        .top_files
        .iter()
        .filter_map(|entry| entry.get("file").and_then(Value::as_str))
        .take(max_nodes)
        .collect();
    for (idx, file) in files.iter().enumerate() {
        out.push_str(&format!(
            "    f{}[\"{}\"]:::file\n",
            idx,
            mermaid_escape_label(file_name(file))
        ));
        out.push_str(&format!("    scope0 --- f{idx}\n"));
    }

    for (source, target) in scope.internal_edges.iter().take(max_nodes) {
        let Some(source_idx) = files.iter().position(|file| *file == source) else {
            continue;
        };
        let Some(target_idx) = files.iter().position(|file| *file == target) else {
            continue;
        };
        out.push_str(&format!("    f{source_idx} --> f{target_idx}\n"));
    }

    out
}
