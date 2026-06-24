use crate::AppState;
use crate::tool_defs::ToolSurface;
use serde_json::json;

#[cfg(feature = "semantic")]
use crate::tool_defs::is_tool_in_surface;

/// Five-way decomposition of why `semantic_search` might not be
/// currently runnable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SemanticSearchStatus {
    #[cfg(feature = "semantic")]
    Available,
    #[cfg(feature = "semantic")]
    ModelAssetsUnavailable,
    #[cfg(feature = "semantic")]
    NotInActiveSurface,
    /// No embedding index exists on disk — call `index_embeddings` to build.
    #[cfg(feature = "semantic")]
    IndexMissing,
    /// An embedding index exists but was built with an older schema version.
    /// The daemon will automatically recreate it on next load; call
    /// `index_embeddings` to trigger the rebuild now.
    #[cfg(feature = "semantic")]
    IndexSchemaOutdated,
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
            #[cfg(feature = "semantic")]
            Self::IndexSchemaOutdated => "index_schema_outdated",
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
            #[cfg(feature = "semantic")]
            Self::IndexSchemaOutdated => Some(
                "embedding index was built with an older schema and will be recreated on next daemon load — call index_embeddings to rebuild now",
            ),
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
            #[cfg(feature = "semantic")]
            Self::IndexSchemaOutdated => Some("semantic_index_schema_outdated"),
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
            #[cfg(feature = "semantic")]
            Self::IndexSchemaOutdated => Some("run_index_embeddings"),
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
            #[cfg(feature = "semantic")]
            Self::IndexSchemaOutdated => Some("embedding_index"),
            Self::FeatureDisabled => Some("binary"),
        }
    }

    pub(crate) fn included_in_profiles(&self) -> Option<Vec<&'static str>> {
        #[cfg(feature = "semantic")]
        {
            match self {
                Self::NotInActiveSurface => Some(vec![
                    "planner-readonly",
                    "builder-minimal",
                    "reviewer-graph",
                ]),
                _ => None,
            }
        }
        #[cfg(not(feature = "semantic"))]
        {
            None
        }
    }

    pub(crate) fn is_schema_outdated(&self) -> bool {
        #[cfg(feature = "semantic")]
        {
            matches!(self, Self::IndexSchemaOutdated)
        }
        #[cfg(not(feature = "semantic"))]
        {
            false
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
        if self.is_schema_outdated() {
            payload["schema_outdated"] = serde_json::json!(true);
        }
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

/// Compute the current `SemanticSearchStatus` without loading the
/// embedding engine unless it is already resident.
///
/// Distinguishes five states:
/// - `Available`: engine loaded or on-disk index matches current schema.
/// - `ModelAssetsUnavailable`: ONNX model files missing.
/// - `NotInActiveSurface`: `semantic_search` not in the active tool surface.
/// - `IndexMissing`: no embedding index file on disk at all.
/// - `IndexSchemaOutdated`: index exists but was built with an older schema
///   version; the daemon will recreate it on load — call `index_embeddings`
///   to trigger the rebuild immediately.
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
    let guard = state.embedding_ref();
    if let Some(engine) = guard.as_ref() {
        // Engine is resident — trust its live count.
        let indexed_count = engine.index_info().indexed_symbols;
        if indexed_count == 0 {
            return SemanticSearchStatus::IndexMissing;
        }
        return SemanticSearchStatus::Available;
    }
    // Engine not yet loaded — probe the on-disk DB.
    // `inspect_existing_index` returns `Ok(None)` when the DB exists but
    // the stored `schema_version` does not match the compiled constant.
    // We need to distinguish that case from "no DB at all" so callers get
    // an actionable `IndexSchemaOutdated` rather than the misleading
    // `IndexMissing`.
    let db_path = state
        .project()
        .as_path()
        .join(".codelens/index/embeddings.db");
    let db_exists = db_path.exists();
    let index_info = codelens_engine::EmbeddingEngine::inspect_existing_index(&state.project())
        .ok()
        .flatten();
    match index_info {
        Some(info) if info.indexed_symbols > 0 => SemanticSearchStatus::Available,
        Some(_) => SemanticSearchStatus::IndexMissing, // DB valid schema but 0 rows
        None if db_exists => SemanticSearchStatus::IndexSchemaOutdated,
        None => SemanticSearchStatus::IndexMissing,
    }
}

#[cfg(not(feature = "semantic"))]
pub(crate) fn determine_semantic_search_status(
    _state: &AppState,
    _surface: ToolSurface,
) -> SemanticSearchStatus {
    SemanticSearchStatus::FeatureDisabled
}
