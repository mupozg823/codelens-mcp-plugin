//! Index-maintenance and corpus-shape tools:
//!   - `refresh_symbol_index` — force re-scan + graph cache invalidate
//!     + (with `semantic` feature) embedding freshness probe.
//!   - `get_complexity` — per-function branch-count complexity report.
//!   - `get_project_structure` — directory tree summary.
//!
//! Distinct from the symbol-query pipeline: these tools probe or
//! mutate the **on-disk index state** itself, not symbol semantics.
//! Kept together because they all read/refresh the same
//! `SymbolIndex` / `graph_cache` / `embedding_ref` triplet on
//! `AppState`.

use super::super::{AppState, ToolResult, optional_string, required_string, success_meta};
use super::formatter::count_branches;
use crate::protocol::BackendKind;
use crate::tools::symbol_query::sparse_retriever::flatten_symbols;
use codelens_engine::{SymbolKind, read_file};
use serde_json::{Value, json};

pub fn refresh_symbol_index(state: &AppState, _arguments: &Value) -> ToolResult {
    let stats = state.symbol_index().refresh_all()?;
    state.graph_cache().invalidate();
    // A forced re-scan can land in the same wall-clock tick as the sparse
    // cache's `(file_count, max_indexed_at)` fingerprint, which would let the
    // BM25/ranked path keep serving the pre-refresh snapshot. Drop this
    // project's sparse entries so the next query rebuilds from fresh symbols.
    state
        .sparse_symbol_cache()
        .invalidate_project(&state.current_project_scope());
    #[cfg(feature = "semantic")]
    let mut payload = json!(stats);
    #[cfg(not(feature = "semantic"))]
    let mut payload = json!(stats);
    #[cfg(feature = "semantic")]
    {
        let project = state.project();
        let guard = state.embedding_ref();
        if let Some(engine) = guard.as_ref()
            && engine.is_indexed()
        {
            match engine.ensure_index_fresh_for_project(&project) {
                Ok(report) => {
                    if let Some(map) = payload.as_object_mut() {
                        map.insert("embedding_freshness".to_owned(), json!(report));
                    }
                }
                Err(error) => {
                    if let Some(map) = payload.as_object_mut() {
                        map.insert(
                            "embedding_freshness".to_owned(),
                            json!({
                                "status": "unavailable",
                                "reason": error.to_string()
                            }),
                        );
                    }
                }
            }
        }
    }
    // #358 defense-in-depth: a zero-file refresh is almost always a
    // misconfiguration (wrong project binding, over-broad excludes) or a
    // discovery defect — flag it instead of returning a bare success the
    // caller mistakes for a healthy empty index.
    if let Some(map) = payload.as_object_mut()
        && map
            .get("supported_files")
            .and_then(Value::as_u64)
            .is_some_and(|count| count == 0)
    {
        map.insert(
            "warning".to_owned(),
            json!({
                "code": "zero_supported_files",
                "message": "No supported files were discovered under the project root. \
                            Verify the project binding points at the intended workspace \
                            and check .codelens/config.json exclude patterns — a wrong \
                            root or over-broad excludes silently yield an empty index.",
            }),
        );
    }
    Ok((payload, success_meta(BackendKind::TreeSitter, 0.95)))
}

pub fn get_complexity(state: &AppState, arguments: &Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    let symbol_name = optional_string(arguments, "symbol_name");
    let file_result = read_file(&state.project(), path, None, None)?;
    let lines = file_result.content.lines().collect::<Vec<_>>();
    let symbols = state.symbol_index().get_symbols_overview_cached(path, 2)?;

    let functions = flatten_symbols(&symbols)
        .into_iter()
        .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
        .filter(|s| symbol_name.is_none_or(|name| s.name == name))
        .map(|s| {
            let start = s.line.saturating_sub(1).min(lines.len());
            let end = (s.line + 50).min(lines.len());
            let branches = count_branches(&lines[start..end]);
            json!({
                "name": s.name,
                "kind": s.kind.as_label(),
                "file": s.file_path,
                "line": s.line,
                "branches": branches,
                "complexity": 1 + branches
            })
        })
        .collect::<Vec<_>>();

    let results = if functions.is_empty() {
        let branches = count_branches(&lines);
        vec![json!({
            "name": path,
            "branches": branches,
            "complexity": 1 + branches
        })]
    } else {
        functions
    };

    let avg_complexity = if results.is_empty() {
        0.0
    } else {
        results
            .iter()
            .filter_map(|e| e.get("complexity").and_then(|v| v.as_i64()))
            .map(|v| v as f64)
            .sum::<f64>()
            / results.len() as f64
    };

    Ok((
        json!({
            "path": path,
            "functions": results,
            "count": results.len(),
            "avg_complexity": avg_complexity
        }),
        success_meta(BackendKind::TreeSitter, 0.89),
    ))
}

#[allow(dead_code)]
pub fn get_project_structure(state: &AppState, _arguments: &Value) -> ToolResult {
    let dirs = state.symbol_index().get_project_structure()?;
    let total_files: usize = dirs.iter().map(|d| d.files).sum();
    let total_symbols: usize = dirs.iter().map(|d| d.symbols).sum();
    Ok((
        json!({
            "directories": dirs,
            "total_files": total_files,
            "total_symbols": total_symbols,
            "dir_count": dirs.len()
        }),
        success_meta(BackendKind::Sqlite, 0.95),
    ))
}
