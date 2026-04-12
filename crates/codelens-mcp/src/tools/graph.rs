use super::{
    AppState, ToolResult, optional_bool, optional_string, optional_usize, required_string,
    success_meta,
};
use crate::protocol::BackendKind;
use crate::tools::symbols::flatten_symbols;
use codelens_engine::{
    find_circular_dependencies, find_dead_code_v2, find_scoped_references, get_blast_radius,
    get_callees, get_callers, get_change_coupling, get_changed_files, get_importance,
    get_importers, search_for_pattern,
};
use serde_json::json;

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

pub fn get_impact_analysis(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
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

pub fn find_importers_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let max_results = optional_usize(arguments, "max_results", 50);
    Ok(get_importers(
        &state.project(),
        file_path,
        max_results,
        &state.graph_cache(),
    )
    .map(|value| {
        (
            json!({ "file": file_path, "importers": value, "count": value.len() }),
            success_meta(BackendKind::Hybrid, 0.87),
        )
    })?)
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

pub fn find_dead_code_v2_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
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

pub fn find_referencing_code_snippets(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let symbol_name = required_string(arguments, "symbol_name")?;
    let file_glob = optional_string(arguments, "file_glob");
    let context_lines = optional_usize(arguments, "context_lines", 2);
    let max_results = optional_usize(arguments, "max_results", 50);
    Ok(search_for_pattern(
        &state.project(),
        symbol_name,
        file_glob,
        max_results,
        context_lines,
        context_lines,
    )
    .map(|matches| {
        let snippets = matches
            .iter()
            .map(|m| {
                let mut obj = json!({
                    "file_path": m.file_path,
                    "line": m.line,
                    "column": m.column,
                    "matched_text": m.matched_text,
                    "line_content": m.line_content,
                });
                if !m.context_before.is_empty() {
                    obj["context_before"] = json!(m.context_before);
                }
                if !m.context_after.is_empty() {
                    obj["context_after"] = json!(m.context_after);
                }
                obj
            })
            .collect::<Vec<_>>();
        (
            json!({ "snippets": snippets, "count": snippets.len() }),
            success_meta(BackendKind::Filesystem, 0.92),
        )
    })?)
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

pub fn get_callers_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let function_name = required_string(arguments, "function_name")?;
    let max_results = optional_usize(arguments, "max_results", 50);
    Ok(
        get_callers(&state.project(), function_name, max_results).map(|value| {
            (
                json!({ "function": function_name, "callers": value, "count": value.len() }),
                success_meta(BackendKind::Hybrid, 0.85),
            )
        })?,
    )
}

pub fn get_callees_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let function_name = required_string(arguments, "function_name")?;
    let file_path = optional_string(arguments, "file_path");
    let max_results = optional_usize(arguments, "max_results", 50);
    Ok(
        get_callees(&state.project(), function_name, file_path, max_results).map(|value| {
            (
                json!({ "function": function_name, "callees": value, "count": value.len() }),
                success_meta(BackendKind::Hybrid, 0.85),
            )
        })?,
    )
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

pub fn get_architecture_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let min_size = arguments
        .get("min_community_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(2) as usize;

    let graph = state.graph_cache().get_or_build(&state.project())?;
    let overview = codelens_engine::community::detect_communities(&graph, min_size)?;

    Ok((
        json!({
            "communities": overview.communities,
            "total_files": overview.total_files,
            "total_edges": overview.total_edges,
            "modularity": (overview.modularity * 1000.0).round() / 1000.0,
            "community_count": overview.communities.len(),
        }),
        success_meta(BackendKind::Hybrid, 0.88),
    ))
}
