use crate::AppState;
use crate::tool_runtime::{ToolResult, required_string};
use crate::tools::report_contract::make_handle_response;
use crate::tools::report_utils::{stable_cache_key, strings_from_array};
use crate::tools::semantic_retriever::{semantic_results_for_query, semantic_status};
use codelens_engine::search::{SEMANTIC_COUPLING_THRESHOLD, SEMANTIC_NEW_RESULT_THRESHOLD};
use codelens_engine::{
    CircularDependency, CouplingEntry, find_circular_dependencies, get_change_coupling,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use super::mermaid::insert_module_diagram_sections;
use super::{
    analysis_completeness_section, build_dead_code_semantic_query, build_module_semantic_query,
    insert_semantic_status, semantic_degraded_note, validate_architecture_scope,
    verifier_files_for_path,
};

const SCOPED_EVIDENCE_LIMIT: usize = 5;

#[allow(deprecated)]
pub fn module_boundary_report(state: &AppState, arguments: &Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    let include_diagram = arguments
        .get("include_diagram")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_nodes = arguments
        .get("max_nodes")
        .and_then(Value::as_u64)
        .unwrap_or(10) as usize;
    validate_architecture_scope(&state.project(), path)?;
    let impact_arguments = json!({"file_path": path, "max_depth": 2});
    #[cfg(test)]
    let impact_arguments = {
        let mut test_arguments = impact_arguments;
        if let Some(limit) = arguments
            .get("_test_directory_file_limit")
            .and_then(Value::as_u64)
        {
            test_arguments["_test_directory_file_limit"] = json!(limit);
        }
        test_arguments
    };
    let impact = crate::tools::graph::get_impact_analysis(state, &impact_arguments)?.0;
    let cycles = find_circular_dependencies(&state.project(), 0, &state.graph_cache())?;
    let total_cycles = cycles.len();
    let coupling = get_change_coupling(&state.project(), 6, 0.3, 3, 0)?;
    let total_couplings = coupling.len();
    let symbols =
        crate::tools::symbols::get_symbols_overview(state, &json!({"path": path, "depth": 1}))?.0;

    let (cycle_hits, cycle_limit_hit) = cycle_hits_for_path(&cycles, path, SCOPED_EVIDENCE_LIMIT);
    let (coupling_hits, coupling_limit_hit) =
        coupling_hits_for_path(&coupling, path, SCOPED_EVIDENCE_LIMIT);

    let top_findings = vec![format!(
        "{} importer(s), {} impacted file(s), {} cycle hit(s), {} temporal coupling hit(s)",
        impact
            .get("direct_importers")
            .and_then(|v| v.as_array())
            .map(|v| v.len())
            .unwrap_or_default(),
        impact
            .get("total_affected_files")
            .and_then(|v| v.as_u64())
            .unwrap_or_default(),
        cycle_hits.len(),
        coupling_hits.len()
    )];
    let mut sections = BTreeMap::new();
    sections.insert(
        "analysis_completeness".to_owned(),
        analysis_completeness_section(&impact, cycle_limit_hit, coupling_limit_hit),
    );
    if include_diagram {
        insert_module_diagram_sections(state, path, &impact, max_nodes, &mut sections);
    }
    sections.insert("impact".to_owned(), impact);
    sections.insert(
        "cycle_hits".to_owned(),
        json!({
            "path": path,
            "total_project_cycles": total_cycles,
            "result_limit": SCOPED_EVIDENCE_LIMIT,
            "limit_hit": cycle_limit_hit,
            "cycles": cycle_hits,
        }),
    );
    sections.insert(
        "coupling_hits".to_owned(),
        json!({
            "path": path,
            "source": "git_temporal_coupling",
            "status": coupling_status(total_couplings, coupling_hits.len()),
            "total_couplings": total_couplings,
            "result_limit": SCOPED_EVIDENCE_LIMIT,
            "limit_hit": coupling_limit_hit,
            "couplings": coupling_hits,
            "note": "This section is temporal co-change evidence only. Empty couplings do not mean semantic analysis is unavailable; inspect semantic_coupling and semantic_status for embedding evidence.",
        }),
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

    let final_semantic_status = semantic_status(state);
    let sem_results = if semantic_status_name(&final_semantic_status) == "ready" {
        let module_query = build_module_semantic_query(path, &symbol_names);
        semantic_results_for_query(state, &module_query, 10, false, None)
    } else {
        Vec::new()
    };
    let semantic_result_count = sem_results.len();
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
    sections.insert(
        "semantic_coupling".to_owned(),
        semantic_coupling_section(
            &final_semantic_status,
            semantic_result_count,
            semantic_coupling,
        ),
    );
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
        stable_cache_key(
            "module_boundary_report",
            arguments,
            &["path", "include_diagram", "max_nodes"],
        ),
        format!("Module boundary report for `{path}` with inbound/outbound and structural risk."),
        top_findings,
        0.87,
        next_actions,
        sections,
        verifier_files_for_path(&state.project(), path),
        None,
        Some(arguments),
    )
}

fn file_matches_scope(file: &str, scope: &str) -> bool {
    let file = file.trim_start_matches("./");
    let scope = scope.trim_start_matches("./").trim_end_matches('/');
    scope.is_empty()
        || scope == "."
        || file == scope
        || file
            .strip_prefix(scope)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn cycle_hits_for_path(
    entries: &[CircularDependency],
    path: &str,
    limit: usize,
) -> (Vec<Value>, bool) {
    let mut hits = entries
        .iter()
        .filter(|entry| {
            entry
                .cycle
                .iter()
                .any(|file| file_matches_scope(file, path))
        })
        .map(|entry| json!(entry))
        .take(limit.saturating_add(1))
        .collect::<Vec<_>>();
    let limit_hit = hits.len() > limit;
    hits.truncate(limit);
    (hits, limit_hit)
}

fn coupling_hits_for_path(
    entries: &[CouplingEntry],
    path: &str,
    limit: usize,
) -> (Vec<Value>, bool) {
    let mut hits = entries
        .iter()
        .filter(|entry| {
            file_matches_scope(&entry.file_a, path) || file_matches_scope(&entry.file_b, path)
        })
        .map(|entry| json!(entry))
        .take(limit.saturating_add(1))
        .collect::<Vec<_>>();
    let limit_hit = hits.len() > limit;
    hits.truncate(limit);
    (hits, limit_hit)
}

fn coupling_status(total_couplings: usize, hit_count: usize) -> &'static str {
    if hit_count > 0 {
        "matched"
    } else if total_couplings > 0 {
        "no_path_matches"
    } else {
        "empty_git_history"
    }
}

fn semantic_status_name(status: &Value) -> &str {
    status
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unavailable")
}

fn semantic_coupling_section(
    semantic_status: &Value,
    semantic_result_count: usize,
    matches: Vec<Value>,
) -> Value {
    let status_name = semantic_status_name(semantic_status);
    let coupling_status = if status_name == "ready" {
        if matches.is_empty() {
            "ready_no_external_matches"
        } else {
            "ready_matches"
        }
    } else {
        "semantic_unavailable"
    };
    json!({
        "status": coupling_status,
        "semantic_status": semantic_status,
        "semantic_result_count": semantic_result_count,
        "hint": "Semantically similar symbols outside this module — potential hidden coupling",
        "matches": matches,
        "note": "Empty matches means no external semantic coupling above the reporting threshold for this query, not that semantic analysis is impossible.",
    })
}

#[allow(deprecated)]
pub fn dead_code_report(state: &AppState, arguments: &Value) -> ToolResult {
    // Accept `path` as a soft alias of `scope`. The rest of the composite
    // report family (`module_boundary_report`, `impact_report`, …) takes a
    // `path` argument; without this fallback a caller copy-pasting that
    // convention silently scans the whole project root (issue G1, 2026-05-18
    // self-dogfood).
    let scope = arguments
        .get("scope")
        .or_else(|| arguments.get("path"))
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
    // Only load the embedding engine when an existing compatible index is ready.
    let final_semantic_status = semantic_status(state);
    let semantic_hints: Vec<Value> = if semantic_status_name(&final_semantic_status) == "ready" {
        candidates
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
                let results = semantic_results_for_query(state, &query, 3, false, None);
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
            .collect()
    } else {
        Vec::new()
    };

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
        stable_cache_key(
            "dead_code_report",
            arguments,
            &["scope", "path", "max_results"],
        ),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coupling_hits_for_path_reads_coupling_entries_directly() {
        let entries = vec![
            CouplingEntry {
                file_a: "src/billing/service.rs".to_owned(),
                file_b: "src/billing/repository.rs".to_owned(),
                co_changes: 4,
                total_changes_a: 5,
                total_changes_b: 6,
                strength: 0.66,
            },
            CouplingEntry {
                file_a: "src/auth/session.rs".to_owned(),
                file_b: "src/auth/token.rs".to_owned(),
                co_changes: 3,
                total_changes_a: 4,
                total_changes_b: 4,
                strength: 0.75,
            },
        ];

        let (hits, limit_hit) = coupling_hits_for_path(&entries, "src/billing", 5);

        assert_eq!(hits.len(), 1);
        assert!(!limit_hit);
        assert_eq!(hits[0]["file_a"], "src/billing/service.rs");
        assert_eq!(hits[0]["file_b"], "src/billing/repository.rs");
        assert_eq!(coupling_status(entries.len(), hits.len()), "matched");
    }

    #[test]
    fn coupling_status_distinguishes_empty_history_from_no_path_match() {
        assert_eq!(coupling_status(0, 0), "empty_git_history");
        assert_eq!(coupling_status(3, 0), "no_path_matches");
        assert_eq!(coupling_status(3, 1), "matched");
    }

    #[test]
    fn scoped_cycle_and_coupling_hits_report_caps_without_substring_leaks() {
        let cycles = vec![
            CircularDependency {
                cycle: vec!["src/pkg/a.rs".to_owned(), "src/pkg/b.rs".to_owned()],
                length: 2,
            },
            CircularDependency {
                cycle: vec!["src/pkg/c.rs".to_owned(), "src/pkg/d.rs".to_owned()],
                length: 2,
            },
            CircularDependency {
                cycle: vec![
                    "src/pkg_extra/not_in_scope.rs".to_owned(),
                    "src/other.rs".to_owned(),
                ],
                length: 2,
            },
        ];
        let (cycle_hits, cycle_limit_hit) = cycle_hits_for_path(&cycles, "src/pkg", 1);
        assert_eq!(cycle_hits.len(), 1);
        assert!(cycle_limit_hit);
        let (all_cycle_hits, all_cycle_limit_hit) = cycle_hits_for_path(&cycles, "src/pkg", 3);
        assert_eq!(all_cycle_hits.len(), 2);
        assert!(!all_cycle_limit_hit);
        assert!(
            all_cycle_hits
                .iter()
                .all(|entry| !entry.to_string().contains("pkg_extra"))
        );

        let couplings = vec![
            CouplingEntry {
                file_a: "src/pkg/a.rs".to_owned(),
                file_b: "src/shared/a.rs".to_owned(),
                co_changes: 4,
                total_changes_a: 5,
                total_changes_b: 6,
                strength: 0.66,
            },
            CouplingEntry {
                file_a: "src/pkg/b.rs".to_owned(),
                file_b: "src/shared/b.rs".to_owned(),
                co_changes: 3,
                total_changes_a: 4,
                total_changes_b: 4,
                strength: 0.75,
            },
            CouplingEntry {
                file_a: "src/pkg_extra/not_in_scope.rs".to_owned(),
                file_b: "src/shared/c.rs".to_owned(),
                co_changes: 3,
                total_changes_a: 4,
                total_changes_b: 4,
                strength: 0.75,
            },
        ];
        let (coupling_hits, coupling_limit_hit) = coupling_hits_for_path(&couplings, "src/pkg", 1);
        assert_eq!(coupling_hits.len(), 1);
        assert!(coupling_limit_hit);
        assert!(
            coupling_hits
                .iter()
                .all(|entry| !entry.to_string().contains("pkg_extra"))
        );
        let (all_coupling_hits, all_coupling_limit_hit) =
            coupling_hits_for_path(&couplings, "src/pkg", 3);
        assert_eq!(all_coupling_hits.len(), 2);
        assert!(!all_coupling_limit_hit);
    }

    #[test]
    fn semantic_coupling_section_keeps_ready_empty_matches_actionable() {
        let section = semantic_coupling_section(
            &json!({
                "status": "ready",
                "indexed_symbols": 42,
                "loaded": false,
            }),
            7,
            Vec::new(),
        );

        assert_eq!(section["status"], "ready_no_external_matches");
        assert_eq!(section["semantic_result_count"], 7);
        assert!(section["matches"].as_array().unwrap().is_empty());
        assert!(
            section["note"]
                .as_str()
                .unwrap()
                .contains("not that semantic analysis is impossible")
        );
    }
}
