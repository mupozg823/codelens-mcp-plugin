use crate::protocol::BackendKind;
use crate::session_metrics_payload::build_session_metrics_payload;
use crate::tool_defs::{
    ToolPreset, ToolProfile, ToolSurface, default_budget_for_preset, default_budget_for_profile,
};
use crate::tools::{AppState, ToolResult, success_meta};
use serde_json::json;

pub fn get_watch_status(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let failure_health = state.watcher_failure_health();
    match &state.watcher {
        Some(watcher) => {
            let mut stats = watcher.stats();
            stats.index_failures = Some(failure_health.recent_failures);
            let mut payload = serde_json::to_value(stats).unwrap_or_else(|_| json!({}));
            if let Some(map) = payload.as_object_mut() {
                map.insert(
                    "index_failures_total".to_owned(),
                    json!(failure_health.total_failures),
                );
                map.insert(
                    "stale_index_failures".to_owned(),
                    json!(failure_health.stale_failures),
                );
                map.insert(
                    "persistent_index_failures".to_owned(),
                    json!(failure_health.persistent_failures),
                );
                map.insert(
                    "pruned_missing_failures".to_owned(),
                    json!(failure_health.pruned_missing_failures),
                );
                map.insert(
                    "recent_failure_window_seconds".to_owned(),
                    json!(failure_health.recent_window_seconds),
                );
            }
            Ok((payload, success_meta(BackendKind::Config, 1.0)))
        }
        None => Ok((
            json!({
                "running": false,
                "events_processed": 0,
                "files_reindexed": 0,
                "lock_contention_batches": 0,
                "index_failures": failure_health.recent_failures,
                "index_failures_total": failure_health.total_failures,
                "stale_index_failures": failure_health.stale_failures,
                "persistent_index_failures": failure_health.persistent_failures,
                "pruned_missing_failures": failure_health.pruned_missing_failures,
                "recent_failure_window_seconds": failure_health.recent_window_seconds,
                "note": "File watcher not started"
            }),
            success_meta(BackendKind::Config, 1.0),
        )),
    }
}

pub fn prune_index_failures(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let failure_health = state.prune_index_failures()?;
    Ok((
        json!({
            "pruned_missing_failures": failure_health.pruned_missing_failures,
            "index_failures": failure_health.recent_failures,
            "index_failures_total": failure_health.total_failures,
            "stale_index_failures": failure_health.stale_failures,
            "persistent_index_failures": failure_health.persistent_failures,
            "recent_failure_window_seconds": failure_health.recent_window_seconds,
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn get_capabilities(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = arguments.get("file_path").and_then(|v| v.as_str());

    // Determine language from file path if provided
    let language = file_path
        .and_then(|fp| {
            std::path::Path::new(fp)
                .extension()
                .and_then(|e| e.to_str())
        })
        .map(|ext| ext.to_ascii_lowercase());

    // Check LSP availability
    let lsp_attached = file_path
        .and_then(crate::tools::default_lsp_command_for_path)
        .map(|cmd| {
            std::process::Command::new("which")
                .arg(&cmd)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        })
        .unwrap_or(false);

    // Check embeddings
    #[cfg(feature = "semantic")]
    let embeddings_loaded = state.embedding.get().map(|e| e.is_some()).unwrap_or(false);
    #[cfg(not(feature = "semantic"))]
    let embeddings_loaded = false;

    #[cfg(feature = "semantic")]
    let configured_embedding_model = codelens_core::configured_embedding_model_name();
    #[cfg(not(feature = "semantic"))]
    let configured_embedding_model = String::from("not_compiled");

    #[cfg(feature = "semantic")]
    let embedding_index_info = state
        .embedding
        .get()
        .and_then(|engine| engine.as_ref().map(|engine| engine.index_info()))
        .or_else(|| {
            codelens_core::EmbeddingEngine::inspect_existing_index(&state.project())
                .ok()
                .flatten()
        });
    #[cfg(not(feature = "semantic"))]
    let embedding_index_info: Option<codelens_core::EmbeddingIndexInfo> = None;

    // Check index freshness
    let index_stats = state.symbol_index().stats().ok();
    let index_fresh = index_stats
        .as_ref()
        .map(|s| s.stale_files == 0 && s.indexed_files > 0)
        .unwrap_or(false);

    // Build available/unavailable features
    let mut available = vec![
        "symbols",
        "imports",
        "calls",
        "rename",
        "search",
        "blast_radius",
        "dead_code",
    ];
    let mut unavailable: Vec<serde_json::Value> = Vec::new();

    if lsp_attached {
        available.extend_from_slice(&[
            "type_hierarchy",
            "diagnostics",
            "workspace_symbols",
            "rename_plan",
        ]);
    } else {
        unavailable
            .push(json!({"feature": "type_hierarchy_lsp", "reason": "no LSP server attached"}));
        unavailable.push(json!({"feature": "diagnostics", "reason": "no LSP server attached"}));
        // Native type hierarchy is still available
        available.push("type_hierarchy_native");
    }

    if embeddings_loaded {
        available.push("semantic_search");
    } else {
        unavailable.push(json!({"feature": "semantic_search", "reason": "embeddings not loaded — call index_embeddings first"}));
    }

    if !index_fresh {
        unavailable.push(json!({"feature": "cached_queries", "reason": "index may be stale — call refresh_symbol_index"}));
    }

    Ok((
        json!({
            "language": language,
            "lsp_attached": lsp_attached,
            "embeddings_loaded": embeddings_loaded,
            "embedding_model": configured_embedding_model,
            "embedding_indexed": embedding_index_info.as_ref().map(|info| info.indexed_symbols > 0).unwrap_or(false),
            "embedding_indexed_symbols": embedding_index_info.as_ref().map(|info| info.indexed_symbols).unwrap_or(0),
            "index_fresh": index_fresh,
            "indexed_files": index_stats.as_ref().map(|s| s.indexed_files).unwrap_or(0),
            "available": available,
            "unavailable": unavailable,
        }),
        success_meta(BackendKind::Config, 0.95),
    ))
}

pub fn get_tool_metrics(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let snapshot = state.metrics().snapshot();
    let surfaces = state.metrics().surface_snapshot();
    let per_tool: Vec<serde_json::Value> = snapshot
        .into_iter()
        .map(|(name, m)| {
            json!({
                "tool": name,
                "calls": m.call_count,
                "success_count": m.success_count,
                "total_ms": m.total_ms,
                "max_ms": m.max_ms,
                "total_tokens": m.total_tokens,
                "avg_output_tokens": if m.call_count > 0 {
                    m.total_tokens / m.call_count as usize
                } else { 0 },
                "p95_latency_ms": crate::telemetry::percentile_95(&m.latency_samples),
                "success_rate": if m.call_count > 0 {
                    m.success_count as f64 / m.call_count as f64
                } else { 0.0 },
                "error_rate": if m.call_count > 0 {
                    m.error_count as f64 / m.call_count as f64
                } else { 0.0 },
                "errors": m.error_count,
                "last_called": m.last_called_at,
            })
        })
        .collect();
    let count = per_tool.len();
    let per_surface = surfaces
        .into_iter()
        .map(|(surface, metrics)| {
            json!({
                "surface": surface,
                "calls": metrics.call_count,
                "success_count": metrics.success_count,
                "total_ms": metrics.total_ms,
                "total_tokens": metrics.total_tokens,
                "errors": metrics.error_count,
                "avg_tokens_per_call": if metrics.call_count > 0 {
                    metrics.total_tokens / metrics.call_count as usize
                } else { 0 },
                "p95_latency_ms": crate::telemetry::percentile_95(&metrics.latency_samples),
                "surface_token_efficiency": if metrics.success_count > 0 {
                    metrics.total_tokens as f64 / metrics.success_count as f64
                } else { 0.0 }
            })
        })
        .collect::<Vec<_>>();
    let metrics_payload = build_session_metrics_payload(state);
    Ok((
        json!({
            "tools": per_tool.clone(),
            "per_tool": per_tool,
            "count": count,
            "surfaces": per_surface.clone(),
            "per_surface": per_surface,
            "session": metrics_payload.session,
            "derived_kpis": metrics_payload.derived_kpis
        }),
        success_meta(BackendKind::Telemetry, 1.0),
    ))
}

/// Export session telemetry as markdown — replaces collect-session.sh + Python.
pub fn export_session_markdown(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let session_name = arguments
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("session");
    let snapshot = state.metrics().snapshot();
    let session = state.metrics().session_snapshot();

    let total_calls = session.total_calls.max(1);
    let mut tools: Vec<_> = snapshot.into_iter().collect();
    tools.sort_by(|a, b| b.1.call_count.cmp(&a.1.call_count));
    let count = tools.len();

    let mut md = String::with_capacity(2048);
    md.push_str(&format!("# Session Telemetry: {session_name}\n\n"));
    md.push_str("## Summary\n\n| Metric | Value |\n|---|---|\n");
    md.push_str(&format!("| Total calls | {} |\n", total_calls));
    md.push_str(&format!("| Total time | {}ms |\n", session.total_ms));
    md.push_str(&format!(
        "| Avg per call | {}ms |\n",
        if total_calls > 0 {
            session.total_ms / total_calls
        } else {
            0
        }
    ));
    md.push_str(&format!("| Total tokens | {} |\n", session.total_tokens));
    md.push_str(&format!("| Errors | {} |\n", session.error_count));
    md.push_str(&format!(
        "| Analysis summary reads | {} |\n",
        session.analysis_summary_reads
    ));
    md.push_str(&format!(
        "| Analysis section reads | {} |\n",
        session.analysis_section_reads
    ));
    md.push_str(&format!("| Unique tools | {count} |\n\n"));

    md.push_str("## Tool Usage\n\n| Tool | Calls | Total(ms) | Avg(ms) | Max(ms) | Err |\n|---|---|---|---|---|---|\n");
    for (name, m) in &tools {
        let avg = if m.call_count > 0 {
            m.total_ms as f64 / m.call_count as f64
        } else {
            0.0
        };
        md.push_str(&format!(
            "| {} | {} | {} | {:.1} | {} | {} |\n",
            name, m.call_count, m.total_ms, avg, m.max_ms, m.error_count
        ));
    }

    md.push_str("\n## Distribution\n\n```\n");
    for (name, m) in tools.iter().take(5) {
        let pct = m.call_count as f64 / total_calls as f64 * 100.0;
        let bar = "#".repeat((pct / 2.0) as usize);
        md.push_str(&format!(
            "  {:<30} {:3} ({:5.1}%) {}\n",
            name, m.call_count, pct, bar
        ));
    }
    md.push_str("```\n\n");
    md.push_str(&format!(
        "Tokens/call: {}\n",
        if total_calls > 0 {
            session.total_tokens / total_calls as usize
        } else {
            0
        }
    ));

    Ok((
        json!({
            "markdown": md,
            "session_name": session_name,
            "tool_count": count,
            "total_calls": total_calls,
        }),
        success_meta(BackendKind::Telemetry, 1.0),
    ))
}

pub fn set_preset(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let preset_str = arguments
        .get("preset")
        .and_then(|v| v.as_str())
        .unwrap_or("balanced");
    let new_preset = ToolPreset::from_str(preset_str);
    let old_surface = state.surface().as_label().to_owned();
    state.set_surface(ToolSurface::Preset(new_preset));

    // Auto-set token budget per preset, or accept explicit override
    let budget = arguments
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(default_budget_for_preset(new_preset));
    state.set_token_budget(budget);

    Ok((
        json!({
            "status": "ok",
            "previous_surface": old_surface,
            "current_preset": format!("{new_preset:?}"),
            "active_surface": ToolSurface::Preset(new_preset).as_label(),
            "token_budget": budget,
            "note": "Preset changed. Next tools/list call will reflect the new tool set."
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn set_profile(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let profile_str = arguments
        .get("profile")
        .and_then(|v| v.as_str())
        .unwrap_or("planner-readonly");
    let profile = ToolProfile::from_str(profile_str).ok_or_else(|| {
        crate::error::CodeLensError::Validation(format!("unknown profile `{profile_str}`"))
    })?;
    let old_surface = state.surface().as_label().to_owned();
    state.set_surface(ToolSurface::Profile(profile));
    let budget = arguments
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(default_budget_for_profile(profile));
    state.set_token_budget(budget);

    Ok((
        json!({
            "status": "ok",
            "previous_surface": old_surface,
            "current_profile": profile.as_str(),
            "active_surface": ToolSurface::Profile(profile).as_label(),
            "token_budget": budget,
            "note": "Profile changed. Next tools/list call will reflect the role-specific tool surface."
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}
