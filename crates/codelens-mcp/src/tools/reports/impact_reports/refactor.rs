use crate::AppState;
use crate::tool_runtime::ToolResult;
use crate::tools::report_contract::make_handle_response;
use crate::tools::report_utils::{stable_cache_key, strings_from_array};
use crate::tools::symbols::{semantic_results_for_query, semantic_status};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use super::{insert_semantic_status, module_boundary_report, semantic_degraded_note};

pub fn refactor_safety_report(state: &AppState, arguments: &Value) -> ToolResult {
    let path = arguments
        .get("path")
        .and_then(|value| value.as_str())
        .unwrap_or(".");
    let task = arguments.get("task").and_then(|value| value.as_str());
    let symbol = arguments.get("symbol").and_then(|value| value.as_str());

    let boundary = module_boundary_report(state, &json!({"path": path}))?.0;
    let symbol_impact = if let Some(symbol) = symbol {
        super::super::summarize_symbol_impact(
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
            super::super::analyze_change_request(state, &json!({"task": task}))
                .map(|output| output.0)
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
        crate::util::push_unique_string(
            &mut next_actions,
            "Run index_embeddings before trusting semantic-enriched report sections",
        );
        crate::util::push_unique_string(&mut next_actions, note);
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
        vec![
            arguments
                .get("file_path")
                .and_then(|value| value.as_str())
                .unwrap_or(path)
                .to_owned(),
        ],
        symbol.map(ToOwned::to_owned),
        Some(arguments),
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
            Some(arguments),
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
        Some(arguments),
    )
}
