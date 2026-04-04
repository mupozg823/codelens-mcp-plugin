use super::report_contract::make_handle_response;
use super::report_utils::{stable_cache_key, strings_from_array};
use super::{AppState, ToolResult, required_string};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub fn analyze_change_request(state: &AppState, arguments: &Value) -> ToolResult {
    let task = required_string(arguments, "task")?;
    let ranked = super::symbols::get_ranked_context(
        state,
        &json!({"query": task, "max_tokens": 1200, "include_body": false, "depth": 2}),
    )?
    .0;
    let requested_changed_files = strings_from_array(
        arguments
            .get("changed_files")
            .and_then(|value| value.as_array()),
        "file",
        8,
    );
    let changed = if requested_changed_files.is_empty() {
        super::graph::get_changed_files_tool(state, &json!({"include_untracked": true}))
            .map(|out| out.0)
            .unwrap_or_else(
                |_| json!({"files": [], "count": 0, "note": "git metadata unavailable"}),
            )
    } else {
        json!({
            "files": requested_changed_files
                .iter()
                .map(|path| json!({"path": path, "status": "provided"}))
                .collect::<Vec<_>>(),
            "count": requested_changed_files.len(),
            "source": "provided",
        })
    };
    let ranked_symbols = ranked
        .get("symbols")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let ranked_files = ranked_symbols
        .iter()
        .take(5)
        .map(|entry| {
            json!({
                "file": entry.get("file").or_else(|| entry.get("file_path")).and_then(|v| v.as_str()).unwrap_or_default(),
                "symbol": entry.get("name").and_then(|v| v.as_str()).unwrap_or_default(),
                "kind": entry.get("kind").and_then(|v| v.as_str()).unwrap_or_default(),
                "score": entry.get("relevance_score").cloned().unwrap_or(json!(0))
            })
        })
        .collect::<Vec<_>>();
    let top_findings = ranked_files
        .iter()
        .take(3)
        .filter_map(|entry| {
            Some(format!(
                "{}: start in {}",
                entry.get("symbol")?.as_str()?,
                entry.get("file")?.as_str()?
            ))
        })
        .collect::<Vec<_>>();
    let mut next_actions = vec!["Expand the ranked files before editing".to_owned()];
    let has_changed_files = changed
        .get("files")
        .and_then(|v| v.as_array())
        .map(|entries| !entries.is_empty())
        .unwrap_or(false);
    if has_changed_files {
        next_actions.push("Compare the request against the current diff".to_owned());
    }
    let summary = if let Some(profile_hint) = arguments.get("profile_hint").and_then(|v| v.as_str())
    {
        format!("Compressed change plan for `{task}` tuned for `{profile_hint}`.")
    } else {
        format!("Compressed change plan for `{task}` with the top starting points and risk cues.")
    };

    let mut sections = BTreeMap::new();
    sections.insert(
        "ranked_files".to_owned(),
        json!({
            "task": task,
            "ranked_files": ranked_files,
        }),
    );
    sections.insert("raw_ranked_context".to_owned(), ranked);
    sections.insert("changed_files".to_owned(), changed);
    let touched_files = strings_from_array(
        sections
            .get("changed_files")
            .and_then(|value| value.get("files"))
            .and_then(|value| value.as_array()),
        "path",
        6,
    );
    make_handle_response(
        state,
        "analyze_change_request",
        stable_cache_key(
            "analyze_change_request",
            arguments,
            &["task", "profile_hint", "changed_files"],
        ),
        summary,
        top_findings,
        0.9,
        next_actions,
        sections,
        touched_files,
        None,
    )
}

pub fn verify_change_readiness(state: &AppState, arguments: &Value) -> ToolResult {
    let task = required_string(arguments, "task")?;
    let ranked = super::symbols::get_ranked_context(
        state,
        &json!({"query": task, "max_tokens": 1200, "include_body": false, "depth": 2}),
    )?
    .0;
    let requested_changed_files = strings_from_array(
        arguments
            .get("changed_files")
            .and_then(|value| value.as_array()),
        "file",
        8,
    );
    let changed = if requested_changed_files.is_empty() {
        super::graph::get_changed_files_tool(state, &json!({"include_untracked": true}))
            .map(|out| out.0)
            .unwrap_or_else(
                |_| json!({"files": [], "count": 0, "note": "git metadata unavailable"}),
            )
    } else {
        json!({
            "files": requested_changed_files
                .iter()
                .map(|path| json!({"path": path, "status": "provided"}))
                .collect::<Vec<_>>(),
            "count": requested_changed_files.len(),
            "source": "provided",
        })
    };
    let ranked_symbols = ranked
        .get("symbols")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let ranked_files = ranked_symbols
        .iter()
        .take(5)
        .map(|entry| {
            json!({
                "file": entry.get("file").or_else(|| entry.get("file_path")).and_then(|v| v.as_str()).unwrap_or_default(),
                "symbol": entry.get("name").and_then(|v| v.as_str()).unwrap_or_default(),
                "kind": entry.get("kind").and_then(|v| v.as_str()).unwrap_or_default(),
                "score": entry.get("relevance_score").cloned().unwrap_or(json!(0))
            })
        })
        .collect::<Vec<_>>();
    let top_findings = ranked_files
        .iter()
        .take(3)
        .filter_map(|entry| {
            Some(format!(
                "{}: verify {} first",
                entry.get("symbol")?.as_str()?,
                entry.get("file")?.as_str()?
            ))
        })
        .collect::<Vec<_>>();
    let mut sections = BTreeMap::new();
    sections.insert(
        "ranked_files".to_owned(),
        json!({
            "task": task,
            "ranked_files": ranked_files,
        }),
    );
    sections.insert("raw_ranked_context".to_owned(), ranked);
    sections.insert("changed_files".to_owned(), changed);
    let touched_files = strings_from_array(
        sections
            .get("changed_files")
            .and_then(|value| value.get("files"))
            .and_then(|value| value.as_array()),
        "path",
        6,
    );
    make_handle_response(
        state,
        "verify_change_readiness",
        stable_cache_key(
            "verify_change_readiness",
            arguments,
            &["task", "profile_hint", "changed_files"],
        ),
        format!("Verifier-first readiness report for `{task}` with blockers and preflight cues."),
        top_findings,
        0.91,
        vec![
            "Review blockers before starting edits".to_owned(),
            "Expand verifier evidence before enabling mutation tools".to_owned(),
        ],
        sections,
        touched_files,
        None,
    )
}

pub fn find_minimal_context_for_change(state: &AppState, arguments: &Value) -> ToolResult {
    let task = required_string(arguments, "task")?;
    let ranked = super::symbols::get_ranked_context(
        state,
        &json!({"query": task, "max_tokens": 900, "include_body": false, "depth": 1}),
    )?
    .0;
    let top = ranked
        .get("symbols")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .take(5)
        .map(|entry| {
            json!({
                "file": entry.get("file").or_else(|| entry.get("file_path")).and_then(|v| v.as_str()).unwrap_or_default(),
                "symbol": entry.get("name").and_then(|v| v.as_str()).unwrap_or_default(),
                "reason": format!(
                    "Matched `{}` with score {}",
                    entry.get("name").and_then(|v| v.as_str()).unwrap_or("symbol"),
                    entry.get("relevance_score").and_then(|v| v.as_i64()).unwrap_or_default()
                )
            })
        })
        .collect::<Vec<_>>();
    let top_findings = top
        .iter()
        .take(3)
        .filter_map(|entry| {
            Some(format!(
                "{} in {}",
                entry.get("symbol")?.as_str()?,
                entry.get("file")?.as_str()?
            ))
        })
        .collect::<Vec<_>>();
    let mut sections = BTreeMap::new();
    sections.insert(
        "minimal_context".to_owned(),
        json!({
            "task": task,
            "top_files": top,
        }),
    );
    sections.insert("raw_ranked_context".to_owned(), ranked);
    make_handle_response(
        state,
        "find_minimal_context_for_change",
        stable_cache_key("find_minimal_context_for_change", arguments, &["task"]),
        format!("Minimal starting context for `{task}` with the smallest useful file/symbol set."),
        top_findings,
        0.89,
        vec!["Open only the listed files first".to_owned()],
        sections,
        top.iter()
            .filter_map(|entry| {
                entry
                    .get("file")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned)
            })
            .collect(),
        None,
    )
}

pub fn summarize_symbol_impact(state: &AppState, arguments: &Value) -> ToolResult {
    let symbol = required_string(arguments, "symbol")?;
    let file_path = arguments.get("file_path").and_then(|v| v.as_str());
    let symbol_lookup = super::symbols::find_symbol(
        state,
        &json!({"name": symbol, "file_path": file_path, "include_body": false, "exact_match": true, "max_matches": 5}),
    )?
    .0;
    let callers =
        super::graph::get_callers_tool(state, &json!({"function_name": symbol, "max_results": 10}))
            .map(|out| out.0)
            .unwrap_or_else(|_| json!({"callers": []}));
    let callees = super::graph::get_callees_tool(
        state,
        &json!({"function_name": symbol, "file_path": file_path, "max_results": 10}),
    )
    .map(|out| out.0)
    .unwrap_or_else(|_| json!({"callees": []}));
    let scoped_refs = super::graph::find_scoped_references_tool(
        state,
        &json!({"symbol_name": symbol, "file_path": file_path, "max_results": 20}),
    )?
    .0;

    let top_findings = vec![format!(
        "{} caller(s), {} callee(s), {} classified reference(s)",
        callers
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or_default(),
        callees
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or_default(),
        scoped_refs
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or_default()
    )];
    let mut sections = BTreeMap::new();
    sections.insert("symbol_matches".to_owned(), symbol_lookup);
    sections.insert("callers".to_owned(), callers);
    sections.insert("callees".to_owned(), callees);
    sections.insert("references".to_owned(), scoped_refs);
    make_handle_response(
        state,
        "summarize_symbol_impact",
        stable_cache_key(
            "summarize_symbol_impact",
            arguments,
            &["symbol", "file_path", "depth"],
        ),
        format!("Bounded impact summary for symbol `{symbol}`."),
        top_findings,
        0.88,
        vec!["Validate the dominant call sites before refactoring".to_owned()],
        sections,
        file_path
            .map(|value| vec![value.to_owned()])
            .unwrap_or_default(),
        Some(symbol.to_owned()),
    )
}

pub fn module_boundary_report(state: &AppState, arguments: &Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    let impact =
        super::graph::get_impact_analysis(state, &json!({"file_path": path, "max_depth": 2}))
            .map(|out| out.0)
            .unwrap_or_else(|_| json!({"blast_radius": [], "direct_importers": []}));
    let cycles =
        super::graph::find_circular_dependencies_tool(state, &json!({"max_results": 20}))?.0;
    let coupling = super::graph::get_change_coupling_tool(state, &json!({"max_results": 20}))?.0;
    let symbols = super::symbols::get_symbols_overview(state, &json!({"path": path, "depth": 1}))
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
    make_handle_response(
        state,
        "module_boundary_report",
        stable_cache_key("module_boundary_report", arguments, &["path"]),
        format!("Module boundary report for `{path}` with inbound/outbound and structural risk."),
        top_findings,
        0.87,
        vec!["Check cycle hits before moving ownership boundaries".to_owned()],
        sections,
        vec![path.to_owned()],
        None,
    )
}

pub fn safe_rename_report(state: &AppState, arguments: &Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let symbol = required_string(arguments, "symbol")?;
    let symbol_matches = super::symbols::find_symbol(
        state,
        &json!({"name": symbol, "file_path": file_path, "include_body": false, "exact_match": true, "max_matches": 5}),
    )?
    .0;
    let references = super::graph::find_scoped_references_tool(
        state,
        &json!({"symbol_name": symbol, "file_path": file_path, "max_results": 50}),
    )?
    .0;
    let preview = if let Some(new_name) = arguments.get("new_name").and_then(|v| v.as_str()) {
        super::mutation::rename_symbol(
            state,
            &json!({"file_path": file_path, "symbol_name": symbol, "new_name": new_name, "dry_run": true}),
        )
        .map(|out| out.0)
        .unwrap_or_else(|error| json!({"preview_error": error.to_string()}))
    } else {
        json!({"preview_skipped": true, "reason": "Provide new_name to generate a dry-run preview."})
    };
    let ref_count = references
        .get("count")
        .and_then(|v| v.as_u64())
        .unwrap_or_default();
    let blockers = if symbol_matches
        .get("count")
        .and_then(|v| v.as_u64())
        .unwrap_or_default()
        == 0
    {
        vec!["No exact symbol match found in the requested file.".to_owned()]
    } else {
        Vec::new()
    };
    let mut top_findings = vec![format!(
        "{ref_count} classified reference(s) found for `{symbol}`."
    )];
    if !blockers.is_empty() {
        top_findings.extend(blockers.clone());
    }
    let mut sections = BTreeMap::new();
    sections.insert("symbol_matches".to_owned(), symbol_matches);
    sections.insert("references".to_owned(), references);
    sections.insert("rename_preview".to_owned(), preview);
    make_handle_response(
        state,
        "safe_rename_report",
        stable_cache_key(
            "safe_rename_report",
            arguments,
            &["file_path", "symbol", "new_name"],
        ),
        format!("Rename safety report for `{symbol}` in `{file_path}`."),
        top_findings,
        0.9,
        vec!["Review the preview before enabling mutation tools".to_owned()],
        sections,
        vec![file_path.to_owned()],
        Some(symbol.to_owned()),
    )
}

pub fn unresolved_reference_check(state: &AppState, arguments: &Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let symbol = arguments.get("symbol").and_then(|value| value.as_str());
    let changed_files = strings_from_array(
        arguments
            .get("changed_files")
            .and_then(|value| value.as_array()),
        "file",
        8,
    );
    let symbol_matches = if let Some(symbol) = symbol {
        super::symbols::find_symbol(
            state,
            &json!({
                "name": symbol,
                "file_path": file_path,
                "include_body": false,
                "exact_match": true,
                "max_matches": 5
            }),
        )?
        .0
    } else {
        json!({
            "symbols": [],
            "count": 0,
            "note": "Provide symbol to run an exact unresolved-reference check."
        })
    };
    let references = if let Some(symbol) = symbol {
        super::graph::find_scoped_references_tool(
            state,
            &json!({"symbol_name": symbol, "file_path": file_path, "max_results": 50}),
        )?
        .0
    } else {
        json!({
            "references": [],
            "count": 0,
            "note": "Provide symbol to classify references."
        })
    };
    let mut sections = BTreeMap::new();
    sections.insert("symbol_matches".to_owned(), symbol_matches);
    sections.insert("references".to_owned(), references);
    if !changed_files.is_empty() {
        sections.insert(
            "changed_files".to_owned(),
            json!({
                "files": changed_files
                    .iter()
                    .map(|path| json!({"path": path, "status": "provided"}))
                    .collect::<Vec<_>>(),
                "count": changed_files.len(),
                "source": "provided",
            }),
        );
    }
    let mut top_findings = if let Some(symbol) = symbol {
        vec![format!(
            "Reference guard prepared for `{symbol}` in `{file_path}`."
        )]
    } else {
        vec![format!(
            "Symbol hint missing for `{file_path}`; unresolved-reference verdict will stay conservative."
        )]
    };
    if !changed_files.is_empty() {
        top_findings.push(format!(
            "{} changed file(s) supplied for context.",
            changed_files.len()
        ));
    }
    let mut touched_files = vec![file_path.to_owned()];
    for path in changed_files {
        if !touched_files.iter().any(|existing| existing == &path) {
            touched_files.push(path);
        }
    }
    make_handle_response(
        state,
        "unresolved_reference_check",
        stable_cache_key(
            "unresolved_reference_check",
            arguments,
            &["file_path", "symbol", "changed_files"],
        ),
        if let Some(symbol) = symbol {
            format!("Unresolved-reference check for `{symbol}` in `{file_path}`.")
        } else {
            format!(
                "Unresolved-reference check for `{file_path}` with conservative file-level guards."
            )
        },
        top_findings,
        0.87,
        vec!["Expand verifier_references before a rename or broad edit".to_owned()],
        sections,
        touched_files,
        symbol.map(ToOwned::to_owned),
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
        super::graph::find_dead_code_v2_tool(state, &json!({"max_results": max_results}))?.0;
    let candidates = dead_code
        .get("dead_code")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.to_string().contains(scope))
        .take(10)
        .collect::<Vec<_>>();
    let top_findings = strings_from_array(Some(&candidates), "file", 3);
    let mut sections = BTreeMap::new();
    sections.insert(
        "candidates".to_owned(),
        json!({"scope": scope, "dead_code": candidates}),
    );
    sections.insert("raw_dead_code".to_owned(), dead_code);
    make_handle_response(
        state,
        "dead_code_report",
        stable_cache_key("dead_code_report", arguments, &["scope", "max_results"]),
        format!("Bounded dead-code audit for scope `{scope}`."),
        top_findings,
        0.84,
        vec!["Validate runtime entry points before deleting candidates".to_owned()],
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
        let changed =
            super::graph::get_changed_files_tool(state, &json!({"include_untracked": true}))?.0;
        strings_from_array(
            changed.get("files").and_then(|value| value.as_array()),
            "file",
            8,
        )
    };

    let mut impact_rows = Vec::new();
    let mut top_findings = Vec::new();
    for path in target_files.iter().take(5) {
        let impact = super::graph::get_impact_analysis(
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

    let mut sections = BTreeMap::new();
    sections.insert(
        "impact_rows".to_owned(),
        json!({"files": target_files, "impacts": impact_rows}),
    );
    make_handle_response(
        state,
        "impact_report",
        stable_cache_key("impact_report", arguments, &["path", "changed_files"]),
        "Diff-aware impact report with bounded blast radius and importer evidence.".to_owned(),
        top_findings,
        0.88,
        vec!["Expand only the highest-impact file before deeper review".to_owned()],
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
        summarize_symbol_impact(
            state,
            &json!({"symbol": symbol, "file_path": arguments.get("file_path").and_then(|v| v.as_str())}),
        )
        .map(|output| output.0)
        .unwrap_or_else(|error| json!({"symbol": symbol, "error": error.to_string()}))
    } else {
        json!({"skipped": true, "reason": "no symbol provided"})
    };
    let change_request = task
        .map(|task| analyze_change_request(state, &json!({"task": task})).map(|output| output.0))
        .transpose()?
        .unwrap_or_else(|| json!({"skipped": true, "reason": "no task provided"}));
    let tests = super::filesystem::find_tests(state, &json!({"path": path, "max_results": 10}))
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
        vec!["Use safe_rename_report or focused edits only after checking blockers".to_owned()],
        sections,
        vec![
            arguments
                .get("file_path")
                .and_then(|value| value.as_str())
                .unwrap_or(path)
                .to_owned(),
        ],
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
        let changed =
            super::graph::get_changed_files_tool(state, &json!({"include_untracked": true}))?.0;
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
            super::symbols::get_symbols_overview(state, &json!({"path": path, "depth": 1}))
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
            let refs = super::graph::find_scoped_references_tool(
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
