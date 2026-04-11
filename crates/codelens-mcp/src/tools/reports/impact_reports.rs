use crate::tool_runtime::{required_string, ToolResult};
use crate::tools::report_contract::make_handle_response;
use crate::tools::report_utils::{stable_cache_key, strings_from_array};
use crate::tools::symbols::{
    semantic_query_for_retrieval, semantic_results_for_query, semantic_status,
};
use crate::AppState;
use codelens_engine::search::{SEMANTIC_COUPLING_THRESHOLD, SEMANTIC_NEW_RESULT_THRESHOLD};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn semantic_status_is_ready(status: &Value) -> bool {
    status
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|value| value == "ready")
}

fn push_unique(items: &mut Vec<String>, item: impl Into<String>) {
    let item = item.into();
    if !items.iter().any(|existing| existing == &item) {
        items.push(item);
    }
}

fn semantic_degraded_note(status: &Value) -> Option<String> {
    if semantic_status_is_ready(status) {
        return None;
    }
    let reason = status
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("semantic enrichment unavailable");
    Some(format!(
        "Semantic enrichment unavailable; report uses structural evidence only. {reason}."
    ))
}

fn insert_semantic_status(sections: &mut BTreeMap<String, Value>, status: Value) {
    sections.insert("semantic_status".to_owned(), status);
}

fn path_hint(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".rs")
        .trim_end_matches(".ts")
        .trim_end_matches(".tsx")
        .trim_end_matches(".js")
        .trim_end_matches(".jsx")
        .trim_end_matches(".py")
        .trim_end_matches(".go")
        .replace(['_', '-'], " ")
}

fn build_module_semantic_query(path: &str, symbol_names: &[String]) -> String {
    let hint = path_hint(path);
    let query = if symbol_names.is_empty() {
        format!("module boundary responsibilities {hint}")
    } else {
        format!(
            "module boundary responsibilities {hint} {}",
            symbol_names.join(" ")
        )
    };
    semantic_query_for_retrieval(&query)
}

fn build_dead_code_semantic_query(name: &str, file: Option<&str>) -> String {
    let query = match file {
        Some(file) if !file.is_empty() => {
            format!("similar live code for {name} in {}", path_hint(file))
        }
        _ => format!("similar live code for {name}"),
    };
    semantic_query_for_retrieval(&query)
}

/// Extract a file-path-like string from an impact-analysis entry,
/// tolerating schemas that use `file`, `file_path`, or `path`.
fn impact_entry_file(value: &Value) -> Option<&str> {
    value
        .get("file")
        .and_then(Value::as_str)
        .or_else(|| value.get("file_path").and_then(Value::as_str))
        .or_else(|| value.get("path").and_then(Value::as_str))
}

/// Sanitise a label for safe embedding inside a Mermaid `["..."]` node body.
/// Mermaid does not accept unescaped double-quotes inside quoted labels.
fn mermaid_escape_label(raw: &str) -> String {
    raw.replace('"', "'")
}

/// Render a Mermaid `flowchart LR` diagram summarising direct importers
/// (upstream) and blast-radius dependencies (downstream) of a target file.
///
/// The output is pure text and can be embedded directly into a fenced
/// ```mermaid block to render in GitHub, GitLab, VS Code, or any Markdown
/// host that supports Mermaid. Both sides are capped independently by
/// `max_nodes` so the resulting diagram stays readable even on hub files
/// with hundreds of importers.
pub(crate) fn render_module_mermaid(
    target: &str,
    importers: &[Value],
    downstream: &[Value],
    max_nodes: usize,
) -> String {
    let mut out = String::from("flowchart LR\n");
    out.push_str(&format!(
        "    target0[\"{}\"]\n",
        mermaid_escape_label(target)
    ));

    for (idx, entry) in importers.iter().take(max_nodes).enumerate() {
        let file = impact_entry_file(entry).unwrap_or("<unknown>");
        out.push_str(&format!(
            "    up{idx}[\"{}\"]\n",
            mermaid_escape_label(file)
        ));
        out.push_str(&format!("    up{idx} --> target0\n"));
    }

    for (idx, entry) in downstream.iter().take(max_nodes).enumerate() {
        let file = impact_entry_file(entry).unwrap_or("<unknown>");
        out.push_str(&format!(
            "    down{idx}[\"{}\"]\n",
            mermaid_escape_label(file)
        ));
        out.push_str(&format!("    target0 --> down{idx}\n"));
    }

    out
}

pub fn mermaid_module_graph(state: &AppState, arguments: &Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    let max_nodes = arguments
        .get("max_nodes")
        .and_then(Value::as_u64)
        .unwrap_or(10) as usize;

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
    )
}

pub fn module_boundary_report(state: &AppState, arguments: &Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    let impact = crate::tools::graph::get_impact_analysis(
        state,
        &json!({"file_path": path, "max_depth": 2}),
    )
    .map(|out| out.0)
    .unwrap_or_else(|_| json!({"blast_radius": [], "direct_importers": []}));
    let cycles =
        crate::tools::graph::find_circular_dependencies_tool(state, &json!({"max_results": 20}))?.0;
    let coupling =
        crate::tools::graph::get_change_coupling_tool(state, &json!({"max_results": 20}))?.0;
    let symbols =
        crate::tools::symbols::get_symbols_overview(state, &json!({"path": path, "depth": 1}))
            .map(|out| out.0)
            .unwrap_or_else(|_| json!({"symbols": []}));

    let cycle_hits = cycles
        .get("cycles")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|cycle| cycle.to_string().contains(path))
        .take(5)
        .collect::<Vec<_>>();
    let coupling_hits = coupling
        .get("results")
        .and_then(|v| v.as_array())
        .or_else(|| coupling.get("couplings").and_then(|v| v.as_array()))
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.to_string().contains(path))
        .take(5)
        .collect::<Vec<_>>();

    let top_findings = vec![format!(
        "{} importer(s), {} impacted file(s), {} cycle hit(s)",
        impact
            .get("direct_importers")
            .and_then(|v| v.as_array())
            .map(|v| v.len())
            .unwrap_or_default(),
        impact
            .get("total_affected_files")
            .and_then(|v| v.as_u64())
            .unwrap_or_default(),
        cycle_hits.len()
    )];
    let mut sections = BTreeMap::new();
    sections.insert("impact".to_owned(), impact);
    sections.insert(
        "cycle_hits".to_owned(),
        json!({ "path": path, "cycles": cycle_hits }),
    );
    sections.insert(
        "coupling_hits".to_owned(),
        json!({ "path": path, "couplings": coupling_hits }),
    );
    // Extract symbol names BEFORE moving `symbols` into sections
    let symbol_names: Vec<String> = symbols
        .get("symbols")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| s.get("name").and_then(|n| n.as_str()).map(|n| n.to_owned()))
                .take(5)
                .collect()
        })
        .unwrap_or_default();
    sections.insert("symbols".to_owned(), symbols);

    let module_query = build_module_semantic_query(path, &symbol_names);
    let sem_results = semantic_results_for_query(state, &module_query, 10, false);
    let semantic_coupling: Vec<Value> = sem_results
        .into_iter()
        .filter(|r| r.score > SEMANTIC_COUPLING_THRESHOLD && !r.file_path.contains(path))
        .take(5)
        .map(|r| {
            json!({
                "external_symbol": r.symbol_name,
                "external_file": r.file_path,
                "semantic_score": (r.score * 1000.0).round() / 1000.0,
            })
        })
        .collect();
    if !semantic_coupling.is_empty() {
        sections.insert(
            "semantic_coupling".to_owned(),
            json!({"hint": "Semantically similar symbols outside this module — potential hidden coupling", "matches": semantic_coupling}),
        );
    }
    let final_semantic_status = semantic_status(state);
    insert_semantic_status(&mut sections, final_semantic_status.clone());
    let mut next_actions = vec!["Check cycle hits before moving ownership boundaries".to_owned()];
    if let Some(note) = semantic_degraded_note(&final_semantic_status) {
        push_unique(
            &mut next_actions,
            "Run index_embeddings before trusting semantic-only coupling hints",
        );
        push_unique(&mut next_actions, note);
    }
    make_handle_response(
        state,
        "module_boundary_report",
        stable_cache_key("module_boundary_report", arguments, &["path"]),
        format!("Module boundary report for `{path}` with inbound/outbound and structural risk."),
        top_findings,
        0.87,
        next_actions,
        sections,
        vec![path.to_owned()],
        None,
    )
}

pub fn dead_code_report(state: &AppState, arguments: &Value) -> ToolResult {
    let scope = arguments
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or(".");
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(20);
    let dead_code =
        crate::tools::graph::find_dead_code_v2_tool(state, &json!({"max_results": max_results}))?.0;
    let candidates = dead_code
        .get("dead_code")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.to_string().contains(scope))
        .take(10)
        .collect::<Vec<_>>();
    // Semantic enrichment: for each dead code candidate, find similar live symbols
    // to help verify it's truly unused (not just unreferenced by a different name).
    // Always attempt semantic query — lazy-loads embedding engine via get_or_init
    let semantic_hints: Vec<Value> = candidates
        .iter()
        .filter_map(|entry| {
            let name = entry
                .get("name")
                .or_else(|| entry.get("symbol"))
                .and_then(|v| v.as_str())?;
            let file = entry
                .get("file")
                .or_else(|| entry.get("file_path"))
                .and_then(|v| v.as_str());
            let query = build_dead_code_semantic_query(name, file);
            let results = semantic_results_for_query(state, &query, 3, false);
            if results.is_empty() {
                return None;
            }
            let similar: Vec<Value> = results
                .into_iter()
                .filter(|r| r.score > SEMANTIC_NEW_RESULT_THRESHOLD)
                .map(|r| {
                    json!({
                        "symbol": r.symbol_name,
                        "file": r.file_path,
                        "score": (r.score * 1000.0).round() / 1000.0,
                    })
                })
                .collect();
            if similar.is_empty() {
                return None;
            }
            Some(json!({"dead_symbol": name, "similar_live_symbols": similar}))
        })
        .collect();

    let top_findings = strings_from_array(Some(&candidates), "file", 3);
    let mut sections = BTreeMap::new();
    sections.insert(
        "candidates".to_owned(),
        json!({"scope": scope, "dead_code": candidates}),
    );
    if !semantic_hints.is_empty() {
        sections.insert(
            "semantic_similar_live".to_owned(),
            json!({"hint": "Dead symbols with similar live code — verify before deleting", "matches": semantic_hints}),
        );
    }
    sections.insert("raw_dead_code".to_owned(), dead_code);
    let final_semantic_status = semantic_status(state);
    insert_semantic_status(&mut sections, final_semantic_status.clone());
    let mut next_actions =
        vec!["Validate runtime entry points before deleting candidates".to_owned()];
    if let Some(note) = semantic_degraded_note(&final_semantic_status) {
        push_unique(
            &mut next_actions,
            "Run index_embeddings before trusting semantic duplicate or similarity evidence",
        );
        push_unique(&mut next_actions, note);
    }
    make_handle_response(
        state,
        "dead_code_report",
        stable_cache_key("dead_code_report", arguments, &["scope", "max_results"]),
        format!("Bounded dead-code audit for scope `{scope}`."),
        top_findings,
        0.84,
        next_actions,
        sections,
        if scope == "." {
            Vec::new()
        } else {
            vec![scope.to_owned()]
        },
        None,
    )
}

pub fn impact_report(state: &AppState, arguments: &Value) -> ToolResult {
    let changed_files = strings_from_array(
        arguments
            .get("changed_files")
            .and_then(|value| value.as_array()),
        "file",
        8,
    );
    let target_files = if !changed_files.is_empty() {
        changed_files
    } else if let Some(path) = arguments.get("path").and_then(|value| value.as_str()) {
        vec![path.to_owned()]
    } else {
        let changed = crate::tools::graph::get_changed_files_tool(
            state,
            &json!({"include_untracked": true}),
        )?
        .0;
        strings_from_array(
            changed.get("files").and_then(|value| value.as_array()),
            "file",
            8,
        )
    };

    // Pre-compute change_kind for all target files to avoid repeated git calls inside the loop.
    let project = state.project();
    let change_kinds: std::collections::HashMap<&str, String> = target_files
        .iter()
        .take(5)
        .map(|p| {
            (
                p.as_str(),
                codelens_engine::git::classify_change_kind(&project, p),
            )
        })
        .collect();

    let mut impact_rows = Vec::new();
    let mut top_findings = Vec::new();
    for path in target_files.iter().take(5) {
        let impact = crate::tools::graph::get_impact_analysis(
            state,
            &json!({"file_path": path, "max_depth": 2}),
        )
        .map(|output| output.0)
        .unwrap_or_else(
            |_| json!({"file_path": path, "total_affected_files": 0, "direct_importers": []}),
        );
        let affected = impact
            .get("total_affected_files")
            .and_then(|value| value.as_u64())
            .unwrap_or_default();
        let change_kind = change_kinds
            .get(path.as_str())
            .cloned()
            .unwrap_or_else(|| "mixed".to_owned());
        let kind_label = if change_kind == "additive" {
            " (additive)"
        } else {
            ""
        };
        top_findings.push(format!("{path}: {affected} affected file(s){kind_label}"));
        impact_rows.push(json!({
            "path": path,
            "affected_files": affected,
            "change_kind": change_kind,
            "direct_importers": impact.get("direct_importers").cloned().unwrap_or(json!([])),
            "blast_radius": impact.get("blast_radius").cloned().unwrap_or(json!([])),
        }));
    }

    // Semantic enrichment: find files semantically related to changed files
    // that the import graph might miss (e.g., similar patterns, shared concepts).
    let graph_files: std::collections::HashSet<String> = impact_rows
        .iter()
        .flat_map(|row| {
            let mut files = Vec::new();
            if let Some(path) = row.get("path").and_then(|v| v.as_str()) {
                files.push(path.to_owned());
            }
            if let Some(importers) = row.get("direct_importers").and_then(|v| v.as_array()) {
                for imp in importers {
                    if let Some(f) = imp
                        .as_str()
                        .or_else(|| imp.get("file").and_then(|v| v.as_str()))
                    {
                        files.push(f.to_owned());
                    }
                }
            }
            files
        })
        .collect();

    // Batch semantic enrichment: collect symbols from up to 3 files, then issue
    // a single combined query instead of per-file calls.
    let batch_files: Vec<&String> = target_files.iter().take(3).collect();
    let mut all_symbol_names: Vec<String> = Vec::new();
    let mut batch_file_set: Vec<String> = Vec::new();
    for path in &batch_files {
        batch_file_set.push((*path).clone());
        let names: Vec<String> =
            crate::tools::symbols::get_symbols_overview(state, &json!({"path": path, "depth": 1}))
                .ok()
                .and_then(|out| {
                    out.0.get("symbols").and_then(|v| v.as_array()).map(|arr| {
                        arr.iter()
                            .filter_map(|s| s.get("name").and_then(|n| n.as_str()))
                            .take(5)
                            .map(ToOwned::to_owned)
                            .collect::<Vec<_>>()
                    })
                })
                .unwrap_or_default();
        all_symbol_names.extend(names);
    }
    all_symbol_names.sort_unstable();
    all_symbol_names.dedup();
    let combined_query = all_symbol_names.join(" ");
    let semantic_related: Vec<Value> = if combined_query.is_empty() {
        Vec::new()
    } else {
        semantic_results_for_query(state, &combined_query, 15, false)
            .into_iter()
            .filter(|r| {
                r.score > SEMANTIC_COUPLING_THRESHOLD
                    && !graph_files.contains(&r.file_path)
                    && !batch_file_set.contains(&r.file_path)
            })
            .take(10)
            .map(|r| {
                json!({
                    "related_file": r.file_path,
                    "related_symbol": r.symbol_name,
                    "semantic_score": (r.score * 1000.0).round() / 1000.0,
                })
            })
            .collect()
    };

    let mut sections = BTreeMap::new();
    sections.insert(
        "impact_rows".to_owned(),
        json!({"files": target_files, "impacts": impact_rows}),
    );
    if !semantic_related.is_empty() {
        sections.insert(
            "semantic_related".to_owned(),
            json!({"hint": "Files semantically related but not in import graph", "matches": semantic_related}),
        );
    }
    let final_semantic_status = semantic_status(state);
    insert_semantic_status(&mut sections, final_semantic_status.clone());
    let mut next_actions =
        vec!["Expand only the highest-impact file before deeper review".to_owned()];
    if let Some(note) = semantic_degraded_note(&final_semantic_status) {
        push_unique(
            &mut next_actions,
            "Run index_embeddings before trusting semantic-only related-file hints",
        );
        push_unique(&mut next_actions, note);
    }
    make_handle_response(
        state,
        "impact_report",
        stable_cache_key("impact_report", arguments, &["path", "changed_files"]),
        "Diff-aware impact report with bounded blast radius and importer evidence.".to_owned(),
        top_findings,
        0.88,
        next_actions,
        sections,
        target_files,
        None,
    )
}

// Historical layout: helper production functions follow the test module.
// Reordering would churn ~330 lines of unrelated code; keep the allow local.
#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        build_dead_code_semantic_query, build_module_semantic_query, impact_entry_file,
        mermaid_escape_label, render_module_mermaid,
    };
    use serde_json::json;

    #[test]
    fn module_semantic_query_keeps_module_intent() {
        let query = build_module_semantic_query(
            "crates/codelens-mcp/src/dispatch.rs",
            &["dispatch_tool".to_string(), "semantic_search".to_string()],
        );
        assert!(query.contains("module boundary responsibilities"));
        assert!(query.contains("dispatch"));
        assert!(query.contains("dispatch_tool"));
    }

    #[test]
    fn dead_code_semantic_query_uses_symbol_and_file_hint() {
        let query = build_dead_code_semantic_query("rename_symbol", Some("src/rename.rs"));
        assert!(query.contains("similar live code for"));
        assert!(query.contains("rename_symbol"));
        assert!(query.contains("rename"));
    }

    #[test]
    fn mermaid_header_and_target_node_are_first() {
        let out = render_module_mermaid("src/foo.rs", &[], &[], 10);
        assert!(out.starts_with("flowchart LR\n"));
        assert!(out.contains("target0[\"src/foo.rs\"]"));
        // Zero upstream / downstream → only the target node, no edges.
        assert!(!out.contains("-->"));
    }

    #[test]
    fn mermaid_renders_upstream_and_downstream_edges() {
        let importers = vec![
            json!({"file": "src/a.rs"}),
            json!({"file_path": "src/b.rs"}),
        ];
        let downstream = vec![json!({"path": "src/c.rs"})];
        let out = render_module_mermaid("src/target.rs", &importers, &downstream, 10);

        assert!(out.contains("src/a.rs"));
        assert!(out.contains("src/b.rs"));
        assert!(out.contains("src/c.rs"));
        assert!(out.contains("up0 --> target0"));
        assert!(out.contains("up1 --> target0"));
        assert!(out.contains("target0 --> down0"));
    }

    #[test]
    fn mermaid_respects_max_nodes_cap_per_side() {
        let importers: Vec<serde_json::Value> = (0..20)
            .map(|i| json!({"file": format!("src/a{i}.rs")}))
            .collect();
        let out = render_module_mermaid("src/target.rs", &importers, &[], 5);
        assert!(out.contains("up0["));
        assert!(out.contains("up4["));
        // Node index 5 must be capped out.
        assert!(!out.contains("up5["));
        // Exactly 5 upstream edges.
        let edges = out.matches("--> target0").count();
        assert_eq!(edges, 5);
    }

    #[test]
    fn mermaid_escapes_double_quotes_in_labels() {
        let importers = vec![json!({ "file": r#"src/weird"path.rs"# })];
        let out = render_module_mermaid("src/target.rs", &importers, &[], 10);
        // Raw double quote inside the importer label must be replaced.
        assert!(!out.contains(r#"weird"path.rs"#));
        assert!(out.contains("weird'path.rs"));
    }

    #[test]
    fn mermaid_handles_missing_file_field_gracefully() {
        let importers = vec![json!({"unexpected": 42})];
        let out = render_module_mermaid("src/target.rs", &importers, &[], 10);
        assert!(out.contains("<unknown>"));
        assert!(out.contains("up0 --> target0"));
    }

    #[test]
    fn impact_entry_file_prefers_file_over_fallbacks() {
        let v = json!({"file": "a.rs", "file_path": "b.rs", "path": "c.rs"});
        assert_eq!(impact_entry_file(&v), Some("a.rs"));
        let v2 = json!({"file_path": "b.rs", "path": "c.rs"});
        assert_eq!(impact_entry_file(&v2), Some("b.rs"));
        let v3 = json!({"path": "c.rs"});
        assert_eq!(impact_entry_file(&v3), Some("c.rs"));
        let v4 = json!({});
        assert_eq!(impact_entry_file(&v4), None);
    }

    #[test]
    fn mermaid_escape_label_replaces_quotes_only() {
        assert_eq!(mermaid_escape_label("plain"), "plain");
        assert_eq!(mermaid_escape_label(r#"with "quote""#), "with 'quote'");
        assert_eq!(mermaid_escape_label(""), "");
    }
}

pub fn refactor_safety_report(state: &AppState, arguments: &Value) -> ToolResult {
    let path = arguments
        .get("path")
        .and_then(|value| value.as_str())
        .unwrap_or(".");
    let task = arguments.get("task").and_then(|value| value.as_str());
    let symbol = arguments.get("symbol").and_then(|value| value.as_str());

    let boundary = module_boundary_report(state, &json!({"path": path}))?.0;
    let symbol_impact = if let Some(symbol) = symbol {
        super::summarize_symbol_impact(
            state,
            &json!({"symbol": symbol, "file_path": arguments.get("file_path").and_then(|v| v.as_str())}),
        )
        .map(|output| output.0)
        .unwrap_or_else(|error| json!({"symbol": symbol, "error": error.to_string()}))
    } else {
        json!({"skipped": true, "reason": "no symbol provided"})
    };
    let change_request = task
        .map(|task| {
            super::analyze_change_request(state, &json!({"task": task})).map(|output| output.0)
        })
        .transpose()?
        .unwrap_or_else(|| json!({"skipped": true, "reason": "no task provided"}));
    let tests =
        crate::tools::filesystem::find_tests(state, &json!({"path": path, "max_results": 10}))
            .map(|output| output.0)
            .unwrap_or_else(|_| json!({"tests": []}));

    let mut top_findings = Vec::new();
    if let Some(symbol) = symbol {
        top_findings.push(format!(
            "Validate symbol-level callers before refactoring `{symbol}`."
        ));
    }
    if let Some(task) = task {
        top_findings.push(format!("Keep the refactor aligned with `{task}`."));
    }
    top_findings.push(format!(
        "Check tests around `{path}` before applying broad edits."
    ));

    let mut sections = BTreeMap::new();
    sections.insert("module_boundary".to_owned(), boundary);
    sections.insert("symbol_impact".to_owned(), symbol_impact);
    sections.insert("change_request".to_owned(), change_request);
    sections.insert("related_tests".to_owned(), tests);
    let status = semantic_status(state);
    insert_semantic_status(&mut sections, status.clone());
    let mut next_actions =
        vec!["Use safe_rename_report or focused edits only after checking blockers".to_owned()];
    if let Some(note) = semantic_degraded_note(&status) {
        push_unique(
            &mut next_actions,
            "Run index_embeddings before trusting semantic-enriched report sections",
        );
        push_unique(&mut next_actions, note);
    }
    make_handle_response(
        state,
        "refactor_safety_report",
        stable_cache_key(
            "refactor_safety_report",
            arguments,
            &["task", "symbol", "path", "file_path"],
        ),
        format!("Preview-first refactor safety report for `{path}`."),
        top_findings,
        0.9,
        next_actions,
        sections,
        vec![arguments
            .get("file_path")
            .and_then(|value| value.as_str())
            .unwrap_or(path)
            .to_owned()],
        symbol.map(ToOwned::to_owned),
    )
}

pub fn diff_aware_references(state: &AppState, arguments: &Value) -> ToolResult {
    let changed_files = strings_from_array(
        arguments
            .get("changed_files")
            .and_then(|value| value.as_array()),
        "file",
        8,
    );
    let changed_files = if changed_files.is_empty() {
        let changed = crate::tools::graph::get_changed_files_tool(
            state,
            &json!({"include_untracked": true}),
        )?
        .0;
        strings_from_array(
            changed.get("files").and_then(|value| value.as_array()),
            "file",
            8,
        )
    } else {
        changed_files
    };

    let mut rows = Vec::new();
    let mut top_findings = Vec::new();
    for path in changed_files.iter().take(5) {
        let symbols =
            crate::tools::symbols::get_symbols_overview(state, &json!({"path": path, "depth": 1}))
                .map(|output| output.0)
                .unwrap_or_else(|_| json!({"symbols": []}));
        let symbol_names = symbols
            .get("symbols")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .take(3)
            .filter_map(|entry| {
                entry
                    .get("name")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned)
            })
            .collect::<Vec<_>>();
        let mut reference_hits = Vec::new();
        for symbol_name in &symbol_names {
            let refs = crate::tools::graph::find_scoped_references_tool(
                state,
                &json!({"symbol_name": symbol_name, "file_path": path, "max_results": 20}),
            )
            .map(|output| output.0)
            .unwrap_or_else(|_| json!({"references": [], "count": 0}));
            let count = refs
                .get("count")
                .and_then(|value| value.as_u64())
                .unwrap_or_default();
            reference_hits.push(json!({"symbol": symbol_name, "count": count, "references": refs.get("references").cloned().unwrap_or(json!([]))}));
            top_findings.push(format!(
                "{path}: `{symbol_name}` has {count} classified reference(s)"
            ));
        }
        rows.push(json!({
            "path": path,
            "symbols": symbol_names,
            "reference_hits": reference_hits,
        }));
    }

    let mut sections = BTreeMap::new();
    sections.insert(
        "diff_references".to_owned(),
        json!({"changed_files": changed_files, "rows": rows}),
    );
    make_handle_response(
        state,
        "diff_aware_references",
        stable_cache_key("diff_aware_references", arguments, &["changed_files"]),
        "Diff-aware reference compression for reviewer and CI flows.".to_owned(),
        top_findings.into_iter().take(5).collect(),
        0.86,
        vec!["Expand only the changed file with the highest reference count".to_owned()],
        sections,
        changed_files,
        None,
    )
}

/// Semantic code review: analyze changed files using symbol references,
/// embedding similarity, and call graph to produce structured review comments.
pub fn semantic_code_review(state: &AppState, arguments: &Value) -> ToolResult {
    let changed_files = strings_from_array(
        arguments
            .get("changed_files")
            .and_then(|value| value.as_array()),
        "file",
        8,
    );
    let changed_files = if changed_files.is_empty() {
        let changed = crate::tools::graph::get_changed_files_tool(
            state,
            &json!({"include_untracked": true}),
        )?
        .0;
        strings_from_array(
            changed.get("files").and_then(|value| value.as_array()),
            "file",
            8,
        )
    } else {
        changed_files
    };

    if changed_files.is_empty() {
        let sections = BTreeMap::new();
        return make_handle_response(
            state,
            "semantic_code_review",
            None,
            "No changed files found. Pass changed_files or have uncommitted changes.".to_owned(),
            Vec::new(),
            0.5,
            vec!["Pass changed_files explicitly or make changes before running review".to_owned()],
            sections,
            Vec::new(),
            None,
        );
    }

    let sem_status = semantic_status(state);
    let semantic_available = sem_status
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s == "ready")
        .unwrap_or(false);

    let mut review_items = Vec::new();
    let mut top_findings = Vec::new();

    for path in changed_files.iter().take(5) {
        // 1. Get symbols in the changed file
        let symbols =
            crate::tools::symbols::get_symbols_overview(state, &json!({"path": path, "depth": 1}))
                .map(|o| o.0)
                .unwrap_or_else(|_| json!({"symbols": []}));
        let symbol_names: Vec<String> = symbols
            .get("symbols")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .take(5)
            .filter_map(|e| {
                e.get("name")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned)
            })
            .collect();

        // 2. For each symbol, get reference count + semantic similarity
        let mut symbol_reviews = Vec::new();
        for symbol_name in &symbol_names {
            let refs = crate::tools::graph::find_scoped_references_tool(
                state,
                &json!({"symbol_name": symbol_name, "file_path": path, "max_results": 10}),
            )
            .map(|o| o.0)
            .unwrap_or_else(|_| json!({"count": 0}));
            let ref_count = refs.get("count").and_then(|v| v.as_u64()).unwrap_or(0);

            // Semantic: find related symbols via embedding
            let semantic_matches = if semantic_available {
                let query = format!("{symbol_name} in {path}");
                semantic_results_for_query(state, &query, 3, false)
            } else {
                Vec::new()
            };
            let related: Vec<Value> = semantic_matches
                .iter()
                .filter(|m| m.file_path != *path) // exclude self
                .take(3)
                .map(|m| {
                    json!({
                        "symbol": m.symbol_name,
                        "file": m.file_path,
                        "similarity": (m.score * 1000.0).round() / 1000.0,
                    })
                })
                .collect();

            // Risk: high ref count + semantically similar symbols in other files = high risk
            let risk = if ref_count > 10 || related.len() >= 2 {
                "high"
            } else if ref_count > 3 || !related.is_empty() {
                "medium"
            } else {
                "low"
            };

            symbol_reviews.push(json!({
                "symbol": symbol_name,
                "reference_count": ref_count,
                "risk": risk,
                "semantically_related": related,
            }));

            if risk == "high" {
                top_findings.push(format!(
                    "{path}: `{symbol_name}` is high-risk ({ref_count} refs, {} related symbols)",
                    related.len()
                ));
            }
        }

        review_items.push(json!({
            "file": path,
            "symbols_reviewed": symbol_reviews.len(),
            "reviews": symbol_reviews,
        }));
    }

    let summary = format!(
        "Semantic code review of {} file(s) with {} symbol(s) analyzed.",
        changed_files.len().min(5),
        review_items
            .iter()
            .map(|r| r["symbols_reviewed"].as_u64().unwrap_or(0))
            .sum::<u64>()
    );

    let mut sections = BTreeMap::new();
    sections.insert("review_items".to_owned(), json!({"files": review_items}));
    sections.insert("semantic_status".to_owned(), sem_status);

    make_handle_response(
        state,
        "semantic_code_review",
        stable_cache_key("semantic_code_review", arguments, &["changed_files"]),
        summary,
        top_findings.into_iter().take(5).collect(),
        0.87,
        vec![
            "Review high-risk symbols first".to_owned(),
            "Check semantically related symbols for consistency".to_owned(),
        ],
        sections,
        changed_files,
        None,
    )
}
