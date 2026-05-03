use crate::AppState;
use crate::tool_runtime::{ToolResult, required_string};
use crate::tools::report_contract::make_handle_response;
use crate::tools::report_utils::{stable_cache_key, strings_from_array};
use crate::tools::symbols::{semantic_results_for_query, semantic_status};
use codelens_engine::search::{SEMANTIC_COUPLING_THRESHOLD, SEMANTIC_NEW_RESULT_THRESHOLD};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use super::{
    ScopeBoundarySummary, build_dead_code_semantic_query, build_module_semantic_query,
    collect_scope_boundary_summary, insert_semantic_status, semantic_degraded_note,
};

#[allow(deprecated)]
pub fn module_boundary_report(state: &AppState, arguments: &Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    if let Some(scope) = collect_scope_boundary_summary(state, path)? {
        return module_scope_boundary_report(state, arguments, scope);
    }

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
        crate::util::push_unique_string(
            &mut next_actions,
            "Run index_embeddings before trusting semantic-only coupling hints",
        );
        crate::util::push_unique_string(&mut next_actions, note);
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
        Some(arguments),
    )
}

fn module_scope_boundary_report(
    state: &AppState,
    arguments: &Value,
    scope: ScopeBoundarySummary,
) -> ToolResult {
    let top_findings = vec![
        format!(
            "{} import-capable file(s), {} internal edge(s), {} external importer(s)",
            scope.file_count, scope.internal_edge_count, scope.inbound_external_count
        ),
        format!(
            "{} external dependency edge(s), {} externally affected file(s)",
            scope.outbound_external_count, scope.affected_external_count
        ),
    ];

    let mut sections = BTreeMap::new();
    sections.insert(
        "scope_summary".to_owned(),
        json!({
            "scope": scope.scope,
            "resolved_path": scope.resolved_path,
            "file_count": scope.file_count,
            "truncated": scope.truncated,
            "internal_edge_count": scope.internal_edge_count,
            "internal_edges_returned": scope.internal_edges.len(),
            "external_importer_count": scope.inbound_external_count,
            "external_importers_returned": scope.inbound_external.len(),
            "external_dependency_count": scope.outbound_external_count,
            "external_dependencies_returned": scope.outbound_external.len(),
            "external_affected_count": scope.affected_external_count,
            "external_affected_returned": scope.affected_external.len(),
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
    sections.insert(
        "external_importers".to_owned(),
        json!(scope.inbound_external),
    );
    sections.insert(
        "external_dependencies".to_owned(),
        json!(scope.outbound_external),
    );
    sections.insert(
        "external_affected".to_owned(),
        json!(scope.affected_external),
    );

    let final_semantic_status = semantic_status(state);
    insert_semantic_status(&mut sections, final_semantic_status.clone());
    let mut next_actions = vec![
        "Inspect high-score files before changing module ownership".to_owned(),
        "Check external importers before moving or deleting public module files".to_owned(),
    ];
    if let Some(note) = semantic_degraded_note(&final_semantic_status) {
        crate::util::push_unique_string(
            &mut next_actions,
            "Run index_embeddings before trusting semantic-only coupling hints",
        );
        crate::util::push_unique_string(&mut next_actions, note);
    }

    let path = required_string(arguments, "path")?;
    make_handle_response(
        state,
        "module_boundary_report",
        stable_cache_key(
            "module_boundary_report",
            &json!({"path": path, "scope_kind": "directory"}),
            &["path", "scope_kind"],
        ),
        format!("Directory boundary report for `{path}` with subtree import evidence."),
        top_findings,
        0.86,
        next_actions,
        sections,
        vec![path.to_owned()],
        None,
        Some(arguments),
    )
}

#[allow(deprecated)]
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
        crate::util::push_unique_string(
            &mut next_actions,
            "Run index_embeddings before trusting semantic duplicate or similarity evidence",
        );
        crate::util::push_unique_string(&mut next_actions, note);
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
        Some(arguments),
    )
}
