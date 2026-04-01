use super::{required_string, success_meta, AppState, ToolResult};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

fn strings_from_array(value: Option<&Vec<Value>>, field: &str, limit: usize) -> Vec<String> {
    value
        .into_iter()
        .flatten()
        .take(limit)
        .filter_map(|entry| {
            if let Some(text) = entry.as_str() {
                Some(text.to_owned())
            } else if let Some(obj) = entry.as_object() {
                obj.get(field)
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned)
                    .or_else(|| Some(entry.to_string()))
            } else {
                Some(entry.to_string())
            }
        })
        .collect()
}

fn make_handle_response(
    state: &AppState,
    tool_name: &str,
    summary: String,
    top_findings: Vec<String>,
    confidence: f64,
    next_actions: Vec<String>,
    sections: BTreeMap<String, Value>,
) -> ToolResult {
    let artifact = state.store_analysis(
        tool_name,
        summary.clone(),
        top_findings.clone(),
        confidence,
        next_actions.clone(),
        sections,
    )?;
    Ok((
        json!({
            "analysis_id": artifact.id,
            "summary": artifact.summary,
            "top_findings": artifact.top_findings,
            "confidence": artifact.confidence,
            "next_actions": artifact.next_actions,
            "available_sections": artifact.available_sections,
        }),
        success_meta(BackendKind::Hybrid, confidence),
    ))
}

fn extract_handle_fields(payload: &Value) -> (Option<String>, Vec<String>) {
    let analysis_id = payload
        .get("analysis_id")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let estimated_sections = payload
        .get("available_sections")
        .and_then(|value| value.as_array())
        .map(|items| {
            items.iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    (analysis_id, estimated_sections)
}

fn run_job_kind(state: &AppState, kind: &str, arguments: &Value) -> ToolResult {
    match kind {
        "impact_report" => impact_report(state, arguments),
        "dead_code_report" => dead_code_report(state, arguments),
        "refactor_safety_report" => refactor_safety_report(state, arguments),
        _ => Err(CodeLensError::Validation(format!(
            "unsupported analysis job kind `{kind}`"
        ))),
    }
}

fn debug_step_delay_ms(arguments: &Value) -> u64 {
    arguments
        .get("debug_step_delay_ms")
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
        .min(250)
}

fn maybe_delay(ms: u64) {
    if ms > 0 {
        std::thread::sleep(Duration::from_millis(ms));
    }
}

fn estimated_sections_for_kind(kind: &str) -> Vec<String> {
    match kind {
        "impact_report" => vec!["impact_rows".to_owned()],
        "dead_code_report" => vec!["candidates".to_owned(), "raw_dead_code".to_owned()],
        "refactor_safety_report" => vec![
            "module_boundary".to_owned(),
            "symbol_impact".to_owned(),
            "change_request".to_owned(),
            "related_tests".to_owned(),
        ],
        _ => Vec::new(),
    }
}

fn patch_job_file(
    project_path: &str,
    job_id: &str,
    status: Option<&str>,
    progress: Option<u8>,
    current_step: Option<Option<String>>,
    analysis_id: Option<Option<String>>,
    error: Option<Option<String>>,
) {
    let path = Path::new(project_path)
        .join(".codelens")
        .join("analysis-cache")
        .join("jobs")
        .join(format!("{job_id}.json"));
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    let Ok(mut job) = serde_json::from_slice::<crate::state::AnalysisJob>(&bytes) else {
        return;
    };
    if let Some(status) = status {
        job.status = status.to_owned();
    }
    if let Some(progress) = progress {
        job.progress = progress;
    }
    if let Some(current_step) = current_step {
        job.current_step = current_step;
    }
    if let Some(analysis_id) = analysis_id {
        job.analysis_id = analysis_id;
    }
    if let Some(error) = error {
        job.error = error;
    }
    job.updated_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if let Ok(updated) = serde_json::to_vec_pretty(&job) {
        let tmp_path = path.with_extension("json.tmp");
        let _ = std::fs::write(&tmp_path, updated);
        let _ = std::fs::rename(tmp_path, path);
    }
}

fn advance_job_progress(
    state: &AppState,
    job_id: &str,
    progress: u8,
    current_step: &str,
    delay_ms: u64,
) -> Result<bool, String> {
    if state
        .get_analysis_job(job_id)
        .as_ref()
        .map(|job| job.status.as_str())
        == Some("cancelled")
    {
        return Ok(false);
    }
    state
        .update_analysis_job(
            job_id,
            Some("running"),
            Some(progress),
            Some(Some(current_step.to_owned())),
            None,
            None,
            None,
        )
        .map_err(|error| error.to_string())?;
    maybe_delay(delay_ms);
    Ok(true)
}

fn run_job_kind_with_progress(
    state: &AppState,
    job_id: &str,
    kind: &str,
    arguments: &Value,
) -> Result<Value, String> {
    let delay_ms = debug_step_delay_ms(arguments);
    match kind {
        "impact_report" => run_impact_report_job(state, job_id, arguments, delay_ms),
        "dead_code_report" => run_dead_code_report_job(state, job_id, arguments, delay_ms),
        "refactor_safety_report" => {
            run_refactor_safety_report_job(state, job_id, arguments, delay_ms)
        }
        _ => run_job_kind(state, kind, arguments)
            .map(|(payload, _meta)| payload)
            .map_err(|error| error.to_string()),
    }
}

fn run_dead_code_report_job(
    state: &AppState,
    job_id: &str,
    arguments: &Value,
    delay_ms: u64,
) -> Result<Value, String> {
    let scope = arguments
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or(".");
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(20);
    if !advance_job_progress(state, job_id, 20, "scanning dead code candidates", delay_ms)? {
        return Ok(json!({}));
    }
    let dead_code = super::graph::find_dead_code_v2_tool(state, &json!({"max_results": max_results}))
        .map(|output| output.0)
        .map_err(|error| error.to_string())?;
    if !advance_job_progress(state, job_id, 70, "filtering scoped dead code", delay_ms)? {
        return Ok(json!({}));
    }
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
    if !advance_job_progress(state, job_id, 90, "writing dead code analysis", delay_ms)? {
        return Ok(json!({}));
    }
    make_handle_response(
        state,
        "dead_code_report",
        format!("Bounded dead-code audit for scope `{scope}`."),
        top_findings,
        0.84,
        vec!["Validate runtime entry points before deleting candidates".to_owned()],
        sections,
    )
    .map(|(payload, _meta)| payload)
    .map_err(|error| error.to_string())
}

fn run_impact_report_job(
    state: &AppState,
    job_id: &str,
    arguments: &Value,
    delay_ms: u64,
) -> Result<Value, String> {
    if !advance_job_progress(state, job_id, 20, "collecting changed files", delay_ms)? {
        return Ok(json!({}));
    }
    let changed_files = strings_from_array(
        arguments.get("changed_files").and_then(|value| value.as_array()),
        "file",
        8,
    );
    let target_files = if !changed_files.is_empty() {
        changed_files
    } else if let Some(path) = arguments.get("path").and_then(|value| value.as_str()) {
        vec![path.to_owned()]
    } else {
        let changed =
            super::graph::get_changed_files_tool(state, &json!({"include_untracked": true}))
                .map(|out| out.0)
                .unwrap_or_else(|_| json!({"files": [], "count": 0}));
        strings_from_array(changed.get("files").and_then(|value| value.as_array()), "file", 8)
    };
    if !advance_job_progress(state, job_id, 45, "measuring impact surface", delay_ms)? {
        return Ok(json!({}));
    }
    let mut impact_rows = Vec::new();
    let mut top_findings = Vec::new();
    let total = target_files.iter().take(5).count().max(1);
    for (idx, path) in target_files.iter().take(5).enumerate() {
        let impact = super::graph::get_impact_analysis(state, &json!({"file_path": path, "max_depth": 2}))
            .map(|output| output.0)
            .unwrap_or_else(|_| json!({"file_path": path, "total_affected_files": 0, "direct_importers": []}));
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
        let loop_progress = 45 + (((idx + 1) * 35) / total) as u8;
        if !advance_job_progress(
            state,
            job_id,
            loop_progress.min(80),
            &format!("analyzed impact for {path}"),
            delay_ms,
        )? {
            return Ok(json!({}));
        }
    }
    let mut sections = BTreeMap::new();
    sections.insert(
        "impact_rows".to_owned(),
        json!({"files": target_files, "impacts": impact_rows}),
    );
    if !advance_job_progress(state, job_id, 90, "writing impact analysis", delay_ms)? {
        return Ok(json!({}));
    }
    make_handle_response(
        state,
        "impact_report",
        "Diff-aware impact report with bounded blast radius and importer evidence.".to_owned(),
        top_findings,
        0.88,
        vec!["Expand only the highest-impact file before deeper review".to_owned()],
        sections,
    )
    .map(|(payload, _meta)| payload)
    .map_err(|error| error.to_string())
}

fn run_refactor_safety_report_job(
    state: &AppState,
    job_id: &str,
    arguments: &Value,
    delay_ms: u64,
) -> Result<Value, String> {
    let path = arguments
        .get("path")
        .and_then(|value| value.as_str())
        .unwrap_or(".");
    let task = arguments.get("task").and_then(|value| value.as_str());
    let symbol = arguments.get("symbol").and_then(|value| value.as_str());
    if !advance_job_progress(state, job_id, 20, "analyzing module boundaries", delay_ms)? {
        return Ok(json!({}));
    }
    let boundary = module_boundary_report(state, &json!({"path": path}))
        .map(|output| output.0)
        .map_err(|error| error.to_string())?;
    if !advance_job_progress(state, job_id, 40, "summarizing symbol impact", delay_ms)? {
        return Ok(json!({}));
    }
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
    if !advance_job_progress(state, job_id, 60, "ranking refactor context", delay_ms)? {
        return Ok(json!({}));
    }
    let change_request = task
        .map(|task| analyze_change_request(state, &json!({"task": task})).map(|output| output.0))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| json!({"skipped": true, "reason": "no task provided"}));
    if !advance_job_progress(state, job_id, 80, "collecting related tests", delay_ms)? {
        return Ok(json!({}));
    }
    let tests = super::filesystem::find_tests(state, &json!({"path": path, "max_results": 10}))
        .map(|output| output.0)
        .unwrap_or_else(|_| json!({"tests": []}));

    let mut top_findings = Vec::new();
    if let Some(symbol) = symbol {
        top_findings.push(format!("Validate symbol-level callers before refactoring `{symbol}`."));
    }
    if let Some(task) = task {
        top_findings.push(format!("Keep the refactor aligned with `{task}`."));
    }
    top_findings.push(format!("Check tests around `{path}` before applying broad edits."));

    let mut sections = BTreeMap::new();
    sections.insert("module_boundary".to_owned(), boundary);
    sections.insert("symbol_impact".to_owned(), symbol_impact);
    sections.insert("change_request".to_owned(), change_request);
    sections.insert("related_tests".to_owned(), tests);
    if !advance_job_progress(state, job_id, 92, "writing refactor safety analysis", delay_ms)? {
        return Ok(json!({}));
    }
    make_handle_response(
        state,
        "refactor_safety_report",
        format!("Preview-first refactor safety report for `{path}`."),
        top_findings,
        0.9,
        vec!["Use safe_rename_report or focused edits only after checking blockers".to_owned()],
        sections,
    )
    .map(|(payload, _meta)| payload)
    .map_err(|error| error.to_string())
}

pub fn analyze_change_request(state: &AppState, arguments: &Value) -> ToolResult {
    let task = required_string(arguments, "task")?;
    let ranked = super::symbols::get_ranked_context(
        state,
        &json!({"query": task, "max_tokens": 1200, "include_body": false, "depth": 2}),
    )?
    .0;
    let changed = super::graph::get_changed_files_tool(
        state,
        &json!({"include_untracked": true}),
    )
    .map(|out| out.0)
    .unwrap_or_else(|_| json!({"files": [], "count": 0, "note": "git metadata unavailable"}));
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
    let summary = if let Some(profile_hint) = arguments.get("profile_hint").and_then(|v| v.as_str()) {
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
    make_handle_response(state, "analyze_change_request", summary, top_findings, 0.9, next_actions, sections)
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
        format!("Minimal starting context for `{task}` with the smallest useful file/symbol set."),
        top_findings,
        0.89,
        vec!["Open only the listed files first".to_owned()],
        sections,
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
    let callers = super::graph::get_callers_tool(state, &json!({"function_name": symbol, "max_results": 10}))
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

    let top_findings = vec![
        format!(
            "{} caller(s), {} callee(s), {} classified reference(s)",
            callers.get("count").and_then(|v| v.as_u64()).unwrap_or_default(),
            callees.get("count").and_then(|v| v.as_u64()).unwrap_or_default(),
            scoped_refs.get("count").and_then(|v| v.as_u64()).unwrap_or_default()
        ),
    ];
    let mut sections = BTreeMap::new();
    sections.insert("symbol_matches".to_owned(), symbol_lookup);
    sections.insert("callers".to_owned(), callers);
    sections.insert("callees".to_owned(), callees);
    sections.insert("references".to_owned(), scoped_refs);
    make_handle_response(
        state,
        "summarize_symbol_impact",
        format!("Bounded impact summary for symbol `{symbol}`."),
        top_findings,
        0.88,
        vec!["Validate the dominant call sites before refactoring".to_owned()],
        sections,
    )
}

pub fn module_boundary_report(state: &AppState, arguments: &Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    let impact = super::graph::get_impact_analysis(state, &json!({"file_path": path, "max_depth": 2}))
        .map(|out| out.0)
        .unwrap_or_else(|_| json!({"blast_radius": [], "direct_importers": []}));
    let cycles = super::graph::find_circular_dependencies_tool(state, &json!({"max_results": 20}))?.0;
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

    let top_findings = vec![
        format!(
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
        ),
    ];
    let mut sections = BTreeMap::new();
    sections.insert("impact".to_owned(), impact);
    sections.insert("cycle_hits".to_owned(), json!({ "path": path, "cycles": cycle_hits }));
    sections.insert(
        "coupling_hits".to_owned(),
        json!({ "path": path, "couplings": coupling_hits }),
    );
    sections.insert("symbols".to_owned(), symbols);
    make_handle_response(
        state,
        "module_boundary_report",
        format!("Module boundary report for `{path}` with inbound/outbound and structural risk."),
        top_findings,
        0.87,
        vec!["Check cycle hits before moving ownership boundaries".to_owned()],
        sections,
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
    let ref_count = references.get("count").and_then(|v| v.as_u64()).unwrap_or_default();
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
    let mut top_findings = vec![format!("{ref_count} classified reference(s) found for `{symbol}`.")];
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
        format!("Rename safety report for `{symbol}` in `{file_path}`."),
        top_findings,
        0.9,
        vec!["Review the preview before enabling mutation tools".to_owned()],
        sections,
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
    let dead_code = super::graph::find_dead_code_v2_tool(state, &json!({"max_results": max_results}))?.0;
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
        format!("Bounded dead-code audit for scope `{scope}`."),
        top_findings,
        0.84,
        vec!["Validate runtime entry points before deleting candidates".to_owned()],
        sections,
    )
}

pub fn impact_report(state: &AppState, arguments: &Value) -> ToolResult {
    let changed_files = strings_from_array(
        arguments.get("changed_files").and_then(|value| value.as_array()),
        "file",
        8,
    );
    let target_files = if !changed_files.is_empty() {
        changed_files
    } else if let Some(path) = arguments.get("path").and_then(|value| value.as_str()) {
        vec![path.to_owned()]
    } else {
        let changed = super::graph::get_changed_files_tool(state, &json!({"include_untracked": true}))?.0;
        strings_from_array(changed.get("files").and_then(|value| value.as_array()), "file", 8)
    };

    let mut impact_rows = Vec::new();
    let mut top_findings = Vec::new();
    for path in target_files.iter().take(5) {
        let impact = super::graph::get_impact_analysis(state, &json!({"file_path": path, "max_depth": 2}))
            .map(|output| output.0)
            .unwrap_or_else(|_| json!({"file_path": path, "total_affected_files": 0, "direct_importers": []}));
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
        "Diff-aware impact report with bounded blast radius and importer evidence.".to_owned(),
        top_findings,
        0.88,
        vec!["Expand only the highest-impact file before deeper review".to_owned()],
        sections,
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
        top_findings.push(format!("Validate symbol-level callers before refactoring `{symbol}`."));
    }
    if let Some(task) = task {
        top_findings.push(format!("Keep the refactor aligned with `{task}`."));
    }
    top_findings.push(format!("Check tests around `{path}` before applying broad edits."));

    let mut sections = BTreeMap::new();
    sections.insert("module_boundary".to_owned(), boundary);
    sections.insert("symbol_impact".to_owned(), symbol_impact);
    sections.insert("change_request".to_owned(), change_request);
    sections.insert("related_tests".to_owned(), tests);
    make_handle_response(
        state,
        "refactor_safety_report",
        format!("Preview-first refactor safety report for `{path}`."),
        top_findings,
        0.9,
        vec!["Use safe_rename_report or focused edits only after checking blockers".to_owned()],
        sections,
    )
}

pub fn diff_aware_references(state: &AppState, arguments: &Value) -> ToolResult {
    let changed_files = strings_from_array(
        arguments.get("changed_files").and_then(|value| value.as_array()),
        "file",
        8,
    );
    let changed_files = if changed_files.is_empty() {
        let changed = super::graph::get_changed_files_tool(state, &json!({"include_untracked": true}))?.0;
        strings_from_array(changed.get("files").and_then(|value| value.as_array()), "file", 8)
    } else {
        changed_files
    };

    let mut rows = Vec::new();
    let mut top_findings = Vec::new();
    for path in changed_files.iter().take(5) {
        let symbols = super::symbols::get_symbols_overview(state, &json!({"path": path, "depth": 1}))
            .map(|output| output.0)
            .unwrap_or_else(|_| json!({"symbols": []}));
        let symbol_names = symbols
            .get("symbols")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .take(3)
            .filter_map(|entry| entry.get("name").and_then(|value| value.as_str()).map(ToOwned::to_owned))
            .collect::<Vec<_>>();
        let mut reference_hits = Vec::new();
        for symbol_name in &symbol_names {
            let refs = super::graph::find_scoped_references_tool(
                state,
                &json!({"symbol_name": symbol_name, "file_path": path, "max_results": 20}),
            )
            .map(|output| output.0)
            .unwrap_or_else(|_| json!({"references": [], "count": 0}));
            let count = refs.get("count").and_then(|value| value.as_u64()).unwrap_or_default();
            reference_hits.push(json!({"symbol": symbol_name, "count": count, "references": refs.get("references").cloned().unwrap_or(json!([]))}));
            top_findings.push(format!("{path}: `{symbol_name}` has {count} classified reference(s)"));
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
        "Diff-aware reference compression for reviewer and CI flows.".to_owned(),
        top_findings.into_iter().take(5).collect(),
        0.86,
        vec!["Expand only the changed file with the highest reference count".to_owned()],
        sections,
    )
}

pub(crate) fn run_analysis_job_from_queue(
    worker_state: &AppState,
    job_id: String,
    kind: String,
    arguments: Value,
) {
    let project_path = worker_state.project().as_path().to_string_lossy().to_string();
    if worker_state
        .get_analysis_job(&job_id)
        .as_ref()
        .map(|job| job.status.as_str())
        == Some("cancelled")
    {
        worker_state
            .metrics()
            .record_analysis_job_finished("cancelled", 0);
        return;
    }
    patch_job_file(
        &project_path,
        &job_id,
        Some("running"),
        Some(5),
        Some(Some("worker started".to_owned())),
        None,
        None,
    );
    let worker = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
        if worker_state
            .get_analysis_job(&job_id)
            .as_ref()
            .map(|job| job.status.as_str())
            == Some("cancelled")
        {
            return Ok(());
        }
        let result = run_job_kind_with_progress(worker_state, &job_id, &kind, &arguments);
        match result {
            Ok(payload) if payload.is_object() => {
                let (analysis_id, estimated_sections) = extract_handle_fields(&payload);
                let current = worker_state.get_analysis_job(&job_id);
                if current.as_ref().map(|job| job.status.as_str()) == Some("cancelled") {
                    worker_state
                        .metrics()
                        .record_analysis_job_finished("cancelled", 0);
                    return Ok(());
                }
                worker_state
                    .update_analysis_job(
                        &job_id,
                        Some("completed"),
                        Some(100),
                        Some(Some("completed".to_owned())),
                        Some(estimated_sections),
                        Some(analysis_id),
                        Some(None),
                        )
                        .map_err(|error| error.to_string())?;
                    worker_state
                        .metrics()
                        .record_analysis_job_finished("completed", 0);
                }
                Ok(_) => {}
                Err(error) => {
                worker_state
                    .update_analysis_job(
                        &job_id,
                        Some("failed"),
                        Some(100),
                        Some(Some("failed".to_owned())),
                        None,
                        Some(None),
                        Some(Some(error.to_string())),
                        )
                        .map_err(|error| error.to_string())?;
                    worker_state
                        .metrics()
                        .record_analysis_job_finished("failed", 0);
                }
            }
            Ok(())
    }));
    match worker {
        Err(panic) => {
            let message = if let Some(text) = panic.downcast_ref::<&str>() {
                (*text).to_owned()
            } else if let Some(text) = panic.downcast_ref::<String>() {
                text.clone()
            } else {
                "analysis worker panicked".to_owned()
            };
            patch_job_file(
                &project_path,
                &job_id,
                Some("failed"),
                Some(100),
                Some(Some("failed".to_owned())),
                Some(None),
                Some(Some(message)),
            );
            worker_state
                .metrics()
                .record_analysis_job_finished("failed", 0);
        }
        Ok(Err(message)) => {
            patch_job_file(
                &project_path,
                &job_id,
                Some("failed"),
                Some(100),
                Some(Some("failed".to_owned())),
                Some(None),
                Some(Some(message)),
            );
            worker_state
                .metrics()
                .record_analysis_job_finished("failed", 0);
        }
        Ok(Ok(())) => {}
    }
}

pub fn start_analysis_job(state: &AppState, arguments: &Value) -> ToolResult {
    let kind = required_string(arguments, "kind")?.to_owned();
    let profile_hint = arguments
        .get("profile_hint")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let estimated_sections = estimated_sections_for_kind(&kind);
    let job = state.store_analysis_job(
        &kind,
        profile_hint.clone(),
        estimated_sections.clone(),
        "queued",
        0,
        Some("queued".to_owned()),
        None,
        None,
    )?;
    state.enqueue_analysis_job(job.id.clone(), kind, arguments.clone())?;
    Ok((
        json!({
            "job_id": job.id,
            "status": job.status,
            "progress": job.progress,
            "current_step": job.current_step,
            "analysis_id": job.analysis_id,
            "estimated_sections": estimated_sections,
        }),
        success_meta(BackendKind::Hybrid, 0.92),
    ))
}

pub fn get_analysis_job(state: &AppState, arguments: &Value) -> ToolResult {
    let job_id = required_string(arguments, "job_id")?;
    let job = state
        .get_analysis_job(job_id)
        .ok_or_else(|| CodeLensError::NotFound(format!("unknown job_id `{job_id}`")))?;
    Ok((
        json!({
            "job_id": job.id,
            "kind": job.kind,
            "status": job.status,
            "progress": job.progress,
            "current_step": job.current_step,
            "profile_hint": job.profile_hint,
            "estimated_sections": job.estimated_sections,
            "analysis_id": job.analysis_id,
            "error": job.error,
            "updated_at_ms": job.updated_at_ms,
        }),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

pub fn cancel_analysis_job(state: &AppState, arguments: &Value) -> ToolResult {
    let job_id = required_string(arguments, "job_id")?;
    let job = state.cancel_analysis_job(job_id)?;
    Ok((
        json!({
            "job_id": job.id,
            "status": job.status,
            "progress": job.progress,
            "current_step": job.current_step,
            "analysis_id": job.analysis_id,
        }),
        success_meta(BackendKind::Memory, 1.0),
    ))
}

pub fn get_analysis_section(state: &AppState, arguments: &Value) -> ToolResult {
    let analysis_id = required_string(arguments, "analysis_id")?;
    let section = required_string(arguments, "section")?;
    let artifact = state
        .get_analysis(analysis_id)
        .ok_or_else(|| CodeLensError::NotFound(format!("unknown analysis_id `{analysis_id}`")))?;
    let content = state
        .get_analysis_section(analysis_id, section)
        .map_err(|error| match error {
            CodeLensError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                CodeLensError::NotFound(format!("analysis `{analysis_id}` has no section `{section}`"))
            }
            other => other,
        })?;
    Ok((
        json!({
            "analysis_id": analysis_id,
            "section": section,
            "content": content,
            "tool_name": artifact.tool_name,
            "surface": artifact.surface,
        }),
        success_meta(BackendKind::Memory, artifact.confidence),
    ))
}
