use super::super::AppState;
use super::super::report_utils::{extract_handle_fields, stable_cache_key, strings_from_array};
use super::helpers::{advance_job_progress, debug_step_delay_ms, patch_job_file, run_job_kind};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn run_job_kind_with_progress(
    state: &AppState,
    scope: &str,
    job_id: &str,
    kind: &str,
    arguments: &Value,
) -> Result<Value, String> {
    let delay_ms = debug_step_delay_ms(arguments);
    match kind {
        "impact_report" => run_impact_report_job(state, scope, job_id, arguments, delay_ms),
        "dead_code_report" => run_dead_code_report_job(state, scope, job_id, arguments, delay_ms),
        "refactor_safety_report" => {
            run_refactor_safety_report_job(state, scope, job_id, arguments, delay_ms)
        }
        "module_boundary_report" => {
            run_simple_report_job(state, scope, job_id, kind, arguments, delay_ms)
        }
        "safe_rename_report" => {
            run_simple_report_job(state, scope, job_id, kind, arguments, delay_ms)
        }
        "diff_aware_references" => {
            run_simple_report_job(state, scope, job_id, kind, arguments, delay_ms)
        }
        "semantic_code_review" => {
            run_simple_report_job(state, scope, job_id, kind, arguments, delay_ms)
        }
        "orchestrate_change" => {
            run_simple_report_job(state, scope, job_id, kind, arguments, delay_ms)
        }
        "analyze_change_request" => {
            run_simple_report_job(state, scope, job_id, kind, arguments, delay_ms)
        }
        "verify_change_readiness" => {
            run_simple_report_job(state, scope, job_id, kind, arguments, delay_ms)
        }
        _ => run_job_kind(state, kind, arguments)
            .map(|(payload, _meta)| payload)
            .map_err(|error| error.to_string()),
    }
}

/// Generic progress wrapper for report kinds that don't need step-level progress tracking.
fn run_simple_report_job(
    state: &AppState,
    scope: &str,
    job_id: &str,
    kind: &str,
    arguments: &Value,
    delay_ms: u64,
) -> Result<Value, String> {
    if !advance_job_progress(
        state,
        scope,
        job_id,
        30,
        &format!("starting {kind}"),
        delay_ms,
    )? {
        return Ok(json!({}));
    }
    let result = run_job_kind(state, kind, arguments)
        .map(|(payload, _meta)| payload)
        .map_err(|error| error.to_string())?;
    if !advance_job_progress(
        state,
        scope,
        job_id,
        90,
        &format!("finalizing {kind}"),
        delay_ms,
    )? {
        return Ok(json!({}));
    }
    Ok(result)
}

#[allow(deprecated)]
fn run_dead_code_report_job(
    state: &AppState,
    scope_key: &str,
    job_id: &str,
    arguments: &Value,
    delay_ms: u64,
) -> Result<Value, String> {
    // Accept `path` as a soft alias of `scope` so async-job callers stay
    // consistent with the rest of the composite-report family (issue G1,
    // 2026-05-18 self-dogfood). Mirrors the sync handler in
    // `tools/reports/impact_reports/boundary.rs`.
    let scope = arguments
        .get("scope")
        .or_else(|| arguments.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or(".");
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(20);
    if !advance_job_progress(
        state,
        scope_key,
        job_id,
        20,
        "scanning dead code candidates",
        delay_ms,
    )? {
        return Ok(json!({}));
    }
    let dead_code =
        super::super::graph::find_dead_code_v2_tool(state, &json!({"max_results": max_results}))
            .map(|output| output.0)
            .map_err(|error| error.to_string())?;
    if !advance_job_progress(
        state,
        scope_key,
        job_id,
        70,
        "filtering scoped dead code",
        delay_ms,
    )? {
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
    if !advance_job_progress(
        state,
        scope_key,
        job_id,
        90,
        "writing dead code analysis",
        delay_ms,
    )? {
        return Ok(json!({}));
    }
    super::super::report_contract::make_handle_response(
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
        vec!["Validate runtime entry points before deleting candidates".to_owned()],
        sections,
        if scope == "." {
            Vec::new()
        } else {
            vec![scope.to_owned()]
        },
        None,
        Some(arguments),
    )
    .map(|(payload, _meta)| payload)
    .map_err(|error| error.to_string())
}

#[allow(deprecated)]
fn run_impact_report_job(
    state: &AppState,
    scope: &str,
    job_id: &str,
    arguments: &Value,
    delay_ms: u64,
) -> Result<Value, String> {
    if !advance_job_progress(
        state,
        scope,
        job_id,
        20,
        "collecting changed files",
        delay_ms,
    )? {
        return Ok(json!({}));
    }
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
            super::super::graph::get_changed_files_tool(state, &json!({"include_untracked": true}))
                .map(|out| out.0)
                .unwrap_or_else(|_| json!({"files": [], "count": 0}));
        strings_from_array(
            changed.get("files").and_then(|value| value.as_array()),
            "file",
            8,
        )
    };
    if !advance_job_progress(
        state,
        scope,
        job_id,
        45,
        "measuring impact surface",
        delay_ms,
    )? {
        return Ok(json!({}));
    }
    let mut impact_rows = Vec::new();
    let mut top_findings = Vec::new();
    let total = target_files.iter().take(5).count().max(1);
    for (idx, path) in target_files.iter().take(5).enumerate() {
        let impact = super::super::graph::get_impact_analysis(
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
        let change_kind = codelens_engine::git::classify_change_kind(&state.project(), path);
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
        let loop_progress = 45 + (((idx + 1) * 35) / total) as u8;
        if !advance_job_progress(
            state,
            scope,
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
    if !advance_job_progress(
        state,
        scope,
        job_id,
        90,
        "writing impact analysis",
        delay_ms,
    )? {
        return Ok(json!({}));
    }
    super::super::report_contract::make_handle_response(
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
        Some(arguments),
    )
    .map(|(payload, _meta)| payload)
    .map_err(|error| error.to_string())
}

fn run_refactor_safety_report_job(
    state: &AppState,
    scope: &str,
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
    if !advance_job_progress(
        state,
        scope,
        job_id,
        20,
        "analyzing module boundaries",
        delay_ms,
    )? {
        return Ok(json!({}));
    }
    let boundary = super::super::reports::module_boundary_report(state, &json!({"path": path}))
        .map(|output| output.0)
        .map_err(|error| error.to_string())?;
    if !advance_job_progress(
        state,
        scope,
        job_id,
        40,
        "summarizing symbol impact",
        delay_ms,
    )? {
        return Ok(json!({}));
    }
    let symbol_impact = if let Some(symbol) = symbol {
        super::super::reports::symbol_impact_summary(
            state,
            &json!({"symbol": symbol, "file_path": arguments.get("file_path").and_then(|v| v.as_str())}),
        )
        .map(|output| output.0)
        .unwrap_or_else(|error| json!({"symbol": symbol, "error": error.to_string()}))
    } else {
        json!({"skipped": true, "reason": "no symbol provided"})
    };
    if !advance_job_progress(
        state,
        scope,
        job_id,
        60,
        "ranking refactor context",
        delay_ms,
    )? {
        return Ok(json!({}));
    }
    let change_request = task
        .map(|task| {
            super::super::reports::analyze_change_request(state, &json!({"task": task}))
                .map(|output| output.0)
        })
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| json!({"skipped": true, "reason": "no task provided"}));
    if !advance_job_progress(
        state,
        scope,
        job_id,
        80,
        "collecting related tests",
        delay_ms,
    )? {
        return Ok(json!({}));
    }
    let tests =
        super::super::filesystem::find_tests(state, &json!({"path": path, "max_results": 10}))
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
    super::super::report_contract::make_handle_response(
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
        Some(arguments),
    )
    .map(|(payload, _meta)| payload)
    .map_err(|error| error.to_string())
}

pub(crate) fn run_analysis_job_from_queue(
    worker_state: &AppState,
    job_id: String,
    kind: String,
    arguments: Value,
) -> crate::runtime_types::JobLifecycle {
    use crate::runtime_types::JobLifecycle;
    let scope = worker_state.project_scope_for_arguments(&arguments);
    if worker_state
        .get_analysis_job_for_scope(&scope, &job_id)
        .as_ref()
        .map(|job| job.status)
        == Some(crate::runtime_types::JobLifecycle::Cancelled)
    {
        return JobLifecycle::Cancelled;
    }
    // #357: bind this worker THREAD to the job's project scope for the
    // duration of the job instead of mutating the worker state's project
    // override (which cleared its artifact/job stores on every switch).
    let _project_guard = match worker_state.bind_request_project_scope(&scope) {
        Ok(guard) => guard,
        Err(error) => {
            patch_job_file(
                worker_state,
                &scope,
                &job_id,
                Some(JobLifecycle::Error),
                Some(100),
                Some(Some("failed".to_owned())),
                Some(None),
                Some(Some(format!(
                    "analysis worker failed to bind project scope `{scope}`: {error}"
                ))),
            );
            return JobLifecycle::Error;
        }
    };
    patch_job_file(
        worker_state,
        &scope,
        &job_id,
        Some(crate::runtime_types::JobLifecycle::Running),
        Some(5),
        Some(Some("worker started".to_owned())),
        None,
        None,
    );
    let worker = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> Result<JobLifecycle, String> {
            if worker_state
                .get_analysis_job_for_scope(&scope, &job_id)
                .as_ref()
                .map(|job| job.status)
                == Some(JobLifecycle::Cancelled)
            {
                return Ok(JobLifecycle::Cancelled);
            }
            // Retry transient failures up to 2 times with backoff
            let mut last_err = None;
            let mut result = None;
            for attempt in 0..3 {
                match run_job_kind_with_progress(worker_state, &scope, &job_id, &kind, &arguments) {
                    Ok(payload) => {
                        result = Some(payload);
                        break;
                    }
                    Err(e) if attempt < 2 => {
                        last_err = Some(e);
                        std::thread::sleep(std::time::Duration::from_millis(
                            100 * (attempt as u64 + 1),
                        ));
                        continue;
                    }
                    Err(e) => {
                        last_err = Some(e);
                        break;
                    }
                }
            }
            match result.map(Ok).unwrap_or_else(|| Err(last_err.unwrap())) {
                Ok(payload) if payload.is_object() => {
                    let (analysis_id, estimated_sections) = extract_handle_fields(&payload);
                    let current = worker_state.get_analysis_job_for_scope(&scope, &job_id);
                    if current.as_ref().map(|job| job.status) == Some(JobLifecycle::Cancelled) {
                        return Ok(JobLifecycle::Cancelled);
                    }
                    worker_state
                        .update_analysis_job(
                            &scope,
                            &job_id,
                            Some(JobLifecycle::Completed),
                            Some(100),
                            Some(Some("completed".to_owned())),
                            Some(estimated_sections),
                            Some(analysis_id),
                            Some(None),
                        )
                        .map_err(|error| error.to_string())?;
                    Ok(JobLifecycle::Completed)
                }
                Ok(_) => Ok(JobLifecycle::Error),
                Err(error) => {
                    worker_state
                        .update_analysis_job(
                            &scope,
                            &job_id,
                            Some(JobLifecycle::Error),
                            Some(100),
                            Some(Some("failed".to_owned())),
                            None,
                            Some(None),
                            Some(Some(error.to_string())),
                        )
                        .map_err(|error| error.to_string())?;
                    Ok(JobLifecycle::Error)
                }
            }
        },
    ));
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
                worker_state,
                &scope,
                &job_id,
                Some(JobLifecycle::Error),
                Some(100),
                Some(Some("failed".to_owned())),
                Some(None),
                Some(Some(message)),
            );
            JobLifecycle::Error
        }
        Ok(Err(message)) => {
            patch_job_file(
                worker_state,
                &scope,
                &job_id,
                Some(JobLifecycle::Error),
                Some(100),
                Some(Some("failed".to_owned())),
                Some(None),
                Some(Some(message)),
            );
            JobLifecycle::Error
        }
        Ok(Ok(status)) => status,
    }
}
