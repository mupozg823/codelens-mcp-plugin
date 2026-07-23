use super::diagnostics::DiagnosticsGuidance;
#[cfg(test)]
use super::health::build_health_summary;
pub(crate) use super::health::collect_runtime_health_snapshot;
use super::probe_status::{model_status_for_response, scip_status_for_response};
#[cfg(test)]
use super::semantic::SemanticSearchStatus;
use crate::AppState;
use crate::protocol::BackendKind;
use crate::tool_runtime::{ToolResult, success_meta};
#[cfg(feature = "scip-backend")]
use crate::tools::scip_health::{detect_scip_generator_warnings, scip_generator_warnings_payload};
use serde_json::json;

/// Response detail level. The default `full` preserves the historical
/// 38-field shape. `compact` returns only the 12 core fields LLMs
/// actually consume on a startup probe and trims response token cost
/// from ~5K → ~1K (budget 19% → ~4%). Clients can re-call with
/// `detail=full` when they need the runtime introspection extras
/// (CoreML compute units, SCIP symbol counts, embedding runtime
/// preferences, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapabilitiesDetail {
    Compact,
    Full,
}

impl CapabilitiesDetail {
    fn from_value(arguments: &serde_json::Value) -> Self {
        match arguments
            .get("detail")
            .and_then(|v| v.as_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("compact") => Self::Compact,
            _ => Self::Full,
        }
    }
}

pub fn get_capabilities(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let detail = CapabilitiesDetail::from_value(arguments);
    let file_path = arguments.get("file_path").and_then(|v| v.as_str());

    // Determine language from file path if provided
    let language = file_path
        .and_then(|fp| {
            std::path::Path::new(fp)
                .extension()
                .and_then(|e| e.to_str())
        })
        .map(|ext| ext.to_ascii_lowercase());

    let lsp_pool = state.lsp_pool();
    let diagnostics_guidance = DiagnosticsGuidance::for_file(file_path, |command| {
        lsp_pool.trusted_lsp_binary(command).is_some()
    });
    let lsp_attached = diagnostics_guidance.is_available();

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
    let runtime_health = collect_runtime_health_snapshot(state, active_surface);
    let semantic_search_guidance = runtime_health.semantic_status.guidance_payload();

    #[cfg(feature = "semantic")]
    let configured_embedding_model = codelens_engine::configured_embedding_model_name();
    #[cfg(not(feature = "semantic"))]
    let configured_embedding_model = "disabled".to_owned();

    #[cfg(feature = "semantic")]
    let embedding_runtime = {
        let guard = state.embedding_ref();
        guard
            .as_ref()
            .map(|engine| engine.runtime_info().clone())
            .unwrap_or_else(codelens_engine::configured_embedding_runtime_info)
    };
    #[cfg(not(feature = "semantic"))]
    let embedding_runtime = codelens_engine::EmbeddingRuntimeInfo {
        runtime_preference: "disabled".to_owned(),
        backend: "none".to_owned(),
        threads: 0,
        max_length: 0,
        coreml_model_format: None,
        coreml_compute_units: None,
        coreml_static_input_shapes: None,
        coreml_profile_compute_plan: None,
        coreml_specialization_strategy: None,
        coreml_model_cache_dir: None,
        fallback_reason: Some("semantic feature not compiled".to_owned()),
    };

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
    let embedding_index_info: Option<codelens_engine::EmbeddingIndexInfo> = None;

    // Check index freshness
    let index_fresh = runtime_health.index_fresh();

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
        unavailable.push(diagnostics_guidance.unavailable_payload("type_hierarchy_lsp"));
        unavailable.push(diagnostics_guidance.unavailable_payload("diagnostics"));
        // Native type hierarchy is still available
        available.push("type_hierarchy_native");
    }

    // Phase 4a: decide semantic_search availability from the
    // `semantic_status` decomposition, not from `embeddings_loaded`.
    // Lazy-init means a cold engine with a healthy on-disk index is
    // available even though `embedding_ref()` returns `None`.
    if runtime_health.semantic_status.is_available() {
        available.push("semantic_search");
    } else if let Some(reason) = runtime_health.semantic_status.reason_str() {
        unavailable.push(json!({
            "feature": "semantic_search",
            "reason": reason,
            "status": runtime_health.semantic_status.status_key(),
            "reason_code": runtime_health.semantic_status.reason_code(),
            "recommended_action": runtime_health.semantic_status.recommended_action(),
            "action_target": runtime_health.semantic_status.action_target(),
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
    let semantic_search_status = runtime_health.semantic_status.status_key();
    let indexed_files = runtime_health.indexed_files();
    let supported_files = runtime_health.supported_files();
    let stale_files = runtime_health.stale_files();
    let health_summary = runtime_health.health_summary.clone();
    let daemon_binary_drift = runtime_health.daemon_binary_drift.clone();

    // Intelligence sources: report which backends are active.
    let mut intelligence_sources = vec!["tree_sitter"];
    if lsp_attached {
        intelligence_sources.push("lsp");
    }
    if runtime_health.semantic_status.is_available() {
        intelligence_sources.push("semantic");
    }
    // SCIP: check for index.scip in standard locations
    let project_root = state.project();
    let scip_available = project_root.as_path().join("index.scip").exists()
        || project_root.as_path().join(".scip/index.scip").exists()
        || project_root.as_path().join(".codelens/index.scip").exists();
    #[cfg(feature = "scip-backend")]
    let mut scip_file_count: Option<usize> = None;
    #[cfg(not(feature = "scip-backend"))]
    let scip_file_count: Option<usize> = None;
    #[cfg(feature = "scip-backend")]
    let mut scip_symbol_count: Option<usize> = None;
    #[cfg(not(feature = "scip-backend"))]
    let scip_symbol_count: Option<usize> = None;
    // intelligence_sources reports backends the binary can ACTUALLY use.
    // A stray index.scip on disk does not count when the binary lacks
    // the scip-backend feature — claiming "scip" in that case would
    // mislead agents into routing through type-aware tools that fall
    // back to tree-sitter anyway.
    #[cfg(feature = "scip-backend")]
    if scip_available {
        intelligence_sources.push("scip");
        if let Some(backend) = state.scip() {
            scip_file_count = Some(backend.file_count());
            scip_symbol_count = Some(backend.symbol_count());
        }
    }

    // Tri-state SCIP discovery signal so an agent on the compact startup
    // probe knows whether type-aware get_callers/get_callees are wired,
    // available-but-uninitialized, or not compiled. Pre-slice-3 the
    // compact probe was silent on SCIP entirely — the get_callers SCIP
    // boost shipped in #105 was invisible to agents that didn't drill
    // into `detail=full`.
    let (scip_status, scip_setup_hint) =
        scip_status_for_response(scip_available, project_root.as_path());
    #[cfg(feature = "scip-backend")]
    let scip_generator_warnings = detect_scip_generator_warnings(project_root.as_path())
        .map(|warnings| scip_generator_warnings_payload(&warnings));
    #[cfg(not(feature = "scip-backend"))]
    let scip_generator_warnings: Option<serde_json::Value> = None;

    // P0-2 — explicit semantic model sidecar tri-state. Pre-this-PR
    // the only model signal in the compact response was the indirect
    // `semantic_search_status: "model_assets_unavailable"` enum
    // value, which an agent had to recognise from a longer enum
    // without learning what to do about it. The new field surfaces
    // a direct verdict + an actionable hint for the cargo-install
    // path, matching the slice-3 SCIP discovery pattern.
    let (model_status, model_setup_hint) = model_status_for_response();

    let embedding_indexed_bool = embedding_index_info
        .as_ref()
        .map(|info| info.indexed_symbols > 0)
        .unwrap_or(false);
    let embedding_indexed_symbols = embedding_index_info
        .as_ref()
        .map(|info| info.indexed_symbols)
        .unwrap_or(0);

    let payload = match detail {
        CapabilitiesDetail::Compact => {
            // Core fields LLMs consume on a startup probe. Nested
            // sub-objects (`diagnostics_guidance`, `health_summary`)
            // and runtime introspection (CoreML compute units, SCIP
            // file/symbol counts, embedding runtime preferences,
            // build_info) are only emitted when `detail=full`.
            // L1 slice 3 added `scip_status` (and an optional
            // `scip_setup_hint` when actionable) so an agent can route
            // to type-aware get_callers/get_callees without first
            // asking for the full payload.
            json!({
                "language": language,
                "lsp_attached": lsp_attached,
                "intelligence_sources": intelligence_sources,
                "semantic_search_status": semantic_search_status,
                "embedding_model": configured_embedding_model,
                "embedding_indexed": embedding_indexed_bool,
                "embedding_indexed_symbols": embedding_indexed_symbols,
                "index_fresh": index_fresh,
                "available": available,
                "unavailable": unavailable,
                "binary_version": crate::build_info::BUILD_VERSION,
                "scip_status": scip_status,
                "scip_setup_hint": scip_setup_hint,
                "model_status": model_status,
                "model_setup_hint": model_setup_hint,
                "detail_available": ["full"],
            })
        }
        CapabilitiesDetail::Full => json!({
            "language": language,
            "lsp_attached": lsp_attached,
            "intelligence_sources": intelligence_sources,
            "diagnostics_guidance": diagnostics_guidance.guidance_payload(),
            "embeddings_loaded": embeddings_loaded,
            "semantic_search_status": semantic_search_status,
            "semantic_search_guidance": semantic_search_guidance,
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
            "embedding_indexed": embedding_indexed_bool,
            "embedding_indexed_symbols": embedding_indexed_symbols,
            "index_fresh": index_fresh,
            "indexed_files": indexed_files,
            "supported_files": supported_files,
            "stale_files": stale_files,
            "health_summary": health_summary,
            "available": available,
            "unavailable": unavailable,
            // Phase 4b → P0-3 (#282): top-level `binary_git_sha` and
            // `binary_build_time` were duplicated against the nested
            // `binary_build_info` object and broke prompt-cache prefix
            // stability for hosts that absorbed this payload into their
            // system/tools prefix. Removed in favour of the nested
            // representation as the single source of truth. Volatile
            // identity now lives only inside `binary_build_info`.
            "binary_version": crate::build_info::BUILD_VERSION,
            "daemon_started_at": state.daemon_started_at(),
            "daemon_binary_drift": daemon_binary_drift,
            "project_runtime": state.project_runtime_health_payload(),
            "coordination_health": state.coordination_health_payload(),
            "binary_build_info": binary_build_info,
            "scip_available": scip_available,
            "scip_file_count": scip_file_count,
            "scip_symbol_count": scip_symbol_count,
            "model_status": model_status,
            "model_setup_hint": model_setup_hint,
            "scip_status": scip_status,
            "scip_setup_hint": scip_setup_hint,
            "scip_generator_warnings": scip_generator_warnings,
        }),
    };

    Ok((payload, success_meta(BackendKind::Config, 0.95)))
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
