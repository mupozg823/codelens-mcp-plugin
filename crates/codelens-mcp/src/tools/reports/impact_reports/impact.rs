use crate::AppState;
use crate::tool_runtime::ToolResult;
use crate::tools::report_contract::make_handle_response;
use crate::tools::report_utils::{stable_cache_key, strings_from_array};
use crate::tools::symbols::{semantic_results_for_query, semantic_status};
use codelens_engine::search::SEMANTIC_COUPLING_THRESHOLD;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{insert_semantic_status, semantic_degraded_note};

#[allow(deprecated)]
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
    let change_kinds: std::collections::HashMap<String, String> = target_files
        .iter()
        .take(5)
        .map(|p| {
            (
                p.clone(),
                codelens_engine::git::classify_change_kind(&project, p),
            )
        })
        .collect();

    use rayon::prelude::*;

    let impact_results: Vec<_> = target_files
        .iter()
        .take(5)
        .cloned()
        .collect::<Vec<_>>()
        .into_par_iter()
        .map(|path| {
            let impact = crate::tools::graph::get_impact_analysis(
                state,
                &json!({"file_path": &path, "max_depth": 2}),
            )
            .map(|output| output.0)
            .unwrap_or_else(
                |_| json!({"file_path": &path, "total_affected_files": 0, "direct_importers": []}),
            );
            let affected = impact
                .get("total_affected_files")
                .and_then(|value| value.as_u64())
                .unwrap_or_default();
            let change_kind = change_kinds
                .get(&path)
                .cloned()
                .unwrap_or_else(|| "mixed".to_owned());
            let kind_label = if change_kind == "additive" {
                " (additive)"
            } else {
                ""
            };
            let finding = format!("{path}: {affected} affected file(s){kind_label}");
            let row = json!({
                "path": path,
                "affected_files": affected,
                "change_kind": change_kind,
                "direct_importers": impact.get("direct_importers").cloned().unwrap_or(json!([])),
                "blast_radius": impact.get("blast_radius").cloned().unwrap_or(json!([])),
            });
            (finding, row)
        })
        .collect();

    let mut top_findings = Vec::new();
    let mut impact_rows = Vec::new();
    for (finding, row) in impact_results {
        top_findings.push(finding);
        impact_rows.push(row);
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
        crate::util::push_unique_string(
            &mut next_actions,
            "Run index_embeddings before trusting semantic-only related-file hints",
        );
        crate::util::push_unique_string(&mut next_actions, note);
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
        Some(arguments),
    )
}

// Historical layout: helper production functions follow the test module.
// Reordering would churn ~330 lines of unrelated code; keep the allow local.
#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::super::{
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
        assert!(out.contains("target0[\"foo.rs\"]:::target"));
        assert!(out.contains("classDef target"));
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

        assert!(out.contains("subgraph src"));
        assert!(out.contains("a.rs"));
        assert!(out.contains("b.rs"));
        assert!(out.contains("c.rs"));
        assert!(out.contains(":::upstream"));
        assert!(out.contains(":::downstream"));
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
        // Truncation note for remaining 15 nodes.
        assert!(out.contains("up_more[\"... +15 more\"]:::note"));
        // 5 regular edges + 1 truncation edge = 6 edges to target0.
        let edges = out.matches("--> target0").count();
        assert_eq!(edges, 6);
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

fn changed_file_path(state: &AppState, file: &str) -> PathBuf {
    let path = Path::new(file);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        state.project().as_path().join(path)
    }
}

fn changed_file_fingerprint(state: &AppState, file: &str) -> Value {
    let path = changed_file_path(state, file);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let digest = hasher.finalize();
            let mut hex = String::with_capacity(64);
            for byte in digest {
                use std::fmt::Write as _;
                let _ = write!(hex, "{byte:02x}");
            }
            json!({
                "file": file,
                "sha256": hex,
                "len": bytes.len(),
            })
        }
        Err(error) => json!({
            "file": file,
            "missing": true,
            "error_kind": format!("{:?}", error.kind()),
        }),
    }
}

fn diff_aware_references_cache_key(state: &AppState, changed_files: &[String]) -> Option<String> {
    stable_cache_key(
        "diff_aware_references",
        &json!({
            "changed_files": changed_files,
            "fingerprints": changed_files
                .iter()
                .take(8)
                .map(|file| changed_file_fingerprint(state, file))
                .collect::<Vec<_>>(),
        }),
        &["changed_files", "fingerprints"],
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
        diff_aware_references_cache_key(state, &changed_files),
        "Diff-aware reference compression for reviewer and CI flows.".to_owned(),
        top_findings.into_iter().take(5).collect(),
        0.86,
        vec!["Expand only the changed file with the highest reference count".to_owned()],
        sections,
        changed_files,
        None,
        Some(arguments),
    )
}
