use super::{
    AppState, ToolResult, optional_bool, optional_string, optional_usize, required_string,
    success_meta,
};
use crate::protocol::{BackendKind, ToolResponseMeta};
use crate::tools::symbols::flatten_symbols;
use codelens_engine::{
    find_circular_dependencies, find_dead_code_v2, find_scoped_references, get_blast_radius,
    get_callees, get_callers, get_change_coupling, get_changed_files, get_importance,
    get_importers, phantom_modules, redundant_definitions,
};
use serde_json::{Map, Value, json};

const CALL_GRAPH_RESOLUTIONS: [&str; 7] = [
    "scip",
    "same_file",
    "import_map",
    "import_suffix",
    "unique_name",
    "path_proximity",
    "unresolved",
];

fn resolution_score(strategy: &str) -> f64 {
    match strategy {
        "scip" => 0.98,
        "same_file" => 0.90,
        "import_map" => 0.95,
        "import_suffix" => 0.70,
        "unique_name" => 0.65,
        "path_proximity" => 0.50,
        "unresolved" => 0.25,
        _ => 0.25,
    }
}

fn call_graph_analysis<'a>(
    resolutions: impl Iterator<Item = Option<&'a str>>,
) -> (Map<String, Value>, &'static str, ToolResponseMeta) {
    let resolution_list: Vec<&str> = resolutions
        .map(|value| value.unwrap_or("unresolved"))
        .collect();
    let mut summary = Map::new();
    for key in CALL_GRAPH_RESOLUTIONS {
        summary.insert(key.to_owned(), json!(0));
    }
    for resolution in &resolution_list {
        if let Some(value) = summary.get_mut(*resolution) {
            let count = value.as_u64().unwrap_or_default() + 1;
            *value = json!(count);
        }
    }

    let same_file = summary["same_file"].as_u64().unwrap_or_default();
    let import_map = summary["import_map"].as_u64().unwrap_or_default();
    let import_suffix = summary["import_suffix"].as_u64().unwrap_or_default();
    let unique_name = summary["unique_name"].as_u64().unwrap_or_default();
    let path_proximity = summary["path_proximity"].as_u64().unwrap_or_default();
    let unresolved = summary["unresolved"].as_u64().unwrap_or_default();
    let total = resolution_list.len() as u64;
    let import_evidence = import_map + import_suffix;
    let fallback = path_proximity + unresolved;

    let confidence_basis = if total == 0 || unresolved == total {
        "unresolved_only"
    } else if same_file == total {
        "same_file_only"
    } else if unique_name == total {
        "name_only_unique"
    } else if fallback == total {
        "fallback_only"
    } else if fallback > 0 {
        "mixed_with_fallback"
    } else if import_evidence > 0 {
        "import_evidence"
    } else if same_file > 0 {
        "same_file_only"
    } else {
        "name_only_unique"
    };

    let base_confidence = if resolution_list.is_empty() {
        0.35
    } else {
        let scores: Vec<f64> = resolution_list
            .iter()
            .take(5)
            .map(|resolution| resolution_score(resolution))
            .collect();
        scores.iter().sum::<f64>() / scores.len() as f64
    };
    let mut confidence = base_confidence;
    if path_proximity > 0 {
        confidence = confidence.min(0.60);
    }
    if unresolved > 0 {
        confidence = confidence.min(0.35);
    }

    let backend = if import_evidence > 0 {
        BackendKind::Hybrid
    } else {
        BackendKind::TreeSitter
    };
    let mut meta = success_meta(backend, confidence);
    meta.degraded_reason = if unresolved > 0 {
        Some("unresolved-call-graph-edges".to_owned())
    } else if path_proximity > 0 {
        Some("fallback-dominated-call-graph".to_owned())
    } else {
        None
    };

    (summary, confidence_basis, meta)
}

fn call_graph_evidence_signals(summary: &Map<String, Value>) -> Value {
    let import_evidence = summary["import_map"].as_u64().unwrap_or_default()
        + summary["import_suffix"].as_u64().unwrap_or_default();
    let fallback_evidence = summary["same_file"].as_u64().unwrap_or_default()
        + summary["unique_name"].as_u64().unwrap_or_default()
        + summary["path_proximity"].as_u64().unwrap_or_default()
        + summary["unresolved"].as_u64().unwrap_or_default();
    json!({
        "resolution_summary": summary,
        "precise_available": import_evidence > 0,
        "precise_used": import_evidence > 0,
        "precise_source": if import_evidence > 0 { Some("import_graph") } else { None },
        "fallback_source": if fallback_evidence > 0 { Some("tree_sitter_name_resolution") } else { None },
        "precise_result_count": import_evidence,
    })
}

pub fn get_changed_files_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let git_ref = optional_string(arguments, "ref");
    let include_untracked = optional_bool(arguments, "include_untracked", true);
    let changed = get_changed_files(&state.project(), git_ref, include_untracked)?;
    let ref_label = git_ref.unwrap_or("HEAD");
    Ok((
        json!({ "ref": ref_label, "files": changed, "count": changed.len() }),
        success_meta(BackendKind::Git, 0.95),
    ))
}

/// Internal helper — historically the `get_impact_analysis` MCP tool. The
/// public tool was removed when superseded by `impact_report`, but several
/// internal builders (boundary/impact reports, mermaid graph, report_jobs)
/// still consume this exact JSON shape, so the implementation lives on as
/// a `pub(crate)` helper.
pub(crate) fn get_impact_analysis(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let max_depth = optional_usize(arguments, "max_depth", 3);

    let blast = get_blast_radius(&state.project(), file_path, max_depth, &state.graph_cache())
        .unwrap_or_default();
    let symbols = state
        .symbol_index()
        .get_symbols_overview_cached(file_path, 1)
        .unwrap_or_default();
    let symbol_names: Vec<_> = flatten_symbols(&symbols)
        .iter()
        .map(|s| json!({"name": s.name, "kind": s.kind.as_label(), "line": s.line}))
        .collect();
    let importers =
        get_importers(&state.project(), file_path, 20, &state.graph_cache()).unwrap_or_default();
    let affected: Vec<_> = blast
        .iter()
        .map(|b| {
            let sym_count = state
                .symbol_index()
                .get_symbols_overview_cached(&b.file, 1)
                .map(|s| s.len())
                .unwrap_or(0);
            json!({"file": b.file, "depth": b.depth, "symbol_count": sym_count})
        })
        .collect();

    Ok((
        json!({
            "file": file_path,
            "symbols": symbol_names,
            "symbol_count": symbol_names.len(),
            "direct_importers": importers,
            "blast_radius": affected,
            "total_affected_files": affected.len(),
        }),
        success_meta(BackendKind::Hybrid, 0.85),
    ))
}

pub fn get_symbol_importance(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let top_n = optional_usize(arguments, "top_n", 20);
    Ok(
        get_importance(&state.project(), top_n, &state.graph_cache()).map(|value| {
            (
                json!({ "ranking": value, "count": value.len() }),
                success_meta(BackendKind::Hybrid, 0.84),
            )
        })?,
    )
}

/// Internal helper — historically the `find_dead_code` MCP tool. The
/// public tool was removed when superseded by `dead_code_report`, but
/// internal report builders (boundary, report_jobs) still consume this
/// JSON shape.
pub(crate) fn find_dead_code_v2_tool(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let max_results = optional_usize(arguments, "max_results", 50);
    Ok(
        find_dead_code_v2(&state.project(), max_results, &state.graph_cache()).map(|value| {
            (
                json!({ "dead_code": value, "count": value.len() }),
                success_meta(BackendKind::Hybrid, 0.82),
            )
        })?,
    )
}

pub fn find_orphan_handlers_tool(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let entries = crate::orphan_handlers::find_orphan_handlers(state.project().as_path())?;
    Ok((
        json!({
            "orphan_handlers": entries,
            "count": entries.len(),
        }),
        success_meta(BackendKind::TreeSitter, 0.78),
    ))
}

pub fn find_over_visible_apis_tool(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let entries = crate::over_visible::find_over_visible_apis(state.project().as_path())?;
    let by_kind: std::collections::BTreeMap<String, usize> =
        entries
            .iter()
            .fold(std::collections::BTreeMap::new(), |mut acc, e| {
                *acc.entry(e.kind.clone()).or_insert(0) += 1;
                acc
            });
    Ok((
        json!({
            "over_visible_apis": entries,
            "count": entries.len(),
            "count_by_kind": by_kind,
        }),
        success_meta(BackendKind::TreeSitter, 0.75),
    ))
}

pub fn audit_tool_surface_consistency_tool(
    state: &AppState,
    _arguments: &serde_json::Value,
) -> ToolResult {
    let report = crate::surface_audit::audit_tool_surface_consistency(state.project().as_path())?;
    let drift_count = report.missing_in_dispatch.len() + report.missing_in_toml.len();
    Ok((
        json!({
            "report": report,
            "drift_count": drift_count,
        }),
        success_meta(BackendKind::TreeSitter, 0.92),
    ))
}

pub fn find_phantom_modules_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let max_results = optional_usize(arguments, "max_results", 50);
    let entries = phantom_modules::find_phantom_modules(&state.project(), max_results)?;
    Ok((
        json!({
            "phantom_modules": entries,
            "count": entries.len(),
        }),
        success_meta(BackendKind::TreeSitter, 0.80),
    ))
}

pub fn find_redundant_definitions_tool(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let max_results = optional_usize(arguments, "max_results", 50);
    let entries = redundant_definitions::find_redundant_definitions(&state.project(), max_results)?;
    let mut groups: std::collections::BTreeMap<String, Vec<&_>> = std::collections::BTreeMap::new();
    for entry in &entries {
        groups.entry(entry.target.clone()).or_default().push(entry);
    }
    let grouped = groups
        .iter()
        .map(|(target, members)| {
            json!({
                "target": target,
                "wrapper_count": members.len(),
                "wrappers": members,
            })
        })
        .collect::<Vec<_>>();
    Ok((
        json!({
            "redundant_definitions": entries,
            "count": entries.len(),
            "grouped_by_target": grouped,
        }),
        success_meta(BackendKind::TreeSitter, 0.85),
    ))
}

pub fn find_scoped_references_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let symbol_name = required_string(arguments, "symbol_name")?;
    let file_path = optional_string(arguments, "file_path");
    let max_results = optional_usize(arguments, "max_results", 50);
    Ok(
        find_scoped_references(&state.project(), symbol_name, file_path, max_results).map(
            |refs| {
                (
                    json!({ "references": refs, "count": refs.len() }),
                    success_meta(BackendKind::TreeSitter, 0.95),
                )
            },
        )?,
    )
}

/// Count callers grouped by `(file, function)` so a SCIP entry that
/// attributes a call to the caller-function start line and a tree-sitter
/// entry that attributes the same logical caller to the call-expression
/// line collapse into a single distinct caller.
///
/// The raw `count` field still reports `value.len()` for backwards
/// compatibility; callers that need a deduped tally read
/// `unique_caller_count` from the response.
fn count_distinct_callers(callers: &[codelens_engine::CallerEntry]) -> usize {
    let mut seen: std::collections::HashSet<(&str, &str)> = std::collections::HashSet::new();
    for entry in callers {
        seen.insert((entry.file.as_str(), entry.function.as_str()));
    }
    seen.len()
}

/// Count callees grouped by `(name, resolved_file)` so a SCIP entry and a
/// tree-sitter entry referring to the same callee collapse into one
/// distinct callee even when the per-call-site `line` numbers differ.
fn count_distinct_callees(callees: &[codelens_engine::CalleeEntry]) -> usize {
    let mut seen: std::collections::HashSet<(&str, Option<&str>)> =
        std::collections::HashSet::new();
    for entry in callees {
        seen.insert((entry.name.as_str(), entry.resolved_file.as_deref()));
    }
    seen.len()
}

pub fn get_callers_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    // P1-B — accept `limit`/`top_k` aliases for `max_results` and surface
    // unknown top-level keys so an agent that passes (e.g.) `threshold:
    // 0.5` to a tool that does not honor it sees the field was ignored.
    // See docs/design/arg-validation-policy.md.
    const KNOWN_ARGS: &[&str] = &[
        "function_name",
        "file_path",
        "path",
        "max_results",
        "limit",
        "top_k",
        "max_depth",
        "project_root",
    ];
    let function_name = required_string(arguments, "function_name")?;
    let file_path = optional_string(arguments, "file_path");
    let max_results = crate::tool_runtime::optional_usize_with_aliases(
        arguments,
        "max_results",
        &["limit", "top_k"],
        50,
    );
    let unknown_args = crate::tool_runtime::collect_unknown_args(arguments, KNOWN_ARGS);
    let graph_cache = state.graph_cache();
    Ok(get_callers(
        &state.project(),
        function_name,
        file_path,
        max_results,
        Some(graph_cache.as_ref()),
    )
    .map(|value| {
        // L1 slice 2: when a SCIP index is loaded, prepend type-aware
        // caller entries (resolution: "scip"). SCIP enclosing-scope walk
        // catches callers that tree-sitter's name-based cascade
        // misclassifies — Rust dispatch tables and macro-generated
        // wrappers are the canonical example. Dedup against tree-sitter
        // by (file, function, line) so counts are not inflated.
        #[cfg(feature = "scip-backend")]
        let mut value = value;
        #[cfg(feature = "scip-backend")]
        if let Some(backend) = state.scip() {
            let scip_entries = backend.find_callers(function_name);
            if !scip_entries.is_empty() {
                let existing: std::collections::HashSet<(String, String, usize)> = value
                    .iter()
                    .map(|c| (c.file.clone(), c.function.clone(), c.line))
                    .collect();
                let mut merged: Vec<_> = scip_entries
                    .into_iter()
                    .filter(|c| !existing.contains(&(c.file.clone(), c.function.clone(), c.line)))
                    .collect();
                merged.append(&mut value);
                value = merged;
                if value.len() > max_results {
                    value.truncate(max_results);
                }
            }
        }

        let (resolution_summary, computed_basis, computed_meta) =
            call_graph_analysis(value.iter().map(|entry| entry.resolution));
        // Issue #240: when a SCIP-resolved caller line points at a file
        // that's been edited since the index was built, the precise-tier
        // confidence (≥ 0.95) lies the same way `find_symbol` did
        // pre-#236. Probe staleness against every caller file (cheap —
        // mtime per file) and degrade meta + emit warning when any is
        // newer than the index.
        #[cfg(feature = "scip-backend")]
        let scip_staleness = {
            let candidate_files: Vec<String> = value.iter().map(|c| c.file.clone()).collect();
            crate::tools::scip_health::detect_scip_staleness(
                state.project().as_path(),
                &candidate_files,
            )
        };
        #[cfg(not(feature = "scip-backend"))]
        let scip_staleness: Option<()> = None;
        let (confidence_basis, meta) = if scip_staleness.is_some() {
            (
                "scip_precise_stale_index",
                crate::tool_evidence::meta_degraded("scip", 0.55, "scip_index_stale_vs_source"),
            )
        } else {
            (computed_basis, computed_meta)
        };
        let evidence = crate::tool_evidence::tool_evidence(
            "call_graph",
            &meta,
            confidence_basis,
            call_graph_evidence_signals(&resolution_summary),
        );
        let unique_caller_count = count_distinct_callers(&value);
        let mut payload = json!({
            "function": function_name,
            "callers": value,
            "count": value.len(),
            "unique_caller_count": unique_caller_count,
            "confidence_basis": confidence_basis,
            "resolution_summary": resolution_summary,
            "evidence": evidence,
        });
        #[cfg(feature = "scip-backend")]
        if let Some(stale) = scip_staleness.as_ref()
            && let Some(map) = payload.as_object_mut()
        {
            map.insert(
                "scip_index_stale_warning".to_owned(),
                crate::tools::scip_health::scip_stale_warning_payload(stale),
            );
        }
        if !unknown_args.is_empty()
            && let Some(map) = payload.as_object_mut()
        {
            map.insert("unknown_args".to_owned(), json!(unknown_args));
        }
        (payload, meta)
    })?)
}

pub fn get_callees_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    const KNOWN_ARGS: &[&str] = &[
        "function_name",
        "file_path",
        "path",
        "max_results",
        "limit",
        "top_k",
        "max_depth",
        "project_root",
    ];
    let function_name = required_string(arguments, "function_name")?;
    let file_path = optional_string(arguments, "file_path");
    let max_results = crate::tool_runtime::optional_usize_with_aliases(
        arguments,
        "max_results",
        &["limit", "top_k"],
        50,
    );
    let unknown_args = crate::tool_runtime::collect_unknown_args(arguments, KNOWN_ARGS);
    let graph_cache = state.graph_cache();
    Ok(get_callees(
        &state.project(),
        function_name,
        file_path,
        max_results,
        Some(graph_cache.as_ref()),
    )
    .map(|value| {
        // L1: when a SCIP index is loaded, prepend type-aware callee
        // entries that the tree-sitter cascade missed (e.g. Rust dispatch
        // table dispatch_tool → tree-sitter sees a path expression, not a
        // call). Dedup against tree-sitter results by (name, line) so we
        // never inflate counts; SCIP entries win on conflicts because
        // their resolved_file is authoritative.
        #[cfg(feature = "scip-backend")]
        let mut value = value;
        #[cfg(feature = "scip-backend")]
        if let (Some(target_file), Some(backend)) = (file_path, state.scip()) {
            use codelens_engine::PreciseBackend as _;
            if backend.has_index_for(target_file) {
                let scip_entries = backend.find_callees(function_name, target_file);
                if !scip_entries.is_empty() {
                    let existing: std::collections::HashSet<(String, usize)> = value
                        .iter()
                        .map(|edge| (edge.name.clone(), edge.line))
                        .collect();
                    let mut merged: Vec<_> = scip_entries
                        .into_iter()
                        .filter(|entry| !existing.contains(&(entry.name.clone(), entry.line)))
                        .collect();
                    merged.append(&mut value);
                    value = merged;
                    if value.len() > max_results {
                        value.truncate(max_results);
                    }
                }
            }
        }

        let (resolution_summary, confidence_basis, meta) =
            call_graph_analysis(value.iter().map(|entry| entry.resolution));
        let evidence = crate::tool_evidence::tool_evidence(
            "call_graph",
            &meta,
            confidence_basis,
            call_graph_evidence_signals(&resolution_summary),
        );
        let unique_callee_count = count_distinct_callees(&value);
        let mut payload = json!({
            "function": function_name,
            "callees": value,
            "count": value.len(),
            "unique_callee_count": unique_callee_count,
            "confidence_basis": confidence_basis,
            "resolution_summary": resolution_summary,
            "evidence": evidence,
        });
        if !unknown_args.is_empty()
            && let Some(map) = payload.as_object_mut()
        {
            map.insert("unknown_args".to_owned(), json!(unknown_args));
        }
        (payload, meta)
    })?)
}

pub fn find_circular_dependencies_tool(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let max_results = optional_usize(arguments, "max_results", 50);
    Ok(
        find_circular_dependencies(&state.project(), max_results, &state.graph_cache()).map(
            |value| {
                (
                    json!({ "cycles": value, "count": value.len() }),
                    success_meta(BackendKind::Hybrid, 0.88),
                )
            },
        )?,
    )
}

pub fn get_change_coupling_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let months = optional_usize(arguments, "months", 6);
    let min_strength = arguments
        .get("min_strength")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.3);
    let min_commits = optional_usize(arguments, "min_commits", 3);
    let max_results = optional_usize(arguments, "max_results", 30);
    Ok(get_change_coupling(
        &state.project(),
        months,
        min_strength,
        min_commits,
        max_results,
    )
    .map(|value| {
        (
            json!({ "coupling": value, "count": value.len() }),
            success_meta(BackendKind::Git, 0.85),
        )
    })?)
}

#[cfg(test)]
mod tests {
    use super::{count_distinct_callees, count_distinct_callers};
    use codelens_engine::{CalleeEntry, CallerEntry};

    fn caller(file: &str, function: &str, line: usize, resolution: &'static str) -> CallerEntry {
        CallerEntry {
            file: file.to_string(),
            function: function.to_string(),
            line,
            confidence: 0.9,
            resolution: Some(resolution),
        }
    }

    fn callee(
        name: &str,
        line: usize,
        resolved_file: Option<&str>,
        resolution: &'static str,
    ) -> CalleeEntry {
        CalleeEntry {
            name: name.to_string(),
            line,
            resolved_file: resolved_file.map(str::to_owned),
            confidence: 0.9,
            resolution: Some(resolution),
        }
    }

    #[test]
    fn count_distinct_callers_collapses_scip_and_tree_sitter_for_same_caller_function() {
        // Mirrors the #207-C-1 dogfood observation: the same caller appears
        // once with line attributed by SCIP (caller-fn start) and again with
        // line attributed by tree-sitter (call-expression site), differing
        // by +/-1. The existing (file, function, line) dedup misses these
        // pairs; (file, function) collapses them into a single distinct
        // caller.
        let callers = vec![
            caller("filesystem.rs", "get_current_config", 36, "scip"),
            caller("filesystem.rs", "get_current_config", 37, "unique_name"),
            caller("resources.rs", "read_resource", 137, "scip"),
            caller("resources.rs", "read_resource", 136, "unique_name"),
        ];
        assert_eq!(count_distinct_callers(&callers), 2);
    }

    #[test]
    fn count_distinct_callers_keeps_separate_caller_functions() {
        let callers = vec![
            caller("a.rs", "foo", 1, "scip"),
            caller("a.rs", "bar", 1, "scip"),
            caller("b.rs", "foo", 1, "scip"),
        ];
        assert_eq!(count_distinct_callers(&callers), 3);
    }

    #[test]
    fn count_distinct_callees_collapses_same_name_resolved_file_pairs() {
        let callees = vec![
            callee("bar", 5, Some("b.rs"), "scip"),
            callee("bar", 6, Some("b.rs"), "scip"),
            callee("bar", 7, Some("b.rs"), "unique_name"),
        ];
        assert_eq!(count_distinct_callees(&callees), 1);
    }

    #[test]
    fn count_distinct_callees_separates_unresolved_and_resolved_callees_with_same_name() {
        let callees = vec![
            callee("bar", 5, Some("b.rs"), "scip"),
            callee("bar", 5, None, "unique_name"),
        ];
        // (bar, Some("b.rs")) and (bar, None) are distinct buckets so a
        // resolved callee and an unresolved callee with the same identifier
        // remain countable separately.
        assert_eq!(count_distinct_callees(&callees), 2);
    }
}
