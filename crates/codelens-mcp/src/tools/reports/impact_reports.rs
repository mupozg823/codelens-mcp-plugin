use crate::tools::report_contract::make_handle_response;
use crate::tools::report_utils::{stable_cache_key, strings_from_array};
use crate::tools::symbols::{semantic_results_for_query, semantic_status};
use crate::tools::{required_string, AppState, ToolResult};
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
    sections.insert("symbols".to_owned(), symbols);

    // Semantic coupling: find symbols in other modules that are semantically similar
    // to this module's symbols — hidden coupling the import graph doesn't show.
    let module_name = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".rs")
        .trim_end_matches(".ts")
        .trim_end_matches(".tsx")
        .trim_end_matches(".py")
        .replace('_', " ");
    // Always attempt semantic query — get_or_init will lazy-load the embedding engine
    let sem_results = semantic_results_for_query(state, &module_name, 10, false);
    let semantic_coupling: Vec<Value> = sem_results
        .into_iter()
        .filter(|r| r.score > 0.12 && !r.file_path.contains(path))
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
            let results = semantic_results_for_query(state, name, 3, false);
            if results.is_empty() {
                return None;
            }
            let similar: Vec<Value> = results
                .into_iter()
                .filter(|r| r.score > 0.15)
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
        top_findings.push(format!("{path}: {affected} affected file(s)"));
        impact_rows.push(json!({
            "path": path,
            "affected_files": affected,
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

    // Always attempt — get_or_init lazy-loads embedding engine
    let semantic_related: Vec<Value> = target_files
        .iter()
        .take(3)
        .flat_map(|path| {
            let query = path
                .rsplit('/')
                .next()
                .unwrap_or(path)
                .trim_end_matches(".rs")
                .trim_end_matches(".ts")
                .trim_end_matches(".tsx")
                .trim_end_matches(".py")
                .replace('_', " ");
            semantic_results_for_query(state, &query, 5, false)
                .into_iter()
                .filter(|r| r.score > 0.12 && !graph_files.contains(&r.file_path))
                .map(|r| {
                    json!({
                        "source": path,
                        "related_file": r.file_path,
                        "related_symbol": r.symbol_name,
                        "semantic_score": (r.score * 1000.0).round() / 1000.0,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect();

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
