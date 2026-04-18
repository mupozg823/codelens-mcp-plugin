use crate::protocol::BackendKind;
use crate::tool_defs::ToolSurface;
use crate::tool_runtime::{success_meta, ToolResult};
use crate::AppState;
use serde_json::json;

#[cfg(feature = "semantic")]
use crate::tool_defs::is_tool_in_surface;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiagnosticsStatus {
    Available,
    FilePathRequired,
    UnsupportedExtension,
    LspBinaryMissing,
}

impl DiagnosticsStatus {
    pub(crate) fn status_key(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::FilePathRequired => "file_path_required",
            Self::UnsupportedExtension => "unsupported_extension",
            Self::LspBinaryMissing => "lsp_binary_missing",
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DiagnosticsGuidance {
    status: DiagnosticsStatus,
    file_extension: Option<String>,
    language: Option<&'static str>,
    lsp_command: Option<&'static str>,
    server_name: Option<&'static str>,
    install_command: Option<&'static str>,
    package_manager: Option<&'static str>,
}

impl DiagnosticsGuidance {
    fn for_file(file_path: Option<&str>) -> Self {
        let extension = file_path.and_then(|path| {
            std::path::Path::new(path)
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_ascii_lowercase())
        });
        let recipe = extension
            .as_deref()
            .and_then(codelens_engine::get_lsp_recipe);

        let status = match (file_path, recipe) {
            (None, _) => DiagnosticsStatus::FilePathRequired,
            (Some(_), None) => DiagnosticsStatus::UnsupportedExtension,
            (Some(_), Some(recipe)) if !codelens_engine::lsp_binary_exists(recipe.binary_name) => {
                DiagnosticsStatus::LspBinaryMissing
            }
            (Some(_), Some(_)) => DiagnosticsStatus::Available,
        };

        Self {
            status,
            file_extension: extension,
            language: recipe.map(|recipe| recipe.language),
            lsp_command: recipe.map(|recipe| recipe.binary_name),
            server_name: recipe.map(|recipe| recipe.server_name),
            install_command: recipe.map(|recipe| recipe.install_command),
            package_manager: recipe.map(|recipe| recipe.package_manager),
        }
    }

    fn reason_str(&self) -> Option<&'static str> {
        match self.status {
            DiagnosticsStatus::Available => None,
            DiagnosticsStatus::FilePathRequired => Some(
                "file_path required — provide a concrete source file so CodeLens can select an LSP recipe",
            ),
            DiagnosticsStatus::UnsupportedExtension => Some(
                "unsupported extension — no default LSP recipe is registered for this file type",
            ),
            DiagnosticsStatus::LspBinaryMissing => Some(
                "LSP binary missing — install the configured server or provide an explicit command",
            ),
        }
    }

    fn reason_code(&self) -> Option<&'static str> {
        match self.status {
            DiagnosticsStatus::Available => None,
            DiagnosticsStatus::FilePathRequired => Some("diagnostics_file_path_required"),
            DiagnosticsStatus::UnsupportedExtension => Some("diagnostics_unsupported_extension"),
            DiagnosticsStatus::LspBinaryMissing => Some("diagnostics_lsp_binary_missing"),
        }
    }

    fn recommended_action(&self) -> Option<&'static str> {
        match self.status {
            DiagnosticsStatus::Available => None,
            DiagnosticsStatus::FilePathRequired => Some("provide_file_path"),
            DiagnosticsStatus::UnsupportedExtension => Some("pass_explicit_lsp_command"),
            DiagnosticsStatus::LspBinaryMissing => Some("install_lsp_server"),
        }
    }

    fn action_target(&self) -> Option<&'static str> {
        match self.status {
            DiagnosticsStatus::Available => None,
            DiagnosticsStatus::FilePathRequired => Some("file_path"),
            DiagnosticsStatus::UnsupportedExtension => Some("file_extension"),
            DiagnosticsStatus::LspBinaryMissing => Some("lsp_server"),
        }
    }

    fn guidance_payload(&self) -> serde_json::Value {
        json!({
            "status": self.status.status_key(),
            "available": self.status.is_available(),
            "reason": self.reason_str(),
            "reason_code": self.reason_code(),
            "recommended_action": self.recommended_action(),
            "action_target": self.action_target(),
            "file_extension": self.file_extension,
            "language": self.language,
            "lsp_command": self.lsp_command,
            "server_name": self.server_name,
            "install_command": self.install_command,
            "package_manager": self.package_manager,
        })
    }

    fn unavailable_payload(&self, feature: &str) -> serde_json::Value {
        json!({
            "feature": feature,
            "reason": self.reason_str().unwrap_or("diagnostics available"),
            "status": self.status.status_key(),
            "reason_code": self.reason_code(),
            "recommended_action": self.recommended_action(),
            "action_target": self.action_target(),
            "file_extension": self.file_extension,
            "language": self.language,
            "lsp_command": self.lsp_command,
            "server_name": self.server_name,
            "install_command": self.install_command,
            "package_manager": self.package_manager,
        })
    }
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
    pub(crate) fn status_key(&self) -> &'static str {
        match self {
            #[cfg(feature = "semantic")]
            Self::Available => "available",
            #[cfg(feature = "semantic")]
            Self::ModelAssetsUnavailable => "model_assets_unavailable",
            #[cfg(feature = "semantic")]
            Self::NotInActiveSurface => "not_in_active_surface",
            #[cfg(feature = "semantic")]
            Self::IndexMissing => "index_missing",
            Self::FeatureDisabled => "feature_disabled",
        }
    }

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

    pub(crate) fn reason_code(&self) -> Option<&'static str> {
        match self {
            #[cfg(feature = "semantic")]
            Self::Available => None,
            #[cfg(feature = "semantic")]
            Self::ModelAssetsUnavailable => Some("semantic_model_assets_unavailable"),
            #[cfg(feature = "semantic")]
            Self::NotInActiveSurface => Some("semantic_not_in_active_surface"),
            #[cfg(feature = "semantic")]
            Self::IndexMissing => Some("semantic_index_missing"),
            Self::FeatureDisabled => Some("semantic_feature_disabled"),
        }
    }

    pub(crate) fn recommended_action(&self) -> Option<&'static str> {
        match self {
            #[cfg(feature = "semantic")]
            Self::Available => None,
            #[cfg(feature = "semantic")]
            Self::ModelAssetsUnavailable => Some("configure_model_assets"),
            #[cfg(feature = "semantic")]
            Self::NotInActiveSurface => Some("switch_tool_surface"),
            #[cfg(feature = "semantic")]
            Self::IndexMissing => Some("run_index_embeddings"),
            Self::FeatureDisabled => Some("rebuild_with_semantic_feature"),
        }
    }

    pub(crate) fn action_target(&self) -> Option<&'static str> {
        match self {
            #[cfg(feature = "semantic")]
            Self::Available => None,
            #[cfg(feature = "semantic")]
            Self::ModelAssetsUnavailable => Some("model_assets"),
            #[cfg(feature = "semantic")]
            Self::NotInActiveSurface => Some("tool_surface"),
            #[cfg(feature = "semantic")]
            Self::IndexMissing => Some("embedding_index"),
            Self::FeatureDisabled => Some("binary"),
        }
    }

    pub(crate) fn guidance_payload(&self) -> serde_json::Value {
        json!({
            "status": self.status_key(),
            "available": self.is_available(),
            "reason": self.reason_str(),
            "reason_code": self.reason_code(),
            "recommended_action": self.recommended_action(),
            "action_target": self.action_target(),
        })
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

    let diagnostics_guidance = DiagnosticsGuidance::for_file(file_path);
    let lsp_attached = diagnostics_guidance.status.is_available();

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
    #[allow(unused_mut)]
    let mut scip_file_count: Option<usize> = None;
    #[allow(unused_mut)]
    let mut scip_symbol_count: Option<usize> = None;
    if scip_available {
        intelligence_sources.push("scip");
        // Report SCIP index stats if the backend is loaded.
        #[cfg(feature = "scip-backend")]
        if let Some(backend) = state.scip() {
            scip_file_count = Some(backend.file_count());
            scip_symbol_count = Some(backend.symbol_count());
        }
    }

    Ok((
        json!({
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
            "embedding_indexed": embedding_index_info.as_ref().map(|info| info.indexed_symbols > 0).unwrap_or(false),
            "embedding_indexed_symbols": embedding_index_info.as_ref().map(|info| info.indexed_symbols).unwrap_or(0),
            "index_fresh": index_fresh,
            "indexed_files": indexed_files,
            "supported_files": supported_files,
            "stale_files": stale_files,
            "health_summary": health_summary,
            "available": available,
            "unavailable": unavailable,
            // Phase 4b: flat top-level fields for easy jq-scraping
            // plus the nested `binary_build_info` object for
            // grouped access.
            "binary_version": crate::build_info::BUILD_VERSION,
            "binary_git_sha": crate::build_info::BUILD_GIT_SHA,
            "binary_build_time": crate::build_info::BUILD_TIME,
            "daemon_started_at": state.daemon_started_at(),
            "daemon_binary_drift": daemon_binary_drift,
            "binary_build_info": binary_build_info,
            "scip_available": scip_available,
            "scip_file_count": scip_file_count,
            "scip_symbol_count": scip_symbol_count,
        }),
        success_meta(BackendKind::Config, 0.95),
    ))
}
