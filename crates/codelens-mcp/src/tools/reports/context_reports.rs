use crate::AppState;
use crate::tool_runtime::{ToolResult, required_string};
use crate::tools::report_contract::make_handle_response;
use crate::tools::report_utils::{stable_cache_key, strings_from_array};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub fn analyze_change_request(state: &AppState, arguments: &Value) -> ToolResult {
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
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let ranked_files = ranked_symbols
        .iter()
        .filter(|entry| {
            let file = entry
                .get("file")
                .or_else(|| entry.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            // Exclude non-source files from workflow recommendations
            !file.starts_with("benchmarks/")
                && !file.starts_with("scripts/")
                && !file.starts_with("tests/")
                && !file.ends_with("_test.rs")
                && !file.ends_with(".test.ts")
                && !file.ends_with(".test.tsx")
                && !file.ends_with(".spec.ts")
        })
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
        Some(arguments),
    )
}

pub fn find_minimal_context_for_change(state: &AppState, arguments: &Value) -> ToolResult {
    let task = required_string(arguments, "task")?;
    let ranked = crate::tools::symbols::get_ranked_context(
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
        Some(arguments),
    )
}

pub fn summarize_symbol_impact(state: &AppState, arguments: &Value) -> ToolResult {
    let symbol = required_string(arguments, "symbol")?;
    let file_path = arguments.get("file_path").and_then(|v| v.as_str());
    let symbol_lookup = crate::tools::symbols::find_symbol(
        state,
        &json!({"name": symbol, "file_path": file_path, "include_body": false, "exact_match": true, "max_matches": 5}),
    )?
    .0;
    let callers = crate::tools::graph::get_callers_tool(
        state,
        &json!({"function_name": symbol, "max_results": 10}),
    )
    .map(|out| out.0)
    .unwrap_or_else(|_| json!({"callers": []}));
    let callees = crate::tools::graph::get_callees_tool(
        state,
        &json!({"function_name": symbol, "file_path": file_path, "max_results": 10}),
    )
    .map(|out| out.0)
    .unwrap_or_else(|_| json!({"callees": []}));
    let scoped_refs = crate::tools::graph::find_scoped_references_tool(
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
        Some(arguments),
    )
}
