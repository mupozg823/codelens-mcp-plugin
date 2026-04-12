use crate::AppState;
use crate::protocol::BackendKind;
use crate::session_metrics_payload::build_session_metrics_payload;
use crate::tool_defs::{
    ToolPreset, ToolProfile, ToolSurface, default_budget_for_preset, default_budget_for_profile,
};
use crate::tool_runtime::{ToolResult, success_meta};
use serde_json::json;

#[cfg(feature = "semantic")]
use crate::tool_defs::is_tool_in_surface;

/// Return `true` when the given LSP binary is resolvable either via the
/// daemon's inherited `PATH` (the `which` fast-path) or via a set of
/// common macOS/Linux install locations (`/opt/homebrew/bin`,
/// `/usr/local/bin`, `~/.cargo/bin`, `~/.fnm/aliases/default/bin`,
/// `~/.nvm/versions/node/current/bin`) or via the user-supplied
/// `CODELENS_LSP_PATH_EXTRA` environment variable (`:`-separated list).
///
/// Motivation (Phase 4a, §capability-reporting): the MCP daemon on
/// launchd / systemd typically inherits a minimal `PATH`
/// (`/usr/bin:/bin:/usr/sbin:/sbin`), which excludes Homebrew,
/// cargo, and every Node version manager's install directory. Using
/// `which` alone was reporting `lsp_attached = false` on machines
/// where the LSP binary was installed and functional — a reporting
/// bug, not a real feature absence.
pub(crate) fn resolve_lsp_binary_exists(cmd: &str) -> bool {
    // Fast path: delegate to the daemon's PATH. This is the common case
    // when the daemon is started from an interactive shell or when PATH
    // has been explicitly propagated.
    if std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return true;
    }

    // Slow path: try a conservative allow-list of standard install
    // directories. Each entry is checked as `<dir>/<cmd>` with
    // `Path::exists()` — no execution, just a stat.
    let home = std::env::var("HOME").unwrap_or_default();
    let fallback_dirs: Vec<String> = vec![
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        format!("{home}/.cargo/bin"),
        format!("{home}/.fnm/aliases/default/bin"),
        format!("{home}/.nvm/versions/node/current/bin"),
    ];
    for dir in &fallback_dirs {
        if dir.is_empty() {
            continue;
        }
        if std::path::Path::new(dir).join(cmd).exists() {
            return true;
        }
    }

    // User escape hatch: colon-separated extra directories. Applied
    // after the standard allow-list so users can point at an unusual
    // install location (monorepo-local LSP, custom prefix, etc.).
    if let Ok(extra) = std::env::var("CODELENS_LSP_PATH_EXTRA") {
        for dir in extra.split(':').filter(|p| !p.is_empty()) {
            if std::path::Path::new(dir).join(cmd).exists() {
                return true;
            }
        }
    }

    false
}

/// Four-way decomposition of why `semantic_search` might not be
/// currently runnable. Phase 4a, §capability-reporting: the previous
/// single reason string "embeddings not loaded — call
/// index_embeddings first" conflated four distinct root causes, the
/// only one of which the user could actually act on was
/// `index_missing`. This enum keeps them separate so the caller can
/// suggest the right remediation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SemanticSearchStatus {
    /// The `semantic_search` handler is reachable, either because the
    /// engine is already loaded in memory or because an on-disk index
    /// exists and the engine will be lazy-initialized on first call.
    #[cfg(feature = "semantic")]
    Available,
    /// The bundled CodeSearchNet ONNX model file is missing or
    /// corrupt. User remediation: reinstall with a binary that ships
    /// the model, or set `CODELENS_MODEL_DIR`.
    #[cfg(feature = "semantic")]
    ModelAssetsUnavailable,
    /// The active tool surface (preset or profile) does not include
    /// `semantic_search`. User remediation: switch profile via
    /// `set_profile` / `set_preset`, or use a client that activates a
    /// richer surface.
    #[cfg(feature = "semantic")]
    NotInActiveSurface,
    /// The on-disk symbol index has no embedding rows yet. User
    /// remediation: call `index_embeddings` to build the index.
    #[cfg(feature = "semantic")]
    IndexMissing,
    /// The binary was built without the `semantic` cargo feature.
    /// User remediation: rebuild with `cargo build --features semantic`.
    ///
    /// Only constructed in the `#[cfg(not(feature = "semantic"))]`
    /// branch of `determine_semantic_search_status`. The default
    /// feature set for this crate enables `semantic`, so under a
    /// normal build this variant is unreachable — `#[allow(dead_code)]`
    /// silences the warning without dropping the variant, which we
    /// still want available for no-feature builds and for
    /// `semantic_search_status_reason_strings_are_distinct` to pin
    /// its reason text.
    #[allow(dead_code)]
    FeatureDisabled,
}

impl SemanticSearchStatus {
    pub(crate) fn reason_str(&self) -> Option<&'static str> {
        match self {
            #[cfg(feature = "semantic")]
            Self::Available => None,
            #[cfg(feature = "semantic")]
            Self::ModelAssetsUnavailable => Some(
                "model assets unavailable — reinstall with bundled model or set CODELENS_MODEL_DIR",
            ),
            #[cfg(feature = "semantic")]
            Self::NotInActiveSurface => Some(
                "not in active surface — call set_profile/set_preset to include semantic_search",
            ),
            #[cfg(feature = "semantic")]
            Self::IndexMissing => {
                Some("index missing — call index_embeddings to build the embedding index")
            }
            Self::FeatureDisabled => {
                Some("feature disabled — rebuild with `cargo build --features semantic`")
            }
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        #[cfg(feature = "semantic")]
        {
            matches!(self, Self::Available)
        }
        #[cfg(not(feature = "semantic"))]
        {
            false
        }
    }
}

/// Compute the current `SemanticSearchStatus` from three observations:
///   1. whether the binary was built with the `semantic` feature,
///   2. whether the CodeSearchNet model assets are on disk,
///   3. whether `semantic_search` is in the active tool surface,
///   4. whether the on-disk symbol-index contains embedding rows.
///
/// The precedence order is deliberately "fix the easiest thing first":
/// feature → model assets → surface → index. A user hitting
/// `FeatureDisabled` must rebuild; a user hitting `IndexMissing` just
/// has to run one tool call.
///
/// **Important (§capability-reporting AC3)**: when the engine is not
/// yet loaded in memory but the on-disk index exists and the surface
/// includes `semantic_search`, the status is `Available` — the actual
/// handler code path calls `state.embedding_engine()` which
/// lazy-initializes the engine under a write lock. Reporting
/// "engine not loaded yet" would be a misleading telemetry-vs-runtime
/// mismatch.
#[cfg(feature = "semantic")]
pub(crate) fn determine_semantic_search_status(
    state: &AppState,
    surface: ToolSurface,
) -> SemanticSearchStatus {
    if !codelens_engine::embedding_model_assets_available() {
        return SemanticSearchStatus::ModelAssetsUnavailable;
    }
    if !is_tool_in_surface("semantic_search", surface) {
        return SemanticSearchStatus::NotInActiveSurface;
    }
    // Check on-disk index status without loading the engine. If the
    // engine is already loaded, `index_info().indexed_symbols` is the
    // authoritative count; otherwise fall back to the on-disk
    // `inspect_existing_index` probe which opens the SQLite file read-only.
    let indexed_count = {
        let guard = state.embedding_ref();
        match guard.as_ref() {
            Some(engine) => engine.index_info().indexed_symbols,
            None => codelens_engine::EmbeddingEngine::inspect_existing_index(&state.project())
                .ok()
                .flatten()
                .map(|info| info.indexed_symbols)
                .unwrap_or(0),
        }
    };
    if indexed_count == 0 {
        return SemanticSearchStatus::IndexMissing;
    }
    SemanticSearchStatus::Available
}

#[cfg(not(feature = "semantic"))]
pub(crate) fn determine_semantic_search_status(
    _state: &AppState,
    _surface: ToolSurface,
) -> SemanticSearchStatus {
    SemanticSearchStatus::FeatureDisabled
}

pub fn get_watch_status(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let failure_health = state.watcher_failure_health();
    match state.watcher_stats() {
        Some(mut stats) => {
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

    // Check LSP availability via the Phase 4a PATH-tolerant helper.
    // The old code used `which` alone, which depends on the MCP
    // daemon's inherited PATH — typically `/usr/bin:/bin:/usr/sbin:/sbin`
    // on launchd, excluding Homebrew / cargo / Node version managers.
    // The new helper falls through to standard install locations
    // (§capability-reporting AC1).
    let lsp_attached = file_path
        .and_then(crate::tools::default_lsp_command_for_path)
        .map(|cmd| resolve_lsp_binary_exists(&cmd))
        .unwrap_or(false);

    // Phase 4a: `embeddings_loaded` is retained for backwards
    // compatibility — it answers "is the engine currently pinned in
    // memory?" not "can I call semantic_search right now?". The
    // actual runtime-capability question is answered by
    // `semantic_status` below, which decomposes four root causes.
    #[cfg(feature = "semantic")]
    let embeddings_loaded = state.embedding_ref().is_some();
    #[cfg(not(feature = "semantic"))]
    let embeddings_loaded = false;

    // Phase 4a §capability-reporting AC2/AC3: decompose the single
    // "semantic_search unavailable" reason into four distinct causes.
    // The decision here is independent of `embeddings_loaded` — a
    // project with an on-disk index but a cold engine is
    // **available**, because the `semantic_search` handler in
    // `dispatch.rs` calls `state.embedding_engine()` which
    // lazy-initializes the engine on first use. Reporting
    // "engine not loaded yet" would be a telemetry-vs-runtime
    // mismatch.
    let active_surface = *state.surface();
    let semantic_status = determine_semantic_search_status(state, active_surface);

    let configured_embedding_model = codelens_engine::configured_embedding_model_name();
    #[cfg(feature = "semantic")]
    let embedding_runtime = {
        let guard = state.embedding_ref();
        guard
            .as_ref()
            .map(|engine| engine.runtime_info().clone())
            .unwrap_or_else(codelens_engine::configured_embedding_runtime_info)
    };
    #[cfg(not(feature = "semantic"))]
    let embedding_runtime = codelens_engine::configured_embedding_runtime_info();

    #[cfg(feature = "semantic")]
    let embedding_index_info = {
        let guard = state.embedding_ref();
        guard
            .as_ref()
            .map(|engine| engine.index_info())
            .or_else(|| {
                codelens_engine::EmbeddingEngine::inspect_existing_index(&state.project())
                    .ok()
                    .flatten()
            })
    };
    #[cfg(not(feature = "semantic"))]
    let embedding_index_info =
        codelens_engine::EmbeddingEngine::inspect_existing_index(&state.project())
            .ok()
            .flatten();

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

    // Phase 4a: decide semantic_search availability from the
    // `semantic_status` decomposition, not from `embeddings_loaded`.
    // Lazy-init means a cold engine with a healthy on-disk index is
    // available even though `embedding_ref()` returns `None`.
    if semantic_status.is_available() {
        available.push("semantic_search");
    } else if let Some(reason) = semantic_status.reason_str() {
        unavailable.push(json!({
            "feature": "semantic_search",
            "reason": reason,
            "status": format!("{:?}", semantic_status),
        }));
    }

    if !index_fresh {
        unavailable.push(json!({"feature": "cached_queries", "reason": "index may be stale — call refresh_symbol_index"}));
    }

    // Phase 4b (§capability-reporting follow-up): surface build
    // metadata + daemon start time. Downstream tooling can compare
    // `binary_build_time` against `daemon_started_at` to detect the
    // exact Phase 4a failure mode ("daemon has been running since
    // before the binary was rebuilt"). We expose both as RFC 3339
    // UTC strings, plus the git SHA / version for human-readable
    // identification. A nested `binary_build_info` object keeps the
    // top-level JSON from growing unbounded while still letting
    // CLI scrapers jq-path directly.
    let binary_build_info = json!({
        "version": crate::build_info::BUILD_VERSION,
        "git_sha": crate::build_info::BUILD_GIT_SHA,
        "git_dirty": crate::build_info::build_git_dirty(),
        "build_time": crate::build_info::BUILD_TIME,
    });

    Ok((
        json!({
            "language": language,
            "lsp_attached": lsp_attached,
            "embeddings_loaded": embeddings_loaded,
            "embedding_model": configured_embedding_model,
            "embedding_runtime_preference": embedding_runtime.runtime_preference,
            "embedding_runtime_backend": embedding_runtime.backend,
            "embedding_threads": embedding_runtime.threads,
            "embedding_max_length": embedding_runtime.max_length,
            "embedding_coreml_model_format": embedding_runtime.coreml_model_format,
            "embedding_coreml_compute_units": embedding_runtime.coreml_compute_units,
            "embedding_coreml_static_input_shapes": embedding_runtime.coreml_static_input_shapes,
            "embedding_coreml_profile_compute_plan": embedding_runtime.coreml_profile_compute_plan,
            "embedding_coreml_specialization_strategy": embedding_runtime.coreml_specialization_strategy,
            "embedding_coreml_model_cache_dir": embedding_runtime.coreml_model_cache_dir,
            "embedding_runtime_fallback_reason": embedding_runtime.fallback_reason,
            "embedding_indexed": embedding_index_info.as_ref().map(|info| info.indexed_symbols > 0).unwrap_or(false),
            "embedding_indexed_symbols": embedding_index_info.as_ref().map(|info| info.indexed_symbols).unwrap_or(0),
            "index_fresh": index_fresh,
            "indexed_files": index_stats.as_ref().map(|s| s.indexed_files).unwrap_or(0),
            "available": available,
            "unavailable": unavailable,
            // Phase 4b: flat top-level fields for easy jq-scraping
            // plus the nested `binary_build_info` object for
            // grouped access.
            "binary_version": crate::build_info::BUILD_VERSION,
            "binary_git_sha": crate::build_info::BUILD_GIT_SHA,
            "binary_build_time": crate::build_info::BUILD_TIME,
            "daemon_started_at": state.daemon_started_at(),
            "binary_build_info": binary_build_info,
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
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let preset_str = arguments
        .get("preset")
        .and_then(|v| v.as_str())
        .unwrap_or("balanced");
    let new_preset = ToolPreset::from_str(preset_str);
    let old_surface = state.execution_surface(&session).as_label().to_owned();

    // Apply effort_level if provided
    if let Some(effort_str) = arguments.get("effort_level").and_then(|v| v.as_str()) {
        let level = match effort_str {
            "low" => crate::client_profile::EffortLevel::Low,
            "medium" => crate::client_profile::EffortLevel::Medium,
            _ => crate::client_profile::EffortLevel::High,
        };
        state.set_effort_level(level);
    }

    // Auto-set token budget per preset, or accept explicit override
    let budget = arguments
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(default_budget_for_preset(new_preset));
    #[cfg(feature = "http")]
    if !session.is_local() {
        state.set_session_surface_and_budget(
            &session.session_id,
            ToolSurface::Preset(new_preset),
            budget,
        );
    } else {
        state.set_surface(ToolSurface::Preset(new_preset));
        state.set_token_budget(budget);
    }
    #[cfg(not(feature = "http"))]
    {
        state.set_surface(ToolSurface::Preset(new_preset));
        state.set_token_budget(budget);
    }

    Ok((
        json!({
            "status": "ok",
            "previous_surface": old_surface,
            "current_preset": format!("{new_preset:?}"),
            "active_surface": ToolSurface::Preset(new_preset).as_label(),
            "token_budget": budget,
            "effort_level": state.effort_level().as_str(),
            "note": "Preset changed. Next tools/list call will reflect the new tool set."
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn set_profile(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let profile_str = arguments
        .get("profile")
        .and_then(|v| v.as_str())
        .unwrap_or("planner-readonly");
    let profile = ToolProfile::from_str(profile_str).ok_or_else(|| {
        crate::error::CodeLensError::Validation(format!("unknown profile `{profile_str}`"))
    })?;
    let old_surface = state.execution_surface(&session).as_label().to_owned();
    let budget = arguments
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(default_budget_for_profile(profile));
    #[cfg(feature = "http")]
    if !session.is_local() {
        state.set_session_surface_and_budget(
            &session.session_id,
            ToolSurface::Profile(profile),
            budget,
        );
    } else {
        state.set_surface(ToolSurface::Profile(profile));
        state.set_token_budget(budget);
    }
    #[cfg(not(feature = "http"))]
    {
        state.set_surface(ToolSurface::Profile(profile));
        state.set_token_budget(budget);
    }

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

// ── Phase 4a tests: capability reporting correctness ─────────────────

#[cfg(test)]
mod capability_reporting_tests {
    use super::*;

    /// Phase 4a AC1: the LSP fallback helper must resolve a binary
    /// that exists in a known install directory even when the daemon
    /// `PATH` does not include it. We synthesise this situation with
    /// the `CODELENS_LSP_PATH_EXTRA` env var pointing at a temp
    /// directory containing a dummy file named after the query.
    #[test]
    fn resolve_lsp_binary_exists_finds_via_env_override() {
        let tempdir = std::env::temp_dir().join(format!(
            "codelens-phase4a-lsp-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&tempdir).expect("mkdir tempdir");
        let fake_binary = tempdir.join("phase4a-fake-lsp-server");
        std::fs::write(&fake_binary, "").expect("touch fake binary");

        let previous = std::env::var("CODELENS_LSP_PATH_EXTRA").ok();
        // SAFETY: this test is synchronous and does not spawn worker
        // threads that race against env mutation.
        unsafe {
            std::env::set_var(
                "CODELENS_LSP_PATH_EXTRA",
                tempdir.to_string_lossy().as_ref(),
            );
        }

        // Fast path (`which`) will fail for this fabricated binary
        // name; the env-override fallback must catch it.
        assert!(
            resolve_lsp_binary_exists("phase4a-fake-lsp-server"),
            "env override fallback must resolve the dummy binary"
        );

        // Restore env
        unsafe {
            match previous {
                Some(v) => std::env::set_var("CODELENS_LSP_PATH_EXTRA", v),
                None => std::env::remove_var("CODELENS_LSP_PATH_EXTRA"),
            }
        }
        let _ = std::fs::remove_file(&fake_binary);
        let _ = std::fs::remove_dir(&tempdir);
    }

    /// Phase 4a AC1 negative: unknown binaries should still return
    /// false so we don't produce false positives in the capability
    /// report.
    #[test]
    fn resolve_lsp_binary_exists_returns_false_for_unknown_binary() {
        let unique = format!(
            "phase4a-definitely-not-installed-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        assert!(
            !resolve_lsp_binary_exists(&unique),
            "helper must not return true for a nonexistent binary"
        );
    }

    /// Phase 4a AC2/AC4: the `SemanticSearchStatus::reason_str`
    /// mapping must emit a distinct remediation message for each
    /// non-available variant, and `None` for `Available`.
    #[cfg(feature = "semantic")]
    #[test]
    fn semantic_search_status_reason_strings_are_distinct() {
        assert_eq!(SemanticSearchStatus::Available.reason_str(), None);
        let reasons = [
            SemanticSearchStatus::ModelAssetsUnavailable
                .reason_str()
                .unwrap(),
            SemanticSearchStatus::NotInActiveSurface
                .reason_str()
                .unwrap(),
            SemanticSearchStatus::IndexMissing.reason_str().unwrap(),
            SemanticSearchStatus::FeatureDisabled.reason_str().unwrap(),
        ];
        // All four distinct, all four mention an actionable remediation
        for (i, r) in reasons.iter().enumerate() {
            for (j, s) in reasons.iter().enumerate() {
                if i != j {
                    assert_ne!(
                        r, s,
                        "SemanticSearchStatus reasons at indices {i} and {j} must be distinct"
                    );
                }
            }
            assert!(
                !r.is_empty(),
                "SemanticSearchStatus reason {i} must be non-empty"
            );
        }
    }

    /// Phase 4a AC3: `is_available` returns true only for
    /// `Available`.
    #[cfg(feature = "semantic")]
    #[test]
    fn semantic_search_status_is_available_only_for_available_variant() {
        assert!(SemanticSearchStatus::Available.is_available());
        assert!(!SemanticSearchStatus::ModelAssetsUnavailable.is_available());
        assert!(!SemanticSearchStatus::NotInActiveSurface.is_available());
        assert!(!SemanticSearchStatus::IndexMissing.is_available());
        assert!(!SemanticSearchStatus::FeatureDisabled.is_available());
    }

    /// Phase 4a AC4: both Codex profiles must now expose
    /// `semantic_search` and `index_embeddings`. This guards against
    /// accidental removal in future preset edits.
    #[cfg(feature = "semantic")]
    #[test]
    fn planner_readonly_and_builder_minimal_expose_semantic_search() {
        use crate::tool_defs::{ToolProfile, ToolSurface, is_tool_in_surface};

        for profile in [ToolProfile::PlannerReadonly, ToolProfile::BuilderMinimal] {
            let surface = ToolSurface::Profile(profile);
            assert!(
                is_tool_in_surface("semantic_search", surface),
                "{profile:?} must expose semantic_search (Phase 4a §capability-reporting AC4)"
            );
            assert!(
                is_tool_in_surface("index_embeddings", surface),
                "{profile:?} must expose index_embeddings (Phase 4a §capability-reporting AC4)"
            );
        }
    }

    /// Phase 4b AC5: the compile-time `build_info` constants must
    /// be populated (non-empty) so `get_capabilities` can report
    /// meaningful values. A `"unknown"` git SHA is acceptable
    /// (e.g. `cargo publish` outside a git checkout), but an empty
    /// string would indicate the build script did not run.
    #[test]
    fn build_info_constants_are_populated() {
        assert!(
            !crate::build_info::BUILD_VERSION.is_empty(),
            "BUILD_VERSION must match CARGO_PKG_VERSION and be non-empty"
        );
        assert!(
            !crate::build_info::BUILD_GIT_SHA.is_empty(),
            "BUILD_GIT_SHA must be non-empty (at minimum 'unknown')"
        );
        assert!(
            !crate::build_info::BUILD_TIME.is_empty(),
            "BUILD_TIME must be non-empty RFC 3339 UTC"
        );
        // BUILD_TIME shape: YYYY-MM-DDTHH:MM:SSZ, 20 chars
        assert_eq!(
            crate::build_info::BUILD_TIME.len(),
            20,
            "BUILD_TIME should be exactly 20 chars (RFC 3339 UTC)"
        );
        assert!(
            crate::build_info::BUILD_TIME.ends_with('Z'),
            "BUILD_TIME should end with Z (UTC marker)"
        );
        // BUILD_GIT_DIRTY parses to bool without panicking
        let _ = crate::build_info::build_git_dirty();
    }
}
