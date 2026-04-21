use super::guidance::{DiagnosticsGuidance, SemanticSearchStatus};
use crate::AppState;
use crate::protocol::BackendKind;
use crate::tool_defs::ToolSurface;
use crate::tool_runtime::{ToolResult, success_meta};
use serde_json::{Value, json};

#[cfg(feature = "semantic")]
use crate::tool_defs::is_tool_in_surface;

#[cfg(feature = "semantic")]
fn embedding_index_info(state: &AppState) -> Option<codelens_engine::EmbeddingIndexInfo> {
    let guard = state.embedding_ref();
    guard
        .as_ref()
        .map(|engine| engine.index_info())
        .or_else(|| {
            codelens_engine::EmbeddingEngine::inspect_existing_index(&state.project())
                .ok()
                .flatten()
        })
}

#[cfg(not(feature = "semantic"))]
fn embedding_index_info(state: &AppState) -> Option<codelens_engine::EmbeddingIndexInfo> {
    codelens_engine::EmbeddingEngine::inspect_existing_index(&state.project())
        .ok()
        .flatten()
}

fn indexed_symbol_count(state: &AppState) -> usize {
    embedding_index_info(state)
        .as_ref()
        .map(|info| info.indexed_symbols)
        .unwrap_or(0)
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
    if indexed_symbol_count(state) == 0 {
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

pub(crate) fn build_health_summary(
    index_stats: Option<&codelens_engine::IndexStats>,
    semantic_status: &SemanticSearchStatus,
    daemon_binary_drift: &serde_json::Value,
) -> serde_json::Value {
    let indexed_files = index_stats.map(|s| s.indexed_files).unwrap_or(0);
    let supported_files = index_stats.map(|s| s.supported_files).unwrap_or(0);
    let stale_files = index_stats.map(|s| s.stale_files).unwrap_or(0);
    let mut warnings = Vec::new();

    let mut push_warning = |code: &str,
                            message: String,
                            recommended_action: Option<&str>,
                            action_target: Option<&str>| {
        warnings.push(json!({
            "code": code,
            "severity": "warn",
            "message": message,
            "recommended_action": recommended_action,
            "action_target": action_target,
        }));
    };

    if supported_files == 0 {
        push_warning(
            "no_supported_files",
            "no supported source files detected".to_string(),
            None,
            None,
        );
    }
    if indexed_files == 0 {
        push_warning(
            "empty_index",
            "symbol index is empty".to_string(),
            Some("refresh_symbol_index"),
            Some("symbol_index"),
        );
    }
    if supported_files > 0 && indexed_files < supported_files {
        push_warning(
            "partial_index_coverage",
            format!("index coverage incomplete ({indexed_files}/{supported_files})"),
            Some("refresh_symbol_index"),
            Some("symbol_index"),
        );
    }
    if stale_files > 0 {
        push_warning(
            "stale_index",
            format!("{stale_files} indexed files are stale"),
            Some("refresh_symbol_index"),
            Some("symbol_index"),
        );
    }

    #[cfg(feature = "semantic")]
    match semantic_status {
        SemanticSearchStatus::ModelAssetsUnavailable | SemanticSearchStatus::IndexMissing => {
            push_warning(
                semantic_status
                    .reason_code()
                    .unwrap_or("semantic_unavailable"),
                semantic_status
                    .reason_str()
                    .unwrap_or("semantic search unavailable")
                    .to_string(),
                semantic_status.recommended_action(),
                semantic_status.action_target(),
            );
        }
        _ => {}
    }

    #[cfg(not(feature = "semantic"))]
    if matches!(semantic_status, SemanticSearchStatus::FeatureDisabled) {
        push_warning(
            semantic_status
                .reason_code()
                .unwrap_or("semantic_feature_disabled"),
            semantic_status
                .reason_str()
                .unwrap_or("semantic feature disabled")
                .to_string(),
            semantic_status.recommended_action(),
            semantic_status.action_target(),
        );
    }

    if daemon_binary_drift
        .get("stale_daemon")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        push_warning(
            daemon_binary_drift
                .get("reason_code")
                .and_then(|v| v.as_str())
                .unwrap_or("stale_daemon"),
            daemon_binary_drift
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("daemon binary drift detected")
                .to_string(),
            daemon_binary_drift
                .get("recommended_action")
                .and_then(|v| v.as_str()),
            daemon_binary_drift
                .get("action_target")
                .and_then(|v| v.as_str()),
        );
    }

    json!({
        "status": if warnings.is_empty() { "ok" } else { "warn" },
        "warning_count": warnings.len(),
        "warnings": warnings,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeHealthSnapshot {
    pub(crate) index_stats: Option<codelens_engine::IndexStats>,
    pub(crate) semantic_status: SemanticSearchStatus,
    pub(crate) daemon_binary_drift: serde_json::Value,
    pub(crate) health_summary: serde_json::Value,
}

impl RuntimeHealthSnapshot {
    pub(crate) fn index_fresh(&self) -> bool {
        self.index_stats
            .as_ref()
            .map(|stats| stats.stale_files == 0 && stats.indexed_files > 0)
            .unwrap_or(false)
    }

    pub(crate) fn indexed_files(&self) -> usize {
        self.index_stats
            .as_ref()
            .map(|stats| stats.indexed_files)
            .unwrap_or(0)
    }

    pub(crate) fn supported_files(&self) -> usize {
        self.index_stats
            .as_ref()
            .map(|stats| stats.supported_files)
            .unwrap_or(0)
    }

    pub(crate) fn stale_files(&self) -> usize {
        self.index_stats
            .as_ref()
            .map(|stats| stats.stale_files)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CapabilitySnapshot {
    pub(crate) runtime_health: RuntimeHealthSnapshot,
    pub(crate) diagnostics_guidance: DiagnosticsGuidance,
    pub(crate) lsp_attached: bool,
    pub(crate) embeddings_loaded: bool,
    pub(crate) semantic_runtime_ready: bool,
    pub(crate) available: Vec<&'static str>,
    pub(crate) unavailable: Vec<Value>,
    pub(crate) intelligence_sources: Vec<&'static str>,
    pub(crate) scip_available: bool,
    pub(crate) scip_file_count: Option<usize>,
    pub(crate) scip_symbol_count: Option<usize>,
}

impl CapabilitySnapshot {
    pub(crate) fn semantic_search_status(&self) -> &'static str {
        self.runtime_health.semantic_status.status_key()
    }

    pub(crate) fn semantic_search_guidance(&self) -> Value {
        self.runtime_health.semantic_status.guidance_payload()
    }

    pub(crate) fn session_health_payload(&self) -> Value {
        json!({
            "semantic_search_status": self.semantic_search_status(),
            "semantic_runtime_ready": self.semantic_runtime_ready,
            "semantic_search_guidance": self.semantic_search_guidance(),
            "indexed_files": self.runtime_health.indexed_files(),
            "supported_files": self.runtime_health.supported_files(),
            "stale_files": self.runtime_health.stale_files(),
            "daemon_binary_drift": self.runtime_health.daemon_binary_drift.clone(),
            "health_summary": self.runtime_health.health_summary.clone(),
            "intelligence_sources": self.intelligence_sources.clone(),
        })
    }
}

pub(crate) fn collect_runtime_health_snapshot(
    state: &AppState,
    surface: ToolSurface,
) -> RuntimeHealthSnapshot {
    let index_stats = state.symbol_index().stats().ok();
    let semantic_status = determine_semantic_search_status(state, surface);
    let daemon_binary_drift =
        crate::build_info::daemon_binary_drift_payload(state.daemon_started_at());
    let health_summary =
        build_health_summary(index_stats.as_ref(), &semantic_status, &daemon_binary_drift);
    RuntimeHealthSnapshot {
        index_stats,
        semantic_status,
        daemon_binary_drift,
        health_summary,
    }
}

#[cfg(feature = "semantic")]
fn semantic_runtime_ready(state: &AppState) -> bool {
    codelens_engine::embedding_model_assets_available() && indexed_symbol_count(state) > 0
}

#[cfg(not(feature = "semantic"))]
fn semantic_runtime_ready(_state: &AppState) -> bool {
    false
}

pub(crate) fn collect_capability_snapshot(
    state: &AppState,
    surface: ToolSurface,
    file_path: Option<&str>,
) -> CapabilitySnapshot {
    let diagnostics_guidance = DiagnosticsGuidance::for_file(file_path);
    let lsp_attached = diagnostics_guidance.status.is_available();
    #[cfg(feature = "semantic")]
    let embeddings_loaded = state.embedding_ref().is_some();
    #[cfg(not(feature = "semantic"))]
    let embeddings_loaded = false;
    let runtime_health = collect_runtime_health_snapshot(state, surface);
    let semantic_runtime_ready = semantic_runtime_ready(state);

    let mut available = vec![
        "symbols",
        "imports",
        "calls",
        "rename",
        "search",
        "blast_radius",
        "dead_code",
    ];
    let mut unavailable = Vec::new();

    if lsp_attached {
        available.extend_from_slice(&[
            "type_hierarchy",
            "diagnostics",
            "workspace_symbols",
            "rename_plan",
        ]);
    } else {
        unavailable.push(diagnostics_guidance.unavailable_payload("type_hierarchy_lsp"));
        unavailable.push(diagnostics_guidance.unavailable_payload("diagnostics"));
        available.push("type_hierarchy_native");
    }

    if runtime_health.semantic_status.is_available() {
        available.push("semantic_search");
    } else {
        unavailable.push(
            runtime_health
                .semantic_status
                .unavailable_payload("semantic_search"),
        );
    }

    if !runtime_health.index_fresh() {
        unavailable.push(json!({
            "feature": "cached_queries",
            "reason": "index may be stale — call refresh_symbol_index"
        }));
    }

    let mut intelligence_sources = vec!["tree_sitter"];
    if lsp_attached {
        intelligence_sources.push("lsp");
    }
    if runtime_health.semantic_status.is_available() {
        intelligence_sources.push("semantic");
    }

    let project_root = state.project();
    let scip_available = project_root.as_path().join("index.scip").exists()
        || project_root.as_path().join(".scip/index.scip").exists()
        || project_root.as_path().join(".codelens/index.scip").exists();
    #[allow(unused_mut)]
    let mut scip_file_count: Option<usize> = None;
    #[allow(unused_mut)]
    let mut scip_symbol_count: Option<usize> = None;
    if scip_available {
        intelligence_sources.push("scip");
        #[cfg(feature = "scip-backend")]
        if let Some(backend) = state.scip() {
            scip_file_count = Some(backend.file_count());
            scip_symbol_count = Some(backend.symbol_count());
        }
    }

    CapabilitySnapshot {
        runtime_health,
        diagnostics_guidance,
        lsp_attached,
        embeddings_loaded,
        semantic_runtime_ready,
        available,
        unavailable,
        intelligence_sources,
        scip_available,
        scip_file_count,
        scip_symbol_count,
    }
}

pub fn get_capabilities(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = arguments.get("file_path").and_then(|v| v.as_str());

    let language = file_path
        .and_then(|fp| {
            std::path::Path::new(fp)
                .extension()
                .and_then(|e| e.to_str())
        })
        .map(|ext| ext.to_ascii_lowercase());

    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let active_surface = state.execution_surface(&session);
    let capability = collect_capability_snapshot(state, active_surface, file_path);

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

    let embedding_index_info = embedding_index_info(state);

    let binary_build_info = json!({
        "version": crate::build_info::BUILD_VERSION,
        "git_sha": crate::build_info::BUILD_GIT_SHA,
        "git_dirty": crate::build_info::build_git_dirty(),
        "build_time": crate::build_info::BUILD_TIME,
    });
    Ok((
        json!({
            "language": language,
            "lsp_attached": capability.lsp_attached,
            "coordination_mode": state.coordination_mode().as_str(),
            "coordination_enforcement": {
                "mode": state.coordination_mode().as_str(),
                "strict_enabled": matches!(state.coordination_mode(), crate::state::RuntimeCoordinationMode::Strict),
                "strict_path_coverage_required": matches!(state.coordination_mode(), crate::state::RuntimeCoordinationMode::Strict),
                "strict_claim_required": matches!(state.coordination_mode(), crate::state::RuntimeCoordinationMode::Strict),
                "strict_overlap_blocks_mutation": matches!(state.coordination_mode(), crate::state::RuntimeCoordinationMode::Strict),
                "strict_applies_to": "trusted_http_refactor_full_mutations",
            },
            "intelligence_sources": capability.intelligence_sources,
            "diagnostics_guidance": capability.diagnostics_guidance.guidance_payload(),
            "embeddings_loaded": capability.embeddings_loaded,
            "semantic_search_status": capability.semantic_search_status(),
            "semantic_runtime_ready": capability.semantic_runtime_ready,
            "semantic_search_guidance": capability.semantic_search_guidance(),
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
            "index_fresh": capability.runtime_health.index_fresh(),
            "indexed_files": capability.runtime_health.indexed_files(),
            "supported_files": capability.runtime_health.supported_files(),
            "stale_files": capability.runtime_health.stale_files(),
            "health_summary": capability.runtime_health.health_summary,
            "available": capability.available,
            "unavailable": capability.unavailable,
            "binary_version": crate::build_info::BUILD_VERSION,
            "binary_git_sha": crate::build_info::BUILD_GIT_SHA,
            "binary_build_time": crate::build_info::BUILD_TIME,
            "daemon_started_at": state.daemon_started_at(),
            "daemon_binary_drift": capability.runtime_health.daemon_binary_drift,
            "binary_build_info": binary_build_info,
            "scip_available": capability.scip_available,
            "scip_file_count": capability.scip_file_count,
            "scip_symbol_count": capability.scip_symbol_count,
        }),
        success_meta(BackendKind::Config, 0.95),
    ))
}
