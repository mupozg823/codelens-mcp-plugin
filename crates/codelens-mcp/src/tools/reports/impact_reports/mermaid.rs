use crate::AppState;
use crate::tool_runtime::{ToolResult, required_string};
use crate::tools::report_contract::make_handle_response;
use crate::tools::report_utils::stable_cache_key;
use serde_json::{Value, json};
use std::collections::BTreeMap;

use super::workspace_modules::build_workspace_module_graph_report;
use super::{
    analysis_completeness_section, file_name, impact_entry_file, mermaid_escape_label, parent_dir,
    validate_architecture_scope, verifier_files_for_path,
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

pub(super) fn insert_module_diagram_sections(
    state: &AppState,
    path: &str,
    impact: &Value,
    max_nodes: usize,
    sections: &mut BTreeMap<String, Value>,
) -> String {
    let importers = impact
        .get("direct_importers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let downstream = impact
        .get("blast_radius")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let importer_count = importers.len();
    let downstream_count = downstream.len();
    let workspace_report =
        build_workspace_module_graph_report(&state.project(), path, impact, max_nodes);
    let (mermaid, top_finding, stats, module_graph) = if let Some(report) = workspace_report {
        (
            report.mermaid,
            report.top_finding,
            report.stats,
            Some(report.module_graph),
        )
    } else {
        (
            render_module_mermaid(path, &importers, &downstream, max_nodes),
            format!(
                "{} upstream, {} downstream (rendered up to {} per side)",
                importer_count, downstream_count, max_nodes
            ),
            json!({
                "target": path,
                "scope_kind": impact.get("scope_kind").and_then(Value::as_str).unwrap_or("file"),
                "granularity": "file_impact",
                "in_scope_file_count": impact.get("in_scope_file_count").cloned().unwrap_or(Value::Null),
                "in_scope_file_limit_hit": impact.get("in_scope_file_limit_hit").cloned().unwrap_or(Value::Null),
                "upstream_total": importer_count,
                "downstream_total": downstream_count,
                "max_nodes_rendered": max_nodes,
            }),
            None,
        )
    };

    sections.insert(
        "diagram".to_owned(),
        json!({
            "format": "mermaid",
            "syntax": "flowchart",
            "content": mermaid,
            "hint": "Embed the `content` field in a fenced ```mermaid block to render in GitHub / GitLab / VS Code Markdown.",
        }),
    );
    sections.insert("stats".to_owned(), stats);
    if let Some(module_graph) = module_graph {
        sections.insert("module_graph".to_owned(), module_graph);
    }
    top_finding
}

#[allow(deprecated)]
pub fn mermaid_module_graph(state: &AppState, arguments: &Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    let max_nodes = arguments
        .get("max_nodes")
        .and_then(Value::as_u64)
        .unwrap_or(10) as usize;

    validate_architecture_scope(&state.project(), path)?;

    let impact = crate::tools::graph::get_impact_analysis(
        state,
        &json!({"file_path": path, "max_depth": 2}),
    )?
    .0;

    let mut sections = BTreeMap::new();
    let top_finding =
        insert_module_diagram_sections(state, path, &impact, max_nodes, &mut sections);
    sections.insert(
        "analysis_completeness".to_owned(),
        analysis_completeness_section(&impact, false, false),
    );
    sections.insert("raw_impact".to_owned(), impact);

    make_handle_response(
        state,
        "mermaid_module_graph",
        stable_cache_key("mermaid_module_graph", arguments, &["path", "max_nodes"]),
        format!("Mermaid flowchart of module dependency boundaries for `{path}`."),
        vec![top_finding],
        0.90,
        vec![
            "Embed the diagram in a PR body to visualise module risk".to_owned(),
            "Call module_boundary_report for structural coupling + cycle evidence".to_owned(),
        ],
        sections,
        verifier_files_for_path(&state.project(), path),
        None,
        Some(arguments),
    )
}
