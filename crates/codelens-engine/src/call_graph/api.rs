use crate::import_graph::GraphCache;
use crate::project::ProjectRoot;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use super::extract::extract_calls;
use super::js_imports::{build_js_import_binding_index, filter_external_import_edges};
use super::resolve::{collect_candidate_files, maybe_import_graph, resolve_call_edges};
use super::types::{CallEdge, CalleeEntry, CallerEntry};

/// Find all functions that call `function_name` across the project.
/// Edges are resolved via the 6-stage confidence cascade when an import graph is available.
pub fn get_callers(
    project: &ProjectRoot,
    function_name: &str,
    file_path: Option<&str>,
    max_results: usize,
    graph_cache: Option<&GraphCache>,
) -> Result<Vec<CallerEntry>> {
    let files: Vec<PathBuf> = if let Some(fp) = file_path {
        vec![project.resolve(fp)?]
    } else {
        collect_candidate_files(project.as_path())?
    };
    let mut all_edges: Vec<CallEdge> = Vec::new();

    for file in &files {
        let mut edges = extract_calls(file);
        // Relativize caller_file paths
        for edge in &mut edges {
            edge.caller_file = project.to_relative(file);
        }
        all_edges.extend(edges);
    }

    let import_bindings = build_js_import_binding_index(project, &files);
    filter_external_import_edges(&mut all_edges, &import_bindings);
    let import_graph = maybe_import_graph(project, &files, graph_cache);
    resolve_call_edges(
        &mut all_edges,
        project,
        import_graph.as_deref(),
        Some(&import_bindings),
    );

    // Filter to edges calling our target
    let mut seen = std::collections::HashSet::new();
    let mut results = Vec::new();

    for edge in all_edges {
        if edge.callee_name == function_name
            || edge.canonical_callee_name.as_deref() == Some(function_name)
        {
            let key = (
                edge.caller_file.clone(),
                edge.caller_name.clone(),
                edge.line,
            );
            if seen.insert(key) {
                results.push(CallerEntry {
                    file: edge.caller_file,
                    function: edge.caller_name,
                    line: edge.line,
                    confidence: edge.confidence,
                    resolution: edge.resolution_strategy,
                });
            }
        }
    }

    // Sort by confidence descending
    results.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if max_results > 0 && results.len() > max_results {
        results.truncate(max_results);
    }
    Ok(results)
}

/// Find all functions called by `function_name` (optionally restricted to a file).
/// Callee names are resolved to their definition files via the 6-stage cascade.
pub fn get_callees(
    project: &ProjectRoot,
    function_name: &str,
    file_path: Option<&str>,
    max_results: usize,
    graph_cache: Option<&GraphCache>,
) -> Result<Vec<CalleeEntry>> {
    let files: Vec<PathBuf> = if let Some(fp) = file_path {
        let resolved = project.resolve(fp)?;
        vec![resolved]
    } else {
        collect_candidate_files(project.as_path())?
    };

    let mut all_edges: Vec<CallEdge> = Vec::new();
    for file in &files {
        let mut edges = extract_calls(file);
        for edge in &mut edges {
            edge.caller_file = project.to_relative(file);
        }
        all_edges.extend(edges);
    }

    let import_bindings = build_js_import_binding_index(project, &files);
    filter_external_import_edges(&mut all_edges, &import_bindings);
    let import_graph = maybe_import_graph(project, &files, graph_cache);
    resolve_call_edges(
        &mut all_edges,
        project,
        import_graph.as_deref(),
        Some(&import_bindings),
    );

    let mut seen: HashMap<(String, usize), ()> = HashMap::new();
    let mut results = Vec::new();

    for edge in all_edges {
        if edge.caller_name == function_name {
            let key = (edge.callee_name.clone(), edge.line);
            if seen.insert(key, ()).is_none() {
                results.push(CalleeEntry {
                    name: edge.callee_name,
                    line: edge.line,
                    resolved_file: edge.resolved_file,
                    confidence: edge.confidence,
                    resolution: edge.resolution_strategy,
                });
            }
        }
    }

    results.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if max_results > 0 && results.len() > max_results {
        results.truncate(max_results);
    }
    Ok(results)
}
