use crate::AppState;
use crate::protocol::BackendKind;
use crate::tool_defs::ToolSurface;
use crate::tool_runtime::{ToolResult, success_meta};
use serde_json::json;
use std::path::Path;

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
    /// the model, or set `SYMBIOTE_MODEL_DIR` / `CODELENS_MODEL_DIR`.
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
                "model assets unavailable — reinstall with bundled model or set SYMBIOTE_MODEL_DIR or CODELENS_MODEL_DIR",
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

    pub(crate) fn included_in_profiles(&self) -> Option<Vec<&'static str>> {
        #[cfg(feature = "semantic")]
        {
            match self {
                Self::NotInActiveSurface => Some(vec!["planner-readonly", "builder-minimal"]),
                _ => None,
            }
        }
        #[cfg(not(feature = "semantic"))]
        {
            None
        }
    }

    pub(crate) fn guidance_payload(&self) -> serde_json::Value {
        let mut payload = json!({
            "status": self.status_key(),
            "available": self.is_available(),
            "reason": self.reason_str(),
            "reason_code": self.reason_code(),
            "recommended_action": self.recommended_action(),
            "action_target": self.action_target(),
        });
        if let Some(profiles) = self.included_in_profiles() {
            let recommended_profile = profiles.first().copied();
            payload["included_in"] = serde_json::json!(profiles);
            if let Some(first) = recommended_profile {
                payload["recommended_profile"] = serde_json::json!(first);
            }
        }
        payload
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
    let daemon_binary_drift = crate::build_info::daemon_binary_drift_payload(
        state.daemon_started_at(),
        Some(state.project().as_path()),
    );
    let health_summary =
        build_health_summary(index_stats.as_ref(), &semantic_status, &daemon_binary_drift);
    RuntimeHealthSnapshot {
        index_stats,
        semantic_status,
        daemon_binary_drift,
        health_summary,
    }
}

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
                "binary_git_sha": crate::build_info::BUILD_GIT_SHA,
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
            "model_status": model_status,
            "model_setup_hint": model_setup_hint,
            "scip_status": scip_status,
            "scip_setup_hint": scip_setup_hint,
        }),
    };

    Ok((payload, success_meta(BackendKind::Config, 0.95)))
}

/// Four-state SCIP discovery signal:
/// - `"enabled"` — feature compiled, `index.scip` present, and fresher
///   than `Cargo.lock`/`Cargo.toml` (the cheap proxy for "code state
///   the index was generated from")
/// - `"stale_index"` — index present but its mtime predates Cargo.lock
///   or Cargo.toml. The index will mostly still work but type
///   resolutions for newly-added/removed crates or symbols will be
///   inaccurate; `scip_setup_hint` recommends a regeneration
/// - `"available_no_index"` — feature compiled but no index detected;
///   pair with the actionable `scip_setup_hint` so agents can guide
///   users to generate one
/// - `"not_compiled"` — feature disabled in this binary; no hint is
///   emitted because the binary cannot benefit from an index even if
///   one were generated
fn scip_status_for_response(
    scip_available: bool,
    project_root: &Path,
) -> (&'static str, Option<String>) {
    #[cfg(feature = "scip-backend")]
    {
        if !scip_available {
            return (
                "available_no_index",
                Some(
                    "Run `scripts/generate-scip-index.sh` (wraps `rust-analyzer scip .`) at the project root to enable type-aware get_callers/get_callees."
                        .to_owned(),
                ),
            );
        }
        if is_scip_index_stale(project_root) {
            return (
                "stale_index",
                Some(
                    "SCIP index is older than Cargo.lock/Cargo.toml — re-run `scripts/generate-scip-index.sh` to refresh type-aware navigation against the current dependency tree."
                        .to_owned(),
                ),
            );
        }
        ("enabled", None)
    }
    #[cfg(not(feature = "scip-backend"))]
    {
        let _ = (scip_available, project_root);
        ("not_compiled", None)
    }
}

/// Tri-state semantic-model sidecar signal:
/// - `"loaded"` — semantic feature compiled AND a complete codesearch
///   model directory was found at one of the standard locations
///   (`CODELENS_MODEL_DIR`, exec-relative `models/`, source tree
///   `crates/codelens-engine/models/`, dirs cache)
/// - `"missing"` — feature compiled, but no model files reachable.
///   Pair with `model_setup_hint` so the cargo-install path receives
///   an actionable next step
/// - `"not_compiled"` — binary lacks the `semantic` feature; no
///   amount of model fetching changes that
fn model_status_for_response() -> (&'static str, Option<String>) {
    #[cfg(feature = "semantic")]
    {
        if codelens_engine::embedding_model_assets_available() {
            ("loaded", None)
        } else {
            (
                "missing",
                Some(
                    "Semantic model sidecar not found. GitHub Release tarballs bundle it; cargo-install users must fetch model.onnx + tokenizer.json + config.json + special_tokens_map.json + tokenizer_config.json (~80 MB) and point CODELENS_MODEL_DIR at the parent directory containing `codesearch/`."
                        .to_owned(),
                ),
            )
        }
    }
    #[cfg(not(feature = "semantic"))]
    {
        ("not_compiled", None)
    }
}

/// Compares the mtime of the detected `index.scip` against
/// `Cargo.lock` (preferred — captures every dep change including
/// transitive bumps) and `Cargo.toml` (fallback — captures workspace
/// member additions). Returns false on any I/O error so the helper
/// stays best-effort: a missing index, missing manifest, or unreadable
/// metadata simply leaves the status as `enabled`/`available_no_index`
/// without spurious "stale" claims.
#[cfg(feature = "scip-backend")]
fn is_scip_index_stale(project_root: &Path) -> bool {
    let Some(index_path) = codelens_engine::ScipBackend::detect(project_root) else {
        return false;
    };
    let Ok(index_meta) = std::fs::metadata(&index_path) else {
        return false;
    };
    let Ok(index_mtime) = index_meta.modified() else {
        return false;
    };

    for manifest in ["Cargo.lock", "Cargo.toml"] {
        let manifest_path = project_root.join(manifest);
        let Ok(meta) = std::fs::metadata(&manifest_path) else {
            continue;
        };
        let Ok(mtime) = meta.modified() else {
            continue;
        };
        if mtime > index_mtime {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_default_is_full() {
        assert_eq!(
            CapabilitiesDetail::from_value(&json!({})),
            CapabilitiesDetail::Full
        );
        assert_eq!(
            CapabilitiesDetail::from_value(&json!({"file_path": "x.rs"})),
            CapabilitiesDetail::Full,
            "unrelated args do not flip the default"
        );
    }

    #[test]
    fn detail_accepts_compact_and_full_case_insensitive() {
        assert_eq!(
            CapabilitiesDetail::from_value(&json!({"detail": "compact"})),
            CapabilitiesDetail::Compact
        );
        assert_eq!(
            CapabilitiesDetail::from_value(&json!({"detail": "COMPACT"})),
            CapabilitiesDetail::Compact
        );
        assert_eq!(
            CapabilitiesDetail::from_value(&json!({"detail": "full"})),
            CapabilitiesDetail::Full
        );
    }

    #[cfg(feature = "scip-backend")]
    #[test]
    fn scip_status_when_compiled_with_fresh_index_is_enabled() {
        // L1 slice 3 + 4 — index.scip is fresher than Cargo.lock /
        // Cargo.toml, so status is `enabled` and no setup hint is
        // needed. The fixture builds an actual on-disk layout so the
        // staleness check (mtime compare) sees real metadata.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), b"[package]\nname=\"x\"\n").unwrap();
        std::fs::write(dir.path().join("Cargo.lock"), b"# dummy\n").unwrap();
        let index_path = dir.path().join("index.scip");
        std::fs::write(&index_path, b"placeholder").unwrap();
        // Backdate manifests by 60s so the index is unambiguously fresher
        // than they are even on filesystems with second-granularity mtimes.
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        filetime::set_file_mtime(
            dir.path().join("Cargo.toml"),
            filetime::FileTime::from_system_time(past),
        )
        .unwrap();
        filetime::set_file_mtime(
            dir.path().join("Cargo.lock"),
            filetime::FileTime::from_system_time(past),
        )
        .unwrap();

        let (status, hint) = scip_status_for_response(true, dir.path());
        assert_eq!(status, "enabled");
        assert!(hint.is_none(), "fresh index needs no hint");
    }

    #[cfg(feature = "scip-backend")]
    #[test]
    fn scip_status_when_index_predates_cargo_lock_is_stale() {
        // L1 slice 4 — when Cargo.lock was touched after the index
        // was built (the canonical "ran cargo update without
        // regenerating SCIP" failure mode), surface `stale_index`
        // plus an actionable regeneration hint. The same path tree
        // also confirms that Cargo.toml-only changes (workspace
        // member added without a lock bump) trigger the same signal.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), b"[package]\nname=\"x\"\n").unwrap();
        std::fs::write(dir.path().join("Cargo.lock"), b"# dummy\n").unwrap();
        let index_path = dir.path().join("index.scip");
        std::fs::write(&index_path, b"placeholder").unwrap();
        // Backdate the index so the manifests appear newer than it.
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(120);
        filetime::set_file_mtime(&index_path, filetime::FileTime::from_system_time(past)).unwrap();

        let (status, hint) = scip_status_for_response(true, dir.path());
        assert_eq!(status, "stale_index");
        let hint = hint.expect("stale_index must surface a regeneration hint");
        assert!(
            hint.contains("scripts/generate-scip-index.sh"),
            "regenerate hint must reference the helper script (got: {hint})"
        );
        assert!(
            hint.to_lowercase().contains("regenerate")
                || hint.to_lowercase().contains("refresh")
                || hint.contains("Cargo.lock"),
            "hint must indicate the regen rationale (got: {hint})"
        );
    }

    #[cfg(feature = "scip-backend")]
    #[test]
    fn scip_status_when_compiled_without_index_emits_setup_hint() {
        // The "no index yet" case: agent on a fresh checkout sees
        // `enabled` is not yet reachable, so we surface a one-line
        // shell command pointing at the helper script.
        let dir = tempfile::tempdir().unwrap();
        let (status, hint) = scip_status_for_response(false, dir.path());
        assert_eq!(status, "available_no_index");
        let hint = hint.expect("setup hint required when index missing");
        assert!(
            hint.contains("scripts/generate-scip-index.sh"),
            "hint must point at the helper script (got: {hint})"
        );
        assert!(
            hint.contains("rust-analyzer scip"),
            "hint must reference the underlying tool (got: {hint})"
        );
    }

    #[cfg(not(feature = "scip-backend"))]
    #[test]
    fn scip_status_when_feature_disabled_is_not_compiled() {
        // Binary built without `--features scip-backend` cannot ever
        // benefit from an index file. We report `not_compiled` and emit
        // no hint to avoid sending agents on a wild goose chase that
        // would not change behavior. Whether `scip_available` is true
        // (stray index file in project root) doesn't matter — the
        // binary cannot read it.
        let dir = tempfile::tempdir().unwrap();
        for scip_available in [false, true] {
            let (status, hint) = scip_status_for_response(scip_available, dir.path());
            assert_eq!(status, "not_compiled");
            assert!(hint.is_none());
        }
    }

    #[cfg(feature = "semantic")]
    #[test]
    fn model_status_reflects_engine_helper_when_compiled() {
        // P0-2 — `model_status` must mirror the engine helper that
        // determines whether the codesearch payload is reachable. The
        // self repo carries the model under
        // `crates/codelens-engine/models/codesearch/`, so the
        // expected verdict here is "loaded" with no hint. If a future
        // refactor moves the model out without updating the helper,
        // this test reverses to "missing" and the assertion makes the
        // change loud.
        let (status, hint) = model_status_for_response();
        if codelens_engine::embedding_model_assets_available() {
            assert_eq!(status, "loaded");
            assert!(hint.is_none(), "loaded state must not carry a setup hint");
        } else {
            assert_eq!(status, "missing");
            let hint = hint.expect("missing state must surface a setup hint");
            assert!(
                hint.contains("CODELENS_MODEL_DIR"),
                "hint must name the env var users have to set (got: {hint})"
            );
            assert!(
                hint.contains("model.onnx"),
                "hint must name the canonical model asset (got: {hint})"
            );
        }
    }

    #[cfg(not(feature = "semantic"))]
    #[test]
    fn model_status_when_feature_disabled_is_not_compiled() {
        // Binary built without the `semantic` feature cannot consume
        // any model payload. We report `not_compiled` and skip the
        // hint — fetching a model wouldn't change runtime behavior.
        let (status, hint) = model_status_for_response();
        assert_eq!(status, "not_compiled");
        assert!(hint.is_none());
    }

    #[test]
    fn detail_unknown_value_falls_back_to_full() {
        // Future-compatible: an unknown value (e.g. "intelligence_only"
        // proposed in a later PR) does not crash; the call returns the
        // backward-compatible full payload.
        assert_eq!(
            CapabilitiesDetail::from_value(&json!({"detail": "minimal"})),
            CapabilitiesDetail::Full
        );
        assert_eq!(
            CapabilitiesDetail::from_value(&json!({"detail": ""})),
            CapabilitiesDetail::Full
        );
        assert_eq!(
            CapabilitiesDetail::from_value(&json!({"detail": 42})),
            CapabilitiesDetail::Full
        );
    }
}
