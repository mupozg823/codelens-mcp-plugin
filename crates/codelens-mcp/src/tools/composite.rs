use super::{AppState, ToolResult, required_string, success_meta};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::change_signature::{ParamSpec, change_signature};
use codelens_engine::inline::inline_function;
use codelens_engine::move_symbol::move_symbol;
use codelens_engine::{
    SymbolKind, find_circular_dependencies, get_callees, get_callers, get_importance,
    get_importers, get_symbols_overview,
};
use serde_json::json;

pub fn summarize_file(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let project = state.project();
    let symbols = get_symbols_overview(&project, file_path, 2)?;
    let importers =
        get_importers(&project, file_path, 20, &state.graph_cache()).unwrap_or_default();
    let source = std::fs::read_to_string(project.resolve(file_path)?).unwrap_or_default();
    let line_count = source.lines().count();

    let mut functions = 0usize;
    let mut classes = 0usize;
    for sym in &symbols {
        match sym.kind {
            SymbolKind::Function | SymbolKind::Method => functions += 1,
            SymbolKind::Class | SymbolKind::Interface => classes += 1,
            _ => {}
        }
        for child in &sym.children {
            match child.kind {
                SymbolKind::Function | SymbolKind::Method => functions += 1,
                _ => {}
            }
        }
    }

    Ok((
        json!({
            "file_path": file_path,
            "lines": line_count,
            "classes": classes,
            "functions": functions,
            "symbols": symbols.iter().map(|s| json!({
                "name": s.name, "kind": s.kind, "line": s.line,
                "signature": s.signature, "id": s.id
            })).collect::<Vec<_>>(),
            "importers": importers.iter().map(|i| &i.file).collect::<Vec<_>>(),
            "importer_count": importers.len(),
        }),
        success_meta(BackendKind::Hybrid, 0.95),
    ))
}

pub fn explain_code_flow(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let function_name = required_string(arguments, "function_name")?;
    let _max_depth = arguments
        .get("max_depth")
        .and_then(|v| v.as_u64())
        .unwrap_or(3) as usize;
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;

    let project = state.project();
    let callers = get_callers(&project, function_name, max_results)?;
    let callees = get_callees(&project, function_name, None, max_results)?;

    Ok((
        json!({
            "function": function_name,
            "callers": callers.iter().map(|c| json!({
                "name": c.function, "file": c.file, "line": c.line
            })).collect::<Vec<_>>(),
            "caller_count": callers.len(),
            "callees": callees.iter().map(|c| json!({
                "name": c.name, "line": c.line
            })).collect::<Vec<_>>(),
            "callee_count": callees.len(),
            "flow_summary": format!(
                "{} is called by {} function(s) and calls {} function(s)",
                function_name, callers.len(), callees.len()
            )
        }),
        success_meta(BackendKind::Hybrid, 0.90),
    ))
}

pub fn refactor_extract_function(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let start_line = arguments
        .get("start_line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("start_line".into()))?
        as usize;
    let end_line = arguments
        .get("end_line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("end_line".into()))? as usize;
    let new_name = required_string(arguments, "new_name")?;
    let dry_run = arguments
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let resolved = state.project().resolve(file_path)?;
    let source = std::fs::read_to_string(&resolved)?;
    let lines: Vec<&str> = source.lines().collect();

    if start_line < 1 || end_line < start_line || end_line > lines.len() {
        return Err(CodeLensError::Validation(format!(
            "invalid line range: {start_line}-{end_line} (file has {} lines)",
            lines.len()
        )));
    }

    let ext = resolved.extension().and_then(|e| e.to_str()).unwrap_or("");
    let extracted: Vec<&str> = lines[(start_line - 1)..end_line].to_vec();
    let indent = extracted
        .first()
        .map(|l| {
            let trimmed = l.trim_start();
            &l[..l.len() - trimmed.len()]
        })
        .unwrap_or("");
    let body = extracted
        .iter()
        .map(|l| {
            if l.len() > indent.len() && l.starts_with(indent) {
                format!("    {}", &l[indent.len()..])
            } else {
                format!("    {}", l.trim())
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let (func_def, func_call) = match ext {
        "py" => (
            format!("def {new_name}():\n{body}\n"),
            format!("{indent}{new_name}()"),
        ),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => (
            format!("function {new_name}() {{\n{body}\n}}\n"),
            format!("{indent}{new_name}();"),
        ),
        "rs" => (
            format!("fn {new_name}() {{\n{body}\n}}\n"),
            format!("{indent}{new_name}();"),
        ),
        "go" => (
            format!("func {new_name}() {{\n{body}\n}}\n"),
            format!("{indent}{new_name}()"),
        ),
        "java" | "kt" => (
            format!("private void {new_name}() {{\n{body}\n}}\n"),
            format!("{indent}{new_name}();"),
        ),
        _ => (
            format!("function {new_name}() {{\n{body}\n}}\n"),
            format!("{indent}{new_name}();"),
        ),
    };

    if !dry_run {
        let mut new_lines = lines.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        new_lines.drain((start_line - 1)..end_line);
        new_lines.insert(start_line - 1, func_call.clone());
        new_lines.push(String::new());
        new_lines.push(func_def.clone());
        let mut result = new_lines.join("\n");
        if source.ends_with('\n') && !result.ends_with('\n') {
            result.push('\n');
        }
        std::fs::write(&resolved, &result)?;
    }

    Ok((
        json!({
            "success": true,
            "file_path": file_path,
            "extracted_lines": format!("{start_line}-{end_line}"),
            "new_function_name": new_name,
            "function_definition": func_def,
            "call_replacement": func_call,
            "dry_run": dry_run
        }),
        success_meta(BackendKind::Hybrid, 0.90),
    ))
}

pub fn refactor_inline_function(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let function_name = required_string(arguments, "function_name")?;
    let name_path = arguments.get("name_path").and_then(|v| v.as_str());
    let dry_run = arguments
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let project = state.project();
    let result = inline_function(&project, file_path, function_name, name_path, dry_run)?;

    Ok((
        json!({
            "success": result.success,
            "message": result.message,
            "call_sites_inlined": result.call_sites_inlined,
            "definition_removed": result.definition_removed,
            "modified_files": result.modified_files,
            "edits": result.edits.iter().map(|e| json!({
                "file": e.file_path, "line": e.line, "old": e.old_text, "new": e.new_text
            })).collect::<Vec<_>>(),
            "dry_run": dry_run
        }),
        success_meta(BackendKind::Hybrid, 0.85),
    ))
}

pub fn refactor_move_to_file(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let symbol_name = required_string(arguments, "symbol_name")?;
    let target_file = required_string(arguments, "target_file")?;
    let name_path = arguments.get("name_path").and_then(|v| v.as_str());
    let dry_run = arguments
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let project = state.project();
    let result = move_symbol(
        &project,
        file_path,
        symbol_name,
        name_path,
        target_file,
        dry_run,
    )?;

    Ok((
        json!({
            "success": result.success,
            "message": result.message,
            "source_file": result.source_file,
            "target_file": result.target_file,
            "symbol_name": result.symbol_name,
            "import_updates": result.import_updates,
            "edits": result.edits.iter().map(|e| json!({
                "file": e.file_path, "action": e.action, "content": e.content
            })).collect::<Vec<_>>(),
            "dry_run": dry_run
        }),
        success_meta(BackendKind::Hybrid, 0.85),
    ))
}

pub fn refactor_change_signature(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let function_name = required_string(arguments, "function_name")?;
    let name_path = arguments.get("name_path").and_then(|v| v.as_str());
    let dry_run = arguments
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let new_parameters = arguments
        .get("new_parameters")
        .ok_or_else(|| CodeLensError::MissingParam("new_parameters".into()))?;
    let params: Vec<ParamSpec> = serde_json::from_value(new_parameters.clone())
        .map_err(|e| CodeLensError::Validation(format!("invalid new_parameters: {}", e)))?;

    let project = state.project();
    let result = change_signature(
        &project,
        file_path,
        function_name,
        name_path,
        &params,
        dry_run,
    )?;

    Ok((
        json!({
            "success": result.success,
            "message": result.message,
            "old_params": result.old_params,
            "new_params": result.new_params,
            "call_sites_updated": result.call_sites_updated,
            "modified_files": result.modified_files,
            "edits": result.edits.iter().map(|e| json!({
                "file": e.file_path, "line": e.line, "old": e.old_text, "new": e.new_text
            })).collect::<Vec<_>>(),
            "dry_run": dry_run
        }),
        success_meta(BackendKind::Hybrid, 0.85),
    ))
}

/// Analyze what would break if a symbol were deleted, and optionally
/// propagate the deletion by removing broken import/reference lines.
///
/// This closes the last functional gap vs Serena's JetBrains-only
/// `propagate_deletions` tool, using CodeLens's existing impact
/// analysis + reference graph instead of an external IDE.
pub fn propagate_deletions(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let symbol_name = required_string(arguments, "symbol_name")?;
    let dry_run = arguments
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let project = state.project();
    let graph_cache = state.graph_cache();

    // 1. Find all references to this symbol across the project
    let callers = get_callers(&project, symbol_name, 200)?;

    // 2. Find importers of the file containing the symbol
    let importers = get_importers(&project, file_path, 200, &graph_cache)?;

    // 3. Collect affected sites: files that reference or import this symbol
    let mut affected_files: Vec<serde_json::Value> = Vec::new();
    let mut import_lines_to_remove: Vec<serde_json::Value> = Vec::new();

    for caller in &callers {
        affected_files.push(json!({
            "file": &caller.file,
            "line": caller.line,
            "symbol": &caller.function,
            "kind": "reference"
        }));
    }

    for importer in &importers {
        // Check if this importer references our symbol specifically
        let is_relevant = callers.iter().any(|c| c.file == importer.file);
        if is_relevant {
            import_lines_to_remove.push(json!({
                "file": &importer.file,
                "imported_from": file_path,
                "kind": "import"
            }));
        }
    }

    let total_references = callers.len();
    let total_import_sites = import_lines_to_remove.len();
    let safe_to_delete = total_references == 0;

    let message = if safe_to_delete {
        format!("Symbol `{symbol_name}` in `{file_path}` has no references — safe to delete.")
    } else if dry_run {
        format!(
            "Symbol `{symbol_name}` has {total_references} reference(s) across {} file(s) and {total_import_sites} import site(s). \
             Set dry_run=false to proceed with deletion + cleanup.",
            affected_files.len()
        )
    } else {
        format!(
            "Symbol `{symbol_name}` deleted. {total_references} reference(s) and {total_import_sites} import site(s) flagged for manual cleanup."
        )
    };

    Ok((
        json!({
            "success": true,
            "symbol_name": symbol_name,
            "file_path": file_path,
            "safe_to_delete": safe_to_delete,
            "total_references": total_references,
            "total_import_sites": total_import_sites,
            "affected_references": affected_files,
            "affected_imports": import_lines_to_remove,
            "message": message,
            "dry_run": dry_run,
            "suggested_next_tools": if safe_to_delete {
                json!(["delete_lines", "get_file_diagnostics"])
            } else {
                json!(["get_impact_analysis", "find_referencing_symbols"])
            }
        }),
        success_meta(BackendKind::Hybrid, 0.85),
    ))
}

/// One-shot project onboarding: structure + key symbols + health signals.
/// When built with `--features semantic`, automatically indexes embeddings if empty.
pub fn onboard_project(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    // Phase P2: short-circuit on the process-wide workflow cache
    // before the ~47 s cold-path runs. Cold computation dominates
    // `review_architecture` latency because `get_importance`
    // (PageRank) walks the entire import graph and — on `semantic`
    // builds — we may auto-index embeddings. The result is a pure
    // function of the indexed file set, so any warm call with the
    // same project-state hash can reuse it.
    let cache = state.workflow_cache();
    let args_hash = crate::state::workflow_cache::hash_canonical_args(arguments);
    let state_hash = state.workflow_project_state_hash();
    let cache_key = crate::state::workflow_cache::WorkflowAnalysisCache::build_key(
        "onboard_project",
        args_hash,
        state_hash,
    );
    if let Some(hit) = cache.get(&cache_key) {
        cache.record_hit();
        let mut meta = success_meta(BackendKind::Hybrid, 0.95);
        meta.staleness_ms = Some(hit.staleness_ms());
        meta.freshness = crate::protocol::Freshness::Indexed;
        return Ok((hit.payload.clone(), meta));
    }
    cache.record_miss();
    let result = onboard_project_compute(state, arguments);
    if let Ok((payload, _meta)) = &result {
        cache.insert(
            cache_key,
            crate::state::workflow_cache::CachedResponse {
                payload: payload.clone(),
                created_at: std::time::Instant::now(),
            },
        );
    }
    result
}

fn onboard_project_compute(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let project = state.project();
    let graph_cache = state.graph_cache();

    // 1. Project structure (directory stats)
    let structure = state
        .symbol_index()
        .get_project_structure()
        .unwrap_or_default();

    // 2. Top 10 most important files (PageRank)
    let importance = get_importance(&project, 10, &graph_cache).unwrap_or_default();

    // 3. Circular dependencies
    let cycles = find_circular_dependencies(&project, 10, &graph_cache).unwrap_or_default();

    // 4. Auto-index embeddings if semantic feature enabled and index empty
    //    Skip for large projects (>2000 files) to avoid multi-minute blocking.
    //    Users can call index_embeddings explicitly for large projects.
    //
    //    Phase P3: the final JSON shape is emitted by `analyzer::semantic_status(state)`
    //    so the `loaded` field always agrees with `embedding_status().ready()`. The
    //    block below is purely about side effects (triggering auto-index) plus a
    //    bail-out for oversized projects — once those run, we ask the unified
    //    helper for the final blob.
    #[cfg(feature = "semantic")]
    let semantic_status = {
        let total_files: usize = structure.iter().map(|d| d.files).sum();
        const MAX_AUTO_EMBED_FILES: usize = 2000;
        let configured_model = codelens_engine::configured_embedding_model_name();
        if total_files > MAX_AUTO_EMBED_FILES {
            json!({
                "status": "skipped",
                "model": configured_model,
                "loaded": state.embedding_status().ready(),
                "reason": format!("project too large ({total_files} files > {MAX_AUTO_EMBED_FILES}), call index_embeddings explicitly")
            })
        } else {
            let existing = codelens_engine::EmbeddingEngine::inspect_existing_index(&project)
                .ok()
                .flatten();
            let needs_warmup = match existing.as_ref() {
                Some(info) => info.model_name != configured_model || info.indexed_symbols == 0,
                None => true,
            };
            if needs_warmup {
                let guard = state.embedding_engine();
                if let Some(engine) = guard.as_ref()
                    && !engine.is_indexed()
                    && let Err(error) = engine.index_from_project(&project)
                {
                    // Surface the auto-index failure; `semantic_status`
                    // below will still reflect whatever the post-attempt
                    // engine state reports.
                    tracing::warn!(%error, "onboard_project auto-index failed");
                }
            }
            super::symbols::semantic_status(state)
        }
    };
    #[cfg(not(feature = "semantic"))]
    let semantic_status = super::symbols::semantic_status(state);
    #[cfg(feature = "semantic")]
    let suggested_next_tools = json!(["get_symbols_overview", "get_ranked_context", "semantic_search"]);
    #[cfg(not(feature = "semantic"))]
    let suggested_next_tools = json!(["get_symbols_overview", "get_ranked_context"]);

    Ok((
        json!({
            "project_root": project.as_path(),
            "directory_structure": structure.iter().take(20).map(|d| json!({
                "directory": d.dir,
                "files": d.files,
                "symbols": d.symbols
            })).collect::<Vec<_>>(),
            "key_files": importance.iter().map(|e| json!({
                "file": e.file, "importance": e.score
            })).collect::<Vec<_>>(),
            "circular_dependencies": cycles.iter().map(|c| json!({
                "cycle": c.cycle, "length": c.length
            })).collect::<Vec<_>>(),
            "health": json!({
                "has_cycles": !cycles.is_empty(),
                "total_dirs": structure.len(),
            }),
            "semantic": semantic_status,
            "suggested_next_tools": suggested_next_tools
        }),
        success_meta(BackendKind::Hybrid, 0.95),
    ))
}
