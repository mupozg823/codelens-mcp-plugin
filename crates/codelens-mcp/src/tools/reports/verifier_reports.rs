use crate::tools::report_contract::make_handle_response;
use crate::tools::report_utils::{stable_cache_key, strings_from_array};
use crate::tools::{required_string, AppState, ToolResult};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn verify_change_readiness(state: &AppState, arguments: &Value) -> ToolResult {
    let task = required_string(arguments, "task")?;
    let ranked = crate::tools::symbols::get_ranked_context(
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
        crate::tools::graph::get_changed_files_tool(state, &json!({"include_untracked": true}))
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

pub fn safe_rename_report(state: &AppState, arguments: &Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let symbol = required_string(arguments, "symbol")?;
    let symbol_matches = crate::tools::symbols::find_symbol(
        state,
        &json!({"name": symbol, "file_path": file_path, "include_body": false, "exact_match": true, "max_matches": 5}),
    )?
    .0;
    let references = crate::tools::graph::find_scoped_references_tool(
        state,
        &json!({"symbol_name": symbol, "file_path": file_path, "max_results": 50}),
    )?
    .0;
    let preview = if let Some(new_name) = arguments.get("new_name").and_then(|v| v.as_str()) {
        crate::tools::mutation::rename_symbol(
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
        crate::tools::symbols::find_symbol(
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
        crate::tools::graph::find_scoped_references_tool(
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
